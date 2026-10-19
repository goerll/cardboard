//! Stock photo search. Unsplash when an access key is configured (`UNSPLASH_ACCESS_KEY` or the
//! library setting `unsplashAccessKey`), else Openverse, which needs no key.
//!
//! Unsplash's API guidelines require hotlinking their image URLs, calling the photo's download
//! endpoint when it is used, and crediting the photographer and Unsplash; so Unsplash photos stay
//! remote and carry attribution. Openverse photos are openly licensed copies hosted by many sites,
//! so they are stored locally like any imported image, with their license attribution kept.
use crate::{assets, document::Result};
use serde_json::{Value, json};

const UTM: &str = "utm_source=cardboard&utm_medium=referral";

pub fn query_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
fn s<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    path.iter()
        .try_fold(v, |v, k| v.get(k))
        .and_then(Value::as_str)
        .unwrap_or("")
}
fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value> {
    serde_json::from_slice(&assets::fetch(url, headers, 4 << 20)?)
        .map_err(|e| format!("Unexpected response from {url}: {e}"))
}
fn unsplash_headers(key: &str) -> [(&'static str, String); 2] {
    [
        ("Authorization", format!("Client-ID {key}")),
        ("Accept-Version", "v1".into()),
    ]
}
fn with_utm(url: &str) -> String {
    format!("{url}{}{UTM}", if url.contains('?') { '&' } else { '?' })
}

/// One Unsplash photo as a search result.
pub fn unsplash_result(p: &Value) -> Value {
    let name = s(p, &["user", "name"]);
    let alt = [s(p, &["alt_description"]), s(p, &["description"])]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or("");
    json!({
        "stockId": format!("unsplash:{}", s(p, &["id"])),
        "provider": "unsplash",
        "src": s(p, &["urls", "regular"]),
        "thumb": s(p, &["urls", "thumb"]),
        "width": p["width"], "height": p["height"], "color": p["color"],
        "alt": alt,
        "photographer": name,
        "photographerUrl": with_utm(s(p, &["user", "links", "html"])),
        "sourceUrl": with_utm(s(p, &["links", "html"])),
        "license": "Unsplash License",
        "attribution": format!("Photo by {name} on Unsplash"),
    })
}
/// One Openverse image as a search result.
pub fn openverse_result(p: &Value) -> Value {
    let license = format!(
        "CC {} {}",
        s(p, &["license"]).to_uppercase(),
        s(p, &["license_version"])
    );
    let license = match s(p, &["license"]) {
        "cc0" => "CC0 1.0".into(),
        "pdm" => "Public Domain Mark".into(),
        _ => license.trim().to_string(),
    };
    json!({
        "stockId": format!("openverse:{}", s(p, &["id"])),
        "provider": "openverse",
        "src": s(p, &["url"]),
        "thumb": s(p, &["thumbnail"]),
        "width": p["width"], "height": p["height"],
        "alt": s(p, &["title"]),
        "photographer": s(p, &["creator"]),
        "photographerUrl": s(p, &["creator_url"]),
        "sourceUrl": s(p, &["foreign_landing_url"]),
        "license": license,
        "licenseUrl": s(p, &["license_url"]),
        "attribution": s(p, &["attribution"]),
    })
}

pub fn search(args: &Value, unsplash_key: Option<&str>) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or("query must be a non-empty string")?;
    let count = args
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 30);
    let orientation = args.get("orientation").and_then(Value::as_str);
    if let Some(o) = orientation
        && !matches!(o, "landscape" | "portrait" | "squarish")
    {
        return Err("orientation must be landscape, portrait or squarish".into());
    }
    let provider = args.get("provider").and_then(Value::as_str);
    let use_unsplash = match (provider, unsplash_key) {
        (Some("unsplash"), None) => {
            return Err("Unsplash search needs an access key: set UNSPLASH_ACCESS_KEY, or add unsplashAccessKey under settings in the library file .cardboard-library.json (read on each call, no restart needed). Without a key, omit provider to search Openverse's openly licensed images.".into());
        }
        (Some("unsplash"), Some(_)) | (None, Some(_)) => true,
        (Some("openverse") | None, _) => false,
        (Some(_), _) => return Err("provider must be unsplash or openverse".into()),
    };
    let q = query_escape(query);
    let (provider, results, note) = if use_unsplash {
        let key = unsplash_key.unwrap_or_default();
        let mut url = format!(
            "https://api.unsplash.com/search/photos?query={q}&per_page={count}&content_filter=high"
        );
        if let Some(o) = orientation {
            url.push_str(&format!("&orientation={o}"));
        }
        let [a, b] = unsplash_headers(key);
        let body = get_json(&url, &[(a.0, &a.1), (b.0, &b.1)])?;
        let results = body["results"].as_array().cloned().unwrap_or_default();
        (
            "unsplash",
            results.iter().map(unsplash_result).collect::<Vec<_>>(),
            "Use a result with import_image {stockId}. Unsplash images stay hotlinked as their guidelines require; keep the attribution (import_image adds it to alt and data-attribution) and credit the photographer where the design is published.",
        )
    } else {
        let mut url = format!(
            "https://api.openverse.org/v1/images/?q={q}&page_size={count}&license_type=commercial,modification&mature=false"
        );
        if let Some(o) = orientation {
            let ratio = match o {
                "landscape" => "wide",
                "portrait" => "tall",
                _ => "square",
            };
            url.push_str(&format!("&aspect_ratio={ratio}"));
        }
        let body = get_json(&url, &[])?;
        let results = body["results"].as_array().cloned().unwrap_or_default();
        (
            "openverse",
            results.iter().map(openverse_result).collect::<Vec<_>>(),
            "Openly licensed images from Openverse (no Unsplash key configured). Use a result with import_image {stockId}; most licenses require keeping the attribution, which import_image stores in data-attribution.",
        )
    };
    Ok(json!({"provider":provider,"query":query,"results":results,"note":note}))
}

/// A stock photo chosen for use: a hotlinked URL (Unsplash) or downloaded bytes (Openverse).
pub struct Chosen {
    pub src: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub result: Value,
}
pub fn choose(stock_id: &str, unsplash_key: Option<&str>) -> Result<Chosen> {
    let (provider, id) = stock_id
        .split_once(':')
        .filter(|(_, id)| {
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
        .ok_or(
            "stockId must look like unsplash:<id> or openverse:<id>, as returned by search_images",
        )?;
    match provider {
        "unsplash" => {
            let key = unsplash_key.ok_or("Using an Unsplash photo needs UNSPLASH_ACCESS_KEY (or the library setting unsplashAccessKey)")?;
            let [a, b] = unsplash_headers(key);
            let headers = [(a.0, a.1.as_str()), (b.0, b.1.as_str())];
            let photo = get_json(&format!("https://api.unsplash.com/photos/{id}"), &headers)?;
            // Required by the API guidelines whenever a photo is used.
            let download = s(&photo, &["links", "download_location"]);
            if !download.is_empty() {
                get_json(download, &headers)?;
            }
            let result = unsplash_result(&photo);
            Ok(Chosen {
                src: Some(result["src"].as_str().unwrap_or("").into()),
                bytes: None,
                result,
            })
        }
        "openverse" => {
            let image = get_json(&format!("https://api.openverse.org/v1/images/{id}/"), &[])?;
            let result = openverse_result(&image);
            let bytes = assets::fetch(s(&image, &["url"]), &[], assets::MAX_BYTES)?;
            Ok(Chosen {
                src: None,
                bytes: Some(bytes),
                result,
            })
        }
        _ => Err("stockId provider must be unsplash or openverse".into()),
    }
}
