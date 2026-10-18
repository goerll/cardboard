//! Content-addressed asset store for images and fonts.
//!
//! Bytes live once per library in `<library>/.assets/<sha256>.<ext>`; documents reference them as
//! the root-relative URL `/asset/<sha256>.<ext>`, which the editor server serves, so the canvas,
//! thumbnails and inspector need no rewriting. A saved `.cardboard` file also embeds the assets it
//! references (see `store::write_document`), so a file copied to another machine or library still
//! renders; opening it extracts them into that library's store. `export_html` inlines them.
use crate::document::{Document, Node, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const PREFIX: &str = "/asset/";
pub const DIR: &str = ".assets";
/// Largest single asset (image or font file).
pub const MAX_BYTES: usize = 32 << 20;
const HASH_LEN: usize = 64;

/// The asset store that belongs to a `.cardboard` file: its library's, also for archived files.
pub fn dir_for_file(path: &Path) -> PathBuf {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let library = if parent.file_name().is_some_and(|n| n == ".archive") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    library.join(DIR)
}

pub fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}
pub fn is_font(ext: &str) -> bool {
    matches!(ext, "woff2" | "woff" | "ttf" | "otf")
}
/// The file type from the content itself, so a misnamed or hostile file is never stored.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    Some(if starts(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if starts(b"\xff\xd8\xff") {
        "jpg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "gif"
    } else if starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "webp"
    } else if bytes.get(4..12) == Some(b"ftypavif") || bytes.get(4..12) == Some(b"ftypavis") {
        "avif"
    } else if starts(b"\0\0\x01\0") {
        "ico"
    } else if starts(b"BM") && bytes.len() > 26 {
        "bmp"
    } else if starts(b"wOF2") {
        "woff2"
    } else if starts(b"wOFF") {
        "woff"
    } else if starts(b"\0\x01\0\0") || starts(b"true") {
        "ttf"
    } else if starts(b"OTTO") {
        "otf"
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
        let head = head.trim_start_matches('\u{feff}').trim_start();
        if (head.starts_with("<svg") || head.starts_with("<?xml") || head.starts_with("<!--"))
            && head.contains("<svg")
        {
            "svg"
        } else {
            return None;
        }
    })
}

/// `<sha256>.<ext>` with a known extension.
pub fn valid_name(name: &str) -> bool {
    name.split_once('.').is_some_and(|(hash, ext)| {
        hash.len() == HASH_LEN
            && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            && mime(ext) != "application/octet-stream"
    })
}
/// The asset name in a `/asset/<name>` reference, if `src` is exactly one.
pub fn ref_name(src: &str) -> Option<&str> {
    src.strip_prefix(PREFIX).filter(|n| valid_name(n))
}
pub fn reference(name: &str) -> String {
    format!("{PREFIX}{name}")
}

