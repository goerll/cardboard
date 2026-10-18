//! Document fonts: `@font-face` declarations whose files live in the asset store, from Google
//! Fonts or local files, plus the families installed on this machine.
use crate::{assets, document::Result, stock::query_escape};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FontFace {
    pub family: String,
    /// An asset reference, `/asset/<sha256>.woff2` (or woff, ttf, otf).
    pub src: String,
    /// `400`, or a variable range like `100 900`.
    pub weight: String,
    /// `normal` or `italic`.
    pub style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unicode_range: Option<String>,
    /// Where it came from: `google` or `file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub fn validate_family(family: &str) -> Result<()> {
    if family.trim().is_empty()
        || family.len() > 100
        || family.contains(['"', '\\', ';', '{', '}', '<', '>', '\''])
        || family.chars().any(char::is_control)
    {
        return Err(format!("Invalid font family name: {family:?}"));
    }
    Ok(())
}
fn valid_weight(weight: &str) -> bool {
    let number = |n: &str| n.parse::<u16>().is_ok_and(|n| (1..=1000).contains(&n));
    match weight.split_once(' ') {
        Some((a, b)) => number(a) && number(b),
        None => number(weight) || matches!(weight, "normal" | "bold"),
    }
}
pub fn validate(face: &FontFace) -> Result<()> {
    validate_family(&face.family)?;
    let font_asset = assets::ref_name(&face.src)
        .is_some_and(|n| assets::is_font(n.rsplit('.').next().unwrap_or("")));
    if !font_asset {
        return Err(format!(
            "Font {} must point at a font in the asset store",
            face.family
        ));
    }
    if !valid_weight(&face.weight) {
        return Err(format!("Invalid font weight {:?}", face.weight));
    }
    if !matches!(face.style.as_str(), "normal" | "italic" | "oblique") {
        return Err("Font style must be normal, italic or oblique".into());
    }
    if let Some(range) = &face.unicode_range
        && !range
            .chars()
            .all(|c| c.is_ascii_hexdigit() || matches!(c, 'U' | 'u' | '+' | '-' | '?' | ',' | ' '))
    {
        return Err("Invalid unicode-range".into());
    }
    Ok(())
}

/// `@font-face` rules for a document's fonts. References stay `/asset/...`; exports inline them.
pub fn css(faces: &[FontFace]) -> String {
    faces
        .iter()
        .map(|f| {
            let format = match f.src.rsplit('.').next().unwrap_or("") {
                "woff2" => "woff2",
                "woff" => "woff",
                "otf" => "opentype",
                _ => "truetype",
            };
            let range = f
                .unicode_range
                .as_ref()
                .map(|r| format!(";unicode-range:{r}"))
                .unwrap_or_default();
            format!(
                "@font-face{{font-family:\"{}\";src:url(\"{}\") format(\"{format}\");font-weight:{};font-style:{};font-display:swap{range}}}",
                f.family, f.src, f.weight, f.style
            )
        })
        .collect()
}

/// One entry per family: the weights and styles it has, for `get_basic_info` and `list_fonts`.
pub fn summary(faces: &[FontFace]) -> Vec<Value> {
    type Family<'a> = (BTreeSet<&'a str>, BTreeSet<&'a str>, Option<&'a str>);
    let mut families: BTreeMap<&str, Family> = BTreeMap::new();
    for f in faces {
        let entry = families.entry(&f.family).or_default();
        entry.0.insert(&f.weight);
        entry.1.insert(&f.style);
        entry.2 = entry.2.or(f.source.as_deref());
    }
    families
        .into_iter()
        .map(|(family, (weights, styles, source))| {
            json!({"family":family,"weights":weights,"styles":styles,"source":source})
        })
        .collect()
}

/// Add faces, replacing any with the same family, weight, style and range.
pub fn merge(fonts: &mut Vec<FontFace>, faces: Vec<FontFace>) {
    for face in faces {
        fonts.retain(|f| {
            !(f.family == face.family
                && f.weight == face.weight
                && f.style == face.style
                && f.unicode_range == face.unicode_range)
        });
        fonts.push(face);
    }
}

