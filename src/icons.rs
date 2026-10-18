//! Icons from Iconify (https://iconify.design): Lucide, Phosphor, Tabler, Heroicons, Material
//! Symbols and ~200 more sets behind one API. An inserted icon is ONE layer: an `<svg>` node whose
//! sanitized inner markup lives in `Node::markup`. Fetched icons are cached on disk, so repeat use
//! and offline work need no network; network calls happen before the store is locked
//! (`prefetch`), never while rendering.
use crate::{
    document::{Document, Node, Result},
    html,
};
use quick_xml::{Reader, events::Event};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const API: &str = "https://api.iconify.design";
static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// `$CARDBOARD_ICON_CACHE`, else the platform cache directory + `cardboard/icons`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = CACHE_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return dir;
    }
    if let Some(dir) = std::env::var_os("CARDBOARD_ICON_CACHE") {
        return dir.into();
    }
    let home = crate::store::home_dir();
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
    } else if cfg!(target_os = "macos") {
        home.join("Library/Caches")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
    };
    base.join("cardboard/icons")
}
/// Point the cache somewhere else for this process (tests, hosts with their own data dir).
pub fn set_cache_dir(dir: PathBuf) {
    *CACHE_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir);
}

/// One icon, already resolved from aliases and transforms: inner SVG plus its viewBox.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub body: String,
    pub view_box: [f64; 4],
}

/// Split `set:name` (Iconify ids are lowercase letters, digits and hyphens).
fn split(name: &str) -> Result<(&str, &str)> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    match name.split_once(':') {
        Some((set, icon)) if valid(set) && valid(icon) => Ok((set, icon)),
        _ => Err(format!(
            "Icon names look like set:name, e.g. lucide:arrow-right (got \"{name}\"); find them with search_icons"
        )),
    }
}
fn cache_path(dir: &Path, name: &str) -> Result<PathBuf> {
    let (set, icon) = split(name)?;
    Ok(dir.join(set).join(format!("{icon}.json")))
}