/// Store bytes (type checked by content) and return the asset name. Idempotent.
pub fn put(dir: &Path, bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "Assets are limited to {} MB",
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let ext = sniff(bytes).ok_or("Unsupported file: expected a PNG, JPEG, GIF, WebP, AVIF, SVG, ICO or BMP image, or a WOFF2, WOFF, TTF or OTF font")?;
    let name = format!("{}.{ext}", hex(&Sha256::digest(bytes)));
    let path = dir.join(&name);
    if !path.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        fs::write(&temp, bytes)
            .and_then(|_| fs::rename(&temp, &path))
            .map_err(|e| {
                let _ = fs::remove_file(&temp);
                format!("Could not store asset: {e}")
            })?;
    }
    Ok(name)
}
pub fn path(dir: &Path, name: &str) -> Option<PathBuf> {
    valid_name(name).then(|| dir.join(name))
}
pub fn read(dir: &Path, name: &str) -> Option<Vec<u8>> {
    fs::read(path(dir, name)?).ok()
}
/// Write an embedded asset, but only if its content matches its name.
pub fn restore(dir: &Path, name: &str, base64: &str) -> Result<()> {
    let Some(target) = path(dir, name) else {
        return Ok(());
    };
    if target.exists() {
        return Ok(());
    }
    let bytes = STANDARD
        .decode(base64)
        .map_err(|e| format!("Embedded asset {name} is not valid base64: {e}"))?;
    if !name.starts_with(&hex(&Sha256::digest(&bytes))) {
        return Err(format!("Embedded asset {name} does not match its content"));
    }
    put(dir, &bytes).map(|_| ())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Every asset name referenced anywhere in `text`.
pub fn names_in(text: &str, out: &mut BTreeSet<String>) {
    let mut rest = text;
    while let Some(i) = rest.find(PREFIX) {
        rest = &rest[i + PREFIX.len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
            .unwrap_or(rest.len());
        if valid_name(&rest[..end]) {
            out.insert(rest[..end].into());
        }
    }
}
fn each_value(doc: &mut Document, f: &mut impl FnMut(&mut String)) {
    fn visit(n: &mut Node, f: &mut impl FnMut(&mut String)) {
        n.attributes.values_mut().for_each(&mut *f);
        n.styles.values_mut().for_each(&mut *f);
        for c in &mut n.children {
            visit(c, f);
        }
    }
    for n in doc.pages.iter_mut().flat_map(|p| &mut p.artboards) {
        visit(n, f);
    }
    doc.tokens.values_mut().for_each(&mut *f);
    for face in &mut doc.fonts {
        f(&mut face.src);
    }
}
/// Assets a document refers to, from attributes, styles, tokens and fonts.
pub fn references(doc: &Document) -> BTreeSet<String> {
    fn visit(n: &Node, out: &mut BTreeSet<String>) {
        for v in n.attributes.values().chain(n.styles.values()) {
            names_in(v, out);
        }
        for c in &n.children {
            visit(c, out);
        }
    }
    let mut out = BTreeSet::new();
    for n in doc.pages.iter().flat_map(|p| &p.artboards) {
        visit(n, &mut out);
    }
    for v in doc.tokens.values().chain(doc.fonts.iter().map(|f| &f.src)) {
        names_in(v, &mut out);
    }
    out
}

/// Move base64 `data:` images out of the document into the store. Returns whether anything moved.
/// Runs after every edit and when a file opens, so pasted images and older files stay small.
pub fn externalize(doc: &mut Document, dir: &Path) -> Result<bool> {
    let mut changed = false;
    let mut error = None;
    each_value(doc, &mut |value| {
        if error.is_some() || !value.contains(";base64,") {
            return;
        }
        match externalize_text(value, dir) {
            Ok(Some(v)) => {
                *value = v;
                changed = true;
            }
            Ok(None) => {}
            Err(e) => error = Some(e),
        }
    });
    error.map_or(Ok(changed), Err)
}
fn externalize_text(text: &str, dir: &Path) -> Result<Option<String>> {
    let mut out = String::new();
    let mut rest = text;
    let mut changed = false;
    while let Some(i) = rest.find("data:image/") {
        let after = &rest[i..];
        let Some(comma) = after
            .find(";base64,")
            .filter(|c| !after[..*c].contains([' ', '"', '\'', ')']))
        else {
            out.push_str(&rest[..i + 11]);
            rest = &rest[i + 11..];
            continue;
        };
        let payload_start = comma + 8;
        let payload_len = after[payload_start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')))
            .unwrap_or(after.len() - payload_start);
        let payload = &after[payload_start..payload_start + payload_len];
        match STANDARD.decode(payload).ok().filter(|b| sniff(b).is_some()) {
            Some(bytes) => {
                out.push_str(&rest[..i]);
                out.push_str(&reference(&put(dir, &bytes)?));
                changed = true;
            }
            None => out.push_str(&rest[..i + payload_start + payload_len]),
        }
        rest = &after[payload_start + payload_len..];
    }
    out.push_str(rest);
    Ok(changed.then_some(out))
}

pub enum Embed<'a> {
    /// `data:` URIs: one self-contained file.
    Inline,
    /// `file://` URLs into the store: small output for viewing on this machine.
    Files(&'a Path),
}
/// Rewrite `/asset/` references in exported HTML. Missing assets keep their reference.
pub fn embed(html: &str, dir: &Path, mode: Embed) -> String {
    let mut names = BTreeSet::new();
    names_in(html, &mut names);
    let mut out = html.to_string();
    for name in names {
        let ext = name.rsplit('.').next().unwrap_or("");
        let target = match mode {
            Embed::Inline => match read(dir, &name) {
                Some(bytes) => format!("data:{};base64,{}", mime(ext), encode(&bytes)),
                None => continue,
            },
            Embed::Files(dir) => {
                let path = dir.join(&name);
                if !path.exists() {
                    continue;
                }
                let path = path.to_string_lossy().replace('\\', "/");
                format!(
                    "file://{}{path}",
                    if path.starts_with('/') { "" } else { "/" }
                )
            }
        };
        out = out.replace(&reference(&name), &target);
    }
    out
}

/// Pixel size of PNG, GIF, JPEG and WebP images.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    let be16 = |i: usize| Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    let le16 = |i: usize| Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as u32);
    let be32 = |i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let le24 = |i: usize| Some(le16(i)? | (*b.get(i + 2)? as u32) << 16);
    match sniff(b)? {
        "png" => Some((be32(16)?, be32(20)?)),
        "gif" => Some((le16(6)?, le16(8)?)),
        "webp" => match b.get(12..16)? {
            b"VP8 " => Some((le16(26)? & 0x3fff, le16(28)? & 0x3fff)),
            b"VP8L" => {
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
            }
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            _ => None,
        },
        "jpg" => {
            let mut i = 2;
            while i + 9 < b.len() {
                if b[i] != 0xff {
                    i += 1;
                    continue;
                }
                let marker = b[i + 1];
                if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                    return Some((be16(i + 7)?, be16(i + 5)?));
                }
                i += 2 + be16(i + 2)? as usize;
            }
            None
        }
        _ => None,
    }
}

// Network: only ever called from tools that the person or agent invoked, never while rendering.

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("Cardboard/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .build()
        .into()
}
fn offline(url: &str, e: impl std::fmt::Display) -> String {
    let host = url.split('/').nth(2).unwrap_or(url);
    format!(
        "Could not reach {host} ({e}). Check the internet connection; local files (path) and data URIs work offline."
    )
}
/// GET a URL, failing clearly offline, on HTTP errors, or past `limit` bytes.
pub fn fetch(url: &str, headers: &[(&str, &str)], limit: usize) -> Result<Vec<u8>> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http and https URLs can be fetched".into());
    }
    let mut request = agent().get(url);
    for (k, v) in headers {
        request = request.header(*k, *v);
    }
    let mut response = request.call().map_err(|e| offline(url, e))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .body_mut()
            .with_config()
            .limit(4096)
            .read_to_string()
            .unwrap_or_default();
        return Err(format!(
            "{url} answered HTTP {}{}",
            status.as_u16(),
            if body.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", body.trim().chars().take(300).collect::<String>())
            }
        ));
    }
    let mut bytes = vec![];
    response
        .body_mut()
        .as_reader()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| offline(url, e))?;
    if bytes.len() > limit {
        return Err(format!("{url} is larger than {} MB", limit / (1024 * 1024)));
    }
    Ok(bytes)
}

/// Bytes from exactly one of `url`, `path` or `dataUri`.
pub fn load(args: &serde_json::Value) -> Result<Vec<u8>> {
    let get = |k: &str| args.get(k).and_then(serde_json::Value::as_str);
    match (get("url"), get("path"), get("dataUri")) {
        (Some(url), None, None) => fetch(url, &[], MAX_BYTES),
        (None, Some(path), None) => {
            let path = path.strip_prefix("file://").unwrap_or(path);
            let size = fs::metadata(path)
                .map_err(|e| format!("Could not read {path}: {e}"))?
                .len();
            if size > MAX_BYTES as u64 {
                return Err(format!("{path} is larger than {} MB", MAX_BYTES >> 20));
            }
            fs::read(path).map_err(|e| format!("Could not read {path}: {e}"))
        }
        (None, None, Some(uri)) => {
            let (_, data) = uri
                .strip_prefix("data:")
                .and_then(|u| u.split_once(";base64,"))
                .ok_or("dataUri must be a base64 data: URI")?;
            STANDARD
                .decode(data.trim())
                .map_err(|e| format!("dataUri is not valid base64: {e}"))
        }
        _ => Err("Give exactly one of url, path or dataUri".into()),
    }
}