// Google Fonts

const BROWSER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

pub fn google_url(family: &str, weights: &[String], italic: bool) -> Result<String> {
    validate_family(family)?;
    let mut weights: Vec<&String> = weights.iter().collect();
    for w in &weights {
        let ok = match w.split_once("..") {
            Some((a, b)) => valid_weight(a) && valid_weight(b),
            None => w.parse::<u16>().is_ok() && valid_weight(w),
        };
        if !ok {
            return Err(format!(
                "Invalid weight {w:?}; use numbers like \"400\" or a range like \"100..900\""
            ));
        }
    }
    // Google requires tuples in ascending order.
    weights.sort_by_key(|w| {
        w.split("..")
            .next()
            .unwrap_or("")
            .parse::<u16>()
            .unwrap_or(0)
    });
    weights.dedup();
    let family = query_escape(family.trim());
    let axes = if italic {
        let list = |ital: u8| weights.iter().map(move |w| format!("{ital},{w}"));
        format!(
            "ital,wght@{}",
            list(0).chain(list(1)).collect::<Vec<_>>().join(";")
        )
    } else {
        format!(
            "wght@{}",
            weights
                .iter()
                .map(|w| w.as_str())
                .collect::<Vec<_>>()
                .join(";")
        )
    };
    Ok(format!(
        "https://fonts.googleapis.com/css2?family={family}:{axes}&display=swap"
    ))
}

/// Faces from a Google Fonts stylesheet as (weight, style, unicode-range, file URL), keeping the
/// subsets asked for (`all` keeps every subset). Blocks without a subset comment are always kept.
pub fn parse_google_css(
    css: &str,
    subsets: &[String],
) -> Vec<(String, String, Option<String>, String)> {
    let all = subsets.iter().any(|s| s == "all");
    let mut out = vec![];
    for block in css.split('}') {
        let Some((head, body)) = block.split_once('{') else {
            continue;
        };
        let subset = head
            .split_once("/*")
            .and_then(|(_, c)| c.split_once("*/"))
            .map(|(c, _)| c.trim().to_string());
        if !all && subset.as_ref().is_some_and(|s| !subsets.contains(s)) {
            continue;
        }
        let mut decl = BTreeMap::new();
        for line in body.split(';') {
            if let Some((k, v)) = line.split_once(':') {
                decl.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        let url = decl
            .get("src")
            .and_then(|s| s.split_once("url(")?.1.split_once(')'))
            .map(|(u, _)| u.trim_matches(['"', '\'']).to_string());
        let Some(url) = url else { continue };
        out.push((
            decl.get("font-weight")
                .cloned()
                .unwrap_or_else(|| "400".into()),
            decl.get("font-style")
                .cloned()
                .unwrap_or_else(|| "normal".into()),
            decl.get("unicode-range").cloned(),
            url,
        ));
    }
    out
}

/// Download a Google Fonts family into the asset store.
pub fn google(
    dir: &Path,
    family: &str,
    weights: &[String],
    italic: bool,
    subsets: &[String],
) -> Result<Vec<FontFace>> {
    let url = google_url(family, weights, italic)?;
    let css = assets::fetch(&url, &[("User-Agent", BROWSER_AGENT)], 1 << 20).map_err(|e| {
        if e.contains("HTTP 400") {
            format!(
                "Google Fonts has no family {family:?} with weights {} (check the spelling and the weights it offers on fonts.google.com)",
                weights.join(", ")
            )
        } else {
            e
        }
    })?;
    let css = String::from_utf8_lossy(&css);
    let parsed = parse_google_css(&css, subsets);
    if parsed.is_empty() {
        return Err(format!(
            "Google Fonts returned no files for {family:?} in subsets {}",
            subsets.join(", ")
        ));
    }
    let mut stored: BTreeMap<String, String> = BTreeMap::new();
    let mut faces = vec![];
    for (weight, style, range, file) in parsed {
        if !stored.contains_key(&file) {
            let bytes = assets::fetch(&file, &[], assets::MAX_BYTES)?;
            let name = assets::put(dir, &bytes)?;
            if !assets::is_font(name.rsplit('.').next().unwrap_or("")) {
                return Err(format!("{file} is not a font file"));
            }
            stored.insert(file.clone(), name);
        }
        faces.push(FontFace {
            family: family.trim().into(),
            src: assets::reference(&stored[&file]),
            weight,
            style,
            unicode_range: range,
            source: Some("google".into()),
        });
    }
    Ok(faces)
}

// Installed fonts

/// Family names of the fonts installed on this machine, read once from the system font folders.
pub fn installed() -> &'static [String] {
    static INSTALLED: OnceLock<Vec<String>> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let mut families = BTreeSet::new();
        let mut stack = font_dirs();
        let mut seen = 0usize;
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                    seen += 1;
                    if seen > 20_000 {
                        break;
                    }
                    families.extend(file_families(&path).unwrap_or_default());
                }
            }
        }
        families
            .into_iter()
            .filter(|f: &String| !f.starts_with('.') && !f.trim().is_empty())
            .collect()
    })
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    #[cfg(target_os = "macos")]
    let home = crate::store::home_dir();
    #[cfg(target_os = "macos")]
    dirs.extend([
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
        home.join("Library/Fonts"),
    ]);
    #[cfg(windows)]
    {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(PathBuf::from(windir).join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let home = crate::store::home_dir();
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        dirs.extend([
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            data.join("fonts"),
            home.join(".fonts"),
        ]);
    }
    dirs
}