/// Write an icon into the disk cache (used after fetching, and to seed caches in tests).
pub fn cache_icon(name: &str, icon: &Icon) -> Result<()> {
    let path = cache_path(&cache_dir(), name)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    // Unique per writer: concurrent fetches of the same icon must not share a temp file.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp{}-{n}", std::process::id()));
    let data = json!({"body":icon.body,"viewBox":icon.view_box});
    std::fs::write(&tmp, data.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}
/// The cached icon, or None if it was never fetched.
pub fn cached(name: &str) -> Result<Option<Icon>> {
    let path = cache_path(&cache_dir(), name)?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let data: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let view_box = data["viewBox"]
        .as_array()
        .filter(|v| v.len() == 4)
        .and_then(|v| {
            let n: Vec<f64> = v.iter().filter_map(Value::as_f64).collect();
            n.try_into().ok()
        })
        .ok_or_else(|| format!("Corrupt icon cache entry {}", path.display()))?;
    Ok(Some(Icon {
        body: data["body"].as_str().unwrap_or_default().into(),
        view_box,
    }))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(concat!("cardboard/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}
fn get(url: &str, query: &[(&str, &str)]) -> Result<Value> {
    let mut request = agent().get(url);
    for (k, v) in query {
        request = request.query(*k, *v);
    }
    let text = request
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| {
            format!(
                "Iconify request failed ({e}); check the internet connection. Icons used before stay available offline (cache: {}).",
                cache_dir().display()
            )
        })?;
    // Unknown sets answer with the bare number 404.
    serde_json::from_str(&text).map_err(|_| format!("Unexpected Iconify response: {text:.200}"))
}

/// Resolve one icon from an Iconify JSON collection response, following aliases and applying
/// rotate/flip the way Iconify's own renderer does.
fn resolve(data: &Value, icon: &str) -> Option<Icon> {
    let (mut rotate, mut h_flip, mut v_flip) = (0i64, false, false);
    let mut name = icon.to_string();
    let mut found = None;
    for _ in 0..8 {
        let entry = data["icons"]
            .get(&name)
            .or_else(|| data["aliases"].get(&name))?;
        rotate += entry["rotate"].as_i64().unwrap_or(0);
        h_flip ^= entry["hFlip"].as_bool().unwrap_or(false);
        v_flip ^= entry["vFlip"].as_bool().unwrap_or(false);
        if let Some(body) = entry["body"].as_str() {
            found = Some((body.to_string(), entry));
            break;
        }
        name = entry["parent"].as_str()?.into();
    }
    let (mut body, entry) = found?;
    let num = |k: &str, default: f64| {
        entry[k]
            .as_f64()
            .or_else(|| data[k].as_f64())
            .unwrap_or(default)
    };
    let (mut left, mut top, mut width, mut height) = (
        num("left", 0.0),
        num("top", 0.0),
        num("width", 16.0),
        num("height", 16.0),
    );
    let mut transforms = vec![];
    if h_flip && v_flip {
        rotate += 2;
    } else if h_flip {
        transforms.push(format!(
            "translate({} {}) scale(-1 1)",
            width + left,
            0.0 - top
        ));
        (left, top) = (0.0, 0.0);
    } else if v_flip {
        transforms.push(format!(
            "translate({} {}) scale(1 -1)",
            0.0 - left,
            height + top
        ));
        (left, top) = (0.0, 0.0);
    }
    match rotate.rem_euclid(4) {
        1 => {
            let c = height / 2.0 + top;
            transforms.insert(0, format!("rotate(90 {c} {c})"));
        }
        2 => transforms.insert(
            0,
            format!("rotate(180 {} {})", width / 2.0 + left, height / 2.0 + top),
        ),
        3 => {
            let c = width / 2.0 + left;
            transforms.insert(0, format!("rotate(-90 {c} {c})"));
        }
        _ => {}
    }
    if rotate.rem_euclid(2) == 1 {
        (left, top) = (top, left);
        (width, height) = (height, width);
    }
    if !transforms.is_empty() {
        body = format!("<g transform=\"{}\">{body}</g>", transforms.join(" "));
    }
    Some(Icon {
        body,
        view_box: [left, top, width, height],
    })
}

/// Fetch and cache icons that are not cached yet, one request per set.
pub fn fetch(names: &[String]) -> Result<()> {
    let mut by_set: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for name in names {
        if cached(name)?.is_none() {
            let (set, icon) = split(name)?;
            by_set.entry(set).or_default().push(icon);
        }
    }
    for (set, icons) in by_set {
        let data = get(&format!("{API}/{set}.json"), &[("icons", &icons.join(","))])?;
        if !data.is_object() {
            return Err(format!(
                "Unknown icon set \"{set}\"; find icons with search_icons"
            ));
        }
        for icon in icons {
            let resolved = resolve(&data, icon).ok_or_else(|| {
                format!("Icon {set}:{icon} does not exist; find icons with search_icons")
            })?;
            // Reject unsupported markup now rather than caching it.
            sanitize(&resolved.body)
                .map_err(|e| format!("Icon {set}:{icon} cannot be used: {e}"))?;
            cache_icon(&format!("{set}:{icon}"), &resolved)?;
        }
    }
    Ok(())
}

/// Icon names a tool call will need: `insert_icon`, `<icon name>` in `write_html`, and `batch`.
pub fn referenced(tool: &str, args: &Value) -> Vec<String> {
    match tool {
        "insert_icon" => args["icon"]
            .as_str()
            .map(String::from)
            .into_iter()
            .collect(),
        // Unreadable or malformed input is reported by the importer itself.
        "write_html" => crate::tools::html_source(args)
            .map(|html| crate::import::icon_names(&html))
            .unwrap_or_default(),
        "batch" => args["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|op| referenced(op["tool"].as_str().unwrap_or_default(), &op["args"]))
            .collect(),
        _ => vec![],
    }
}
/// Download the icons a tool call needs. Call without holding the store lock.
pub fn prefetch(tool: &str, args: &Value) -> Result<()> {
    let names = referenced(tool, args);
    if names.is_empty() {
        return Ok(());
    }
    fetch(&names)
}

/// `search_icons`: Iconify search, falling back to cached icon names when offline.
pub fn search(args: &Value) -> Result<Value> {
    let query = args["query"]
        .as_str()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("query must be a non-empty string")?;
    let limit = args["limit"].as_u64().unwrap_or(24).clamp(1, 200) as usize;
    let sets: Vec<String> = match &args["sets"] {
        Value::String(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        _ => vec![],
    };
    let mut query_args = vec![("query", query.to_string())];
    // The API rejects limits below 32; trim locally.
    query_args.push(("limit", limit.max(32).to_string()));
    if !sets.is_empty() {
        query_args.push(("prefixes", sets.join(",")));
    }
    let pairs: Vec<(&str, &str)> = query_args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    match get(&format!("{API}/search"), &pairs) {
        Ok(data) => {
            let icons: Vec<&str> = data["icons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .take(limit)
                .collect();
            Ok(json!({"icons":icons,"total":data["total"],"source":"iconify"}))
        }
        Err(network) => {
            let icons = search_cache(query, &sets, limit);
            if icons.is_empty() {
                return Err(format!("{network} No cached icon matches \"{query}\"."));
            }
            Ok(
                json!({"icons":icons,"source":"cache","note":"Offline: only icons used before are listed"}),
            )
        }
    }
}
fn search_cache(query: &str, sets: &[String], limit: usize) -> Vec<String> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let mut found = vec![];
    let Ok(dirs) = std::fs::read_dir(cache_dir()) else {
        return found;
    };
    for dir in dirs.flatten() {
        let set = dir.file_name().to_string_lossy().into_owned();
        if !sets.is_empty() && !sets.contains(&set) {
            continue;
        }
        for file in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let file = file.file_name().to_string_lossy().into_owned();
            if let Some(icon) = file.strip_suffix(".json")
                && words.iter().all(|w| icon.contains(w.as_str()))
            {
                found.push(format!("{set}:{icon}"));
            }
        }
    }
    found.sort();
    found.truncate(limit);
    found
}

/// Size, stroke width and color options shared by `insert_icon` and `<icon>`.
#[derive(Default)]
pub struct Options<'a> {
    pub size: Option<&'a str>,
    pub stroke_width: Option<&'a str>,
    pub color: Option<&'a str>,
}
/// Build the single-layer `<svg>` node for a cached icon.
pub fn node(doc: &mut Document, name: &str, options: Options) -> Result<Node> {
    let icon = cached(name)?.ok_or_else(|| {
        format!("Icon {name} is not cached yet; insert it through the running editor (it downloads icons) or check the name with search_icons")
    })?;
    let mut tree = parse(&icon.body)?;
    if let Some(width) = options.stroke_width {
        if !width.parse::<f64>().is_ok_and(|w| w > 0.0 && w.is_finite()) {
            return Err("strokeWidth must be a positive number".into());
        }
        if !set_stroke_width(&mut tree, width) {
            return Err(format!(
                "{name} is a filled icon; strokeWidth applies to outline sets such as lucide or tabler"
            ));
        }
    }
    let [left, top, w, h] = icon.view_box;
    let (_, short) = split(name)?;
    let mut node = Node::new(doc.allocate(), "svg", short);
    node.attributes.extend([
        ("xmlns".into(), "http://www.w3.org/2000/svg".into()),
        ("viewBox".into(), format!("{left} {top} {w} {h}")),
        ("data-icon".into(), name.into()),
        ("aria-hidden".into(), "true".into()),
    ]);
    let size = options.size.unwrap_or("24").trim();
    let (width, height) = match size.parse::<f64>() {
        Ok(px) if px > 0.0 && px.is_finite() => (
            format!("{}px", (px * w / h * 100.0).round() / 100.0),
            format!("{px}px"),
        ),
        Ok(_) => return Err("size must be positive".into()),
        Err(_) => (
            if w == h { size.into() } else { "auto".into() },
            size.into(),
        ),
    };
    node.styles.insert("width".into(), width);
    node.styles.insert("height".into(), height);
    // Icons in flex rows should keep their size next to long text.
    node.styles.insert("flex-shrink".into(), "0".into());
    if let Some(color) = options.color {
        node.styles.insert("color".into(), color.trim().into());
    }
    for (k, v) in &node.styles {
        html::validate_style(k, v)?;
    }
    node.markup = Some(write(&tree));
    Ok(node)
}

/// A parsed SVG element. Sanitizing means parsing into this allowlisted shape and writing it back
/// out, so stored markup is always our own canonical serialization.
struct El {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<El>,
}
const ELEMENTS: &[&str] = &[
    "g",
    "path",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "rect",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "mask",
    "use",
];
fn parse(body: &str) -> Result<Vec<El>> {
    if body.len() > 256 * 1024 {
        return Err("Icon markup is too large".into());
    }
    let mut reader = Reader::from_str(body);
    reader.config_mut().check_end_names = true;
    let mut roots = vec![];
    let mut stack: Vec<El> = vec![];
    // Depth of skipped <title>/<desc> content.
    let mut skipping = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("Icon SVG is not well formed: {e}"))?;
        if skipping > 0 {
            match event {
                Event::Start(_) => skipping += 1,
                Event::End(_) => skipping -= 1,
                Event::Eof => break,
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if matches!(tag.as_str(), "title" | "desc") {
                    if matches!(event, Event::Start(_)) {
                        skipping = 1;
                    }
                    continue;
                }
                if !ELEMENTS.contains(&tag.as_str()) {
                    return Err(format!("<{tag}> is not allowed in icons"));
                }
                if stack.len() > 32 {
                    return Err("Icon nesting is too deep".into());
                }
                let mut el = El {
                    tag,
                    attrs: vec![],
                    children: vec![],
                };
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                    let value = a
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|e| e.to_string())?
                        .into_owned();
                    el.attrs.push(attribute(&key, value)?);
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(el);
                } else if let Some(parent) = stack.last_mut() {
                    parent.children.push(el);
                } else {
                    roots.push(el);
                }
            }
            Event::End(_) => {
                let el = stack.pop().ok_or("Unexpected closing tag in icon")?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(el);
                } else {
                    roots.push(el);
                }
            }
            Event::Text(t) if t.iter().all(u8::is_ascii_whitespace) => {}
            Event::Comment(_) => {}
            Event::Eof => break,
            _ => return Err("Icons may only contain SVG shapes".into()),
        }
    }
    if !stack.is_empty() {
        return Err("Unclosed element in icon".into());
    }
    Ok(roots)
}
fn attribute(key: &str, value: String) -> Result<(String, String)> {
    let key = if key == "xlink:href" { "href" } else { key };
    if key == "style" {
        let styles = html::parse_styles(&value)?;
        let css = styles
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";");
        check_urls(&css)?;
        return Ok((key.into(), css));
    }
    html::validate_attribute(key, &value).map_err(|e| format!("{e} in icon"))?;
    if key == "src" || key == "srcset" || (key == "href" && !value.starts_with('#')) {
        return Err("Icons cannot reference external resources".into());
    }
    check_urls(&value)?;
    Ok((key.into(), value))
}
/// Paint references must stay inside the icon: url(#id) only.
fn check_urls(value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    for (i, _) in lower.match_indices("url(") {
        let rest = lower[i + 4..].trim_start_matches([' ', '"', '\'']);
        if !rest.starts_with('#') {
            return Err("Icons cannot reference external resources".into());
        }
    }
    Ok(())
}
fn set_stroke_width(els: &mut [El], width: &str) -> bool {
    let mut found = false;
    for el in els {
        for (k, v) in &mut el.attrs {
            if k == "stroke-width" {
                *v = width.into();
                found = true;
            }
        }
        found |= set_stroke_width(&mut el.children, width);
    }
    found
}
fn write(els: &[El]) -> String {
    let mut out = String::new();
    for el in els {
        out.push('<');
        out.push_str(&el.tag);
        for (k, v) in &el.attrs {
            out.push_str(&format!(" {k}=\"{}\"", html::escape(v)));
        }
        if el.children.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            out.push_str(&write(&el.children));
            out.push_str(&format!("</{}>", el.tag));
        }
    }
    out
}
/// Canonical, allowlisted form of icon markup: no scripts, foreignObject, event handlers, text
/// or external references. Idempotent, so stored markup can be checked by re-sanitizing.
pub fn sanitize(body: &str) -> Result<String> {
    Ok(write(&parse(body)?))
}
/// Icon markup as JSX children (React attribute spelling).
pub fn jsx(markup: &str) -> String {
    fn react_name(k: &str) -> String {
        if k.starts_with("data-") || k.starts_with("aria-") {
            return k.into();
        }
        let mut out = String::new();
        let mut upper = false;
        for c in k.chars() {
            if c == '-' || c == ':' {
                upper = true;
            } else if upper {
                out.push(c.to_ascii_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        }
        if out == "class" {
            "className".into()
        } else {
            out
        }
    }
    fn walk(els: &[El], out: &mut String) {
        for el in els {
            out.push_str(&format!("<{}", el.tag));
            for (k, v) in &el.attrs {
                if k == "style" {
                    // Rare in icon sets; keep it as a React style object.
                    let styles = html::parse_styles(v).unwrap_or_default();
                    let map: serde_json::Map<String, Value> = styles
                        .iter()
                        .map(|(k, v)| (react_name(k), json!(v)))
                        .collect();
                    out.push_str(&format!(" style={{{}}}", Value::Object(map)));
                } else {
                    out.push_str(&format!(" {}={{{}}}", react_name(k), json!(v)));
                }
            }
            if el.children.is_empty() {
                out.push_str(" />");
            } else {
                out.push('>');
                walk(&el.children, out);
                out.push_str(&format!("</{}>", el.tag));
            }
        }
    }
    let mut out = String::new();
    walk(&parse(markup).unwrap_or_default(), &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_inherit_their_parent_and_apply_rotation() {
        let data = json!({"width":24,"height":24,
            "icons":{"arrow-right":{"body":"<path d=\"M5 12h14\"/>"}},
            "aliases":{"arrow-down":{"parent":"arrow-right","rotate":1},"arrow-left":{"parent":"arrow-right","hFlip":true}}});
        let right = resolve(&data, "arrow-right").unwrap();
        assert_eq!(right.view_box, [0.0, 0.0, 24.0, 24.0]);
        assert_eq!(right.body, "<path d=\"M5 12h14\"/>");
        let down = resolve(&data, "arrow-down").unwrap();
        assert!(down.body.starts_with("<g transform=\"rotate(90 12 12)\">"));
        let left = resolve(&data, "arrow-left").unwrap();
        assert!(left.body.contains("translate(24 0) scale(-1 1)"));
        assert!(resolve(&data, "missing").is_none());
    }
}