/// Family names in a TrueType/OpenType file or collection (name table IDs 16, else 1).
pub fn file_families(path: &Path) -> Option<Vec<String>> {
    let mut file = fs::File::open(path).ok()?;
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        let mut buf = vec![0; len];
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let u16_at = |b: &[u8], i: usize| Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |b: &[u8], i: usize| Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?));
    let header = read_at(0, 12)?;
    let offsets: Vec<u64> = if &header[..4] == b"ttcf" {
        let count = u32_at(&header, 8)?.min(64) as usize;
        let table = read_at(12, count * 4)?;
        (0..count)
            .filter_map(|i| u32_at(&table, i * 4).map(u64::from))
            .collect()
    } else {
        vec![0]
    };
    let mut names = vec![];
    for offset in offsets {
        let head = read_at(offset, 12)?;
        let tables = u16_at(&head, 4)? as usize;
        let directory = read_at(offset + 12, tables.min(512) * 16)?;
        let Some(record) = directory.chunks(16).find(|r| &r[..4] == b"name") else {
            continue;
        };
        let (start, len) = (u32_at(record, 8)? as u64, u32_at(record, 12)? as usize);
        let table = read_at(start, len.min(1 << 20))?;
        let count = u16_at(&table, 2)? as usize;
        let strings = u16_at(&table, 4)? as usize;
        let mut best: Option<(u8, String)> = None;
        for i in 0..count {
            let r = 6 + i * 12;
            let (platform, encoding, language, id) = (
                u16_at(&table, r)?,
                u16_at(&table, r + 2)?,
                u16_at(&table, r + 4)?,
                u16_at(&table, r + 6)?,
            );
            let (length, at) = (
                u16_at(&table, r + 8)? as usize,
                u16_at(&table, r + 10)? as usize,
            );
            let Some(raw) = table.get(strings + at..strings + at + length) else {
                continue;
            };
            let name = match (platform, encoding) {
                (3, 1 | 10) | (0, _) => String::from_utf16_lossy(
                    &raw.chunks(2)
                        .filter_map(|c| Some(u16::from_be_bytes(c.try_into().ok()?)))
                        .collect::<Vec<_>>(),
                ),
                (1, 0) => raw.iter().map(|&b| b as char).collect(),
                _ => continue,
            };
            // Typographic family beats legacy family; English beats other languages.
            let english = language == 0x409 || platform == 1 && language == 0 || platform == 0;
            let rank = match id {
                16 => 2,
                1 => 0,
                _ => continue,
            } + u8::from(english);
            if best.as_ref().is_none_or(|(b, _)| rank > *b) {
                best = Some((rank, name.trim().to_string()));
            }
        }
        names.extend(best.map(|(_, n)| n));
    }
    Some(names)
}
