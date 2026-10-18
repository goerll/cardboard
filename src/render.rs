//! Renders the in-memory document in a headless Chromium so agents can see and measure it.
//!
//! The editor itself draws designs with Chromium (CEF in the desktop app, any Chromium browser
//! with `cardboard serve`), so a headless Chromium gives the same pixels and layout the person
//! sees, works offline, and needs no editor window. The browser is driven over the DevTools
//! protocol: one launch per call, a throwaway profile, and the page is the artboard alone.
use crate::{
    document::{Document, Node, Result},
    html,
    store::{Store, random_hex},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub const TOOLS: &[&str] = &["screenshot", "get_bounds"];
/// Output limits keep image payloads well under what MCP clients and vision models accept.
const MAX_EDGE: f64 = 4096.0;
const MAX_PIXELS: f64 = 4_000_000.0;
const MAX_PNG_BYTES: usize = 3_500_000;
const MAX_BOXES: usize = 500;
const TIMEOUT: Duration = Duration::from_secs(30);

// Mirrors the editor's iframe CSS (ui/src/editor/engine.ts FRAME_CSS) so layout matches the canvas.
const FRAME_CSS: &str = "*,*::before,*::after{box-sizing:border-box}html,body{margin:0;padding:0;background:transparent}body{font-family:system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif;-webkit-font-smoothing:antialiased}.cb-ab{position:absolute;left:0;top:0}img{display:block}";
// Same policy as export_html: no scripts or network beyond images and fonts.
const CSP: &str = "default-src 'none'; img-src https: http: data: file:; style-src 'unsafe-inline'; font-src https: data: file:; script-src 'none'; base-uri 'none'; form-action 'none'";

/// Waits for fonts and images, then reports page-space boxes for every design node.
const MEASURE: &str = r#"(async ({styleIds, props}) => {
  const settle = Promise.all([document.fonts.ready, ...[...document.images].map(i => i.decode().catch(() => {}))]);
  await Promise.race([settle, new Promise(r => setTimeout(r, 5000))]);
  const want = new Set(styleIds), out = {};
  for (const el of document.querySelectorAll('[data-cardboard-id]')) {
    const r = el.getBoundingClientRect(), id = el.getAttribute('data-cardboard-id');
    const b = {x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height,
      scrollWidth: el.scrollWidth, clientWidth: el.clientWidth, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight,
      rendered: el.getClientRects().length > 0};
    if (want.has(id)) { const cs = getComputedStyle(el); b.styles = Object.fromEntries(props.map(p => [p, cs.getPropertyValue(p)])); }
    out[id] = b;
  }
  return out;
})"#;

pub struct Job {
    meta: Value,
    html: String,
    viewport: (f64, f64),
    artboard: String,
    kind: Kind,
}
enum Kind {
    Screenshot {
        node: String,
        scale: Option<f64>,
        path: Option<PathBuf>,
    },
    Bounds {
        /// (id, name, tag, parent id) in tree order.
        nodes: Vec<(String, String, String, Option<String>)>,
        truncated: bool,
        props: Vec<String>,
    },
}

/// Snapshot what to render while the store is locked; `Job::run` does the slow part without it.
pub fn prepare(store: &Store, name: &str, args: &Value) -> Result<Job> {
    let file = store.file_id(args)?;
    let doc = store.file(&file)?;
    let meta = json!({"fileId":file,"revision":doc.revision});
    match name {
        "screenshot" => {
            let scale = match args.get("scale") {
                None => None,
                Some(v) => Some(
                    v.as_f64()
                        .filter(|s| (0.1..=4.0).contains(s))
                        .ok_or("scale must be a number from 0.1 to 4")?,
                ),
            };
            let path = match args.get("path") {
                None => None,
                Some(v) => {
                    let path = PathBuf::from(v.as_str().ok_or("path must be a string")?);
                    if !path.is_absolute() {
                        return Err("path must be absolute".into());
                    }
                    Some(path)
                }
            };
            let node = crate::tools::string(args, "nodeId")?.to_string();
            let artboard = artboard_of(doc, &node)?;
            Ok(job(
                doc,
                artboard,
                meta,
                Kind::Screenshot { node, scale, path },
            ))
        }
        "get_bounds" => {
            let ids = args
                .get("nodeIds")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
                .ok_or("nodeIds must be a non-empty array")?;
            let depth = args
                .get("depth")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .min(10) as usize;
            let props = match args.get("styles") {
                None => vec![],
                Some(v) => v
                    .as_array()
                    .ok_or("styles must be an array of CSS property names")?
                    .iter()
                    .map(|p| {
                        p.as_str()
                            .map(html::css_name)
                            .ok_or("styles must contain strings")
                    })
                    .collect::<std::result::Result<_, _>>()?,
            };
            let mut artboard: Option<&Node> = None;
            let mut nodes = vec![];
            for id in ids {
                let id = id.as_str().ok_or("nodeIds must contain strings")?;
                let owner = artboard_of(doc, id)?;
                if artboard.is_some_and(|a| a.id != owner.id) {
                    return Err(
                        "nodeIds must belong to one artboard; call get_bounds once per artboard"
                            .into(),
                    );
                }
                artboard = Some(owner);
                collect(
                    owner.find(id).unwrap(),
                    parent_of(owner, id),
                    depth,
                    &mut nodes,
                );
            }
            let mut seen = std::collections::HashSet::new();
            nodes.retain(|n| seen.insert(n.0.clone()));
            let truncated = nodes.len() > MAX_BOXES;
            nodes.truncate(MAX_BOXES);
            let kind = Kind::Bounds {
                nodes,
                truncated,
                props,
            };
            Ok(job(doc, artboard.unwrap(), meta, kind))
        }
        _ => Err(format!("{name} is not a render tool")),
    }
}

fn job(doc: &Document, artboard: &Node, meta: Value, kind: Kind) -> Job {
    let tokens = doc
        .tokens
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";");
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\"><style>{FRAME_CSS}:root{{{tokens}}}</style></head><body><div class=\"cb-ab\">{}</div></body></html>",
        html::node_html(artboard, true)
    );
    // The editor's viewport is the canvas, so vw/vh/% on an artboard have no fixed meaning;
    // use the artboard's own px size, or a desktop default.
    let px = |key: &str, default: f64| {
        artboard
            .styles
            .get(key)
            .and_then(|v| v.strip_suffix("px"))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| *v >= 1.0 && *v <= 16384.0)
            .unwrap_or(default)
    };
    Job {
        meta,
        html,
        viewport: (px("width", 1440.0), px("height", 900.0)),
        artboard: artboard.id.clone(),
        kind,
    }
}

fn artboard_of<'a>(doc: &'a Document, id: &str) -> Result<&'a Node> {
    doc.node(id)?;
    Ok(doc
        .pages
        .iter()
        .flat_map(|p| &p.artboards)
        .find(|a| a.contains(id))
        .unwrap())
}
fn parent_of(root: &Node, id: &str) -> Option<String> {
    root.children.iter().find_map(|c| {
        if c.id == id {
            Some(root.id.clone())
        } else {
            parent_of(c, id)
        }
    })
}
fn collect(
    node: &Node,
    parent: Option<String>,
    depth: usize,
    out: &mut Vec<(String, String, String, Option<String>)>,
) {
    out.push((node.id.clone(), node.name.clone(), node.tag.clone(), parent));
    if depth > 0 && out.len() <= MAX_BOXES {
        for child in &node.children {
            collect(child, Some(node.id.clone()), depth - 1, out);
        }
    }
}

impl Job {
    pub fn run(self) -> Result<Value> {
        let mut browser = Browser::launch()?;
        let style_ids: Vec<&str> = match &self.kind {
            Kind::Bounds { nodes, props, .. } if !props.is_empty() => {
                nodes.iter().map(|n| n.0.as_str()).collect()
            }
            _ => vec![],
        };
        let props = match &self.kind {
            Kind::Bounds { props, .. } => props.clone(),
            _ => vec![],
        };
        let boxes = browser.load(
            &self.html,
            self.viewport,
            json!({"styleIds":style_ids,"props":props}),
        )?;
        let get = |id: &str| -> Result<&Value> {
            boxes
                .get(id)
                .ok_or_else(|| format!("Node {id} was not rendered"))
        };
        let origin = get(&self.artboard)?;
        let (ox, oy) = (num(origin, "x"), num(origin, "y"));
        let mut result = self.meta;
        match self.kind {
            Kind::Screenshot { node, scale, path } => {
                let b = get(&node)?;
                let (x, y, w, h) = (num(b, "x"), num(b, "y"), num(b, "width"), num(b, "height"));
                if !b["rendered"].as_bool().unwrap_or(false) || w < 1.0 || h < 1.0 {
                    return Err(format!(
                        "Node {node} has no visible size ({w:.0}x{h:.0}); it may be hidden or empty"
                    ));
                }
                let small = node != self.artboard && w.max(h) <= 800.0;
                let requested = scale.unwrap_or(if small { 2.0 } else { 1.0 });
                let scale = requested
                    .min(MAX_EDGE / w.max(h))
                    .min((MAX_PIXELS / (w * h)).sqrt());
                let png = browser.capture((x, y, w, h), self.viewport, scale, "png")?;
                let bytes = BASE64.decode(&png).map_err(|e| e.to_string())?;
                if let Some(path) = &path {
                    std::fs::write(path, &bytes)
                        .map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
                    result["path"] = json!(path);
                }
                // Photos make PNGs huge; JPEG keeps the inline image small enough for any client.
                let (data, mime) = if bytes.len() > MAX_PNG_BYTES {
                    (
                        browser.capture((x, y, w, h), self.viewport, scale, "jpeg")?,
                        "image/jpeg",
                    )
                } else {
                    (png, "image/png")
                };
                result["nodeId"] = json!(node);
                result["artboardId"] = json!(self.artboard);
                result["bounds"] = rect(x - ox, y - oy, w, h);
                result["scale"] = json!(round(scale * 1000.0) / 1000.0);
                result["imageWidth"] = json!((w * scale).round());
                result["imageHeight"] = json!((h * scale).round());
                if scale < requested {
                    result["note"] = json!(
                        "Scale was reduced to keep the image size reasonable; screenshot a child node for detail."
                    );
                }
                result["image"] = json!({"data":data,"mimeType":mime});
            }
            Kind::Bounds {
                nodes, truncated, ..
            } => {
                let mut list = vec![];
                for (id, name, tag, parent) in nodes {
                    let b = get(&id)?;
                    let (x, y, w, h) =
                        (num(b, "x"), num(b, "y"), num(b, "width"), num(b, "height"));
                    let mut entry = rect(x - ox, y - oy, w, h);
                    entry["id"] = json!(id);
                    entry["name"] = json!(name);
                    entry["tag"] = json!(tag);
                    if !b["rendered"].as_bool().unwrap_or(false) {
                        entry["hidden"] = json!(true);
                    }
                    // Content wider/taller than the box: text or children spilling or clipped.
                    for (axis, scroll, client) in [
                        ("overflowX", "scrollWidth", "clientWidth"),
                        ("overflowY", "scrollHeight", "clientHeight"),
                    ] {
                        let extra = num(b, scroll) - num(b, client);
                        if num(b, client) > 0.0 && extra >= 1.0 {
                            entry[axis] = json!(extra);
                        }
                    }
                    if let Some(p) = parent.as_deref().and_then(|p| boxes.get(p))
                        && w > 0.0
                        && (x < num(p, "x") - 0.5
                            || y < num(p, "y") - 0.5
                            || x + w > num(p, "x") + num(p, "width") + 0.5
                            || y + h > num(p, "y") + num(p, "height") + 0.5)
                    {
                        entry["outsideParent"] = json!(true);
                    }
                    if let Some(styles) = b.get("styles") {
                        entry["styles"] = styles.clone();
                    }
                    entry["parentId"] = json!(parent);
                    list.push(entry);
                }
                result["artboardId"] = json!(self.artboard);
                result["nodes"] = json!(list);
                if truncated {
                    result["truncated"] = json!(true);
                }
            }
        }
        Ok(result)
    }
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}
fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({"x":round(x),"y":round(y),"width":round(w),"height":round(h)})
}

/// A Chromium-based browser: `CARDBOARD_BROWSER`, then common install locations and PATH names.
pub fn find_browser() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CARDBOARD_BROWSER") {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!(
                "CARDBOARD_BROWSER is set to {}, which is not a file",
                path.display()
            ))
        };
    }
    let mut candidates: Vec<PathBuf> = vec![];
    if cfg!(target_os = "macos") {
        let home = crate::store::home_dir();
        for app in [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Chromium.app/Contents/MacOS/Chromium",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
        ] {
            candidates.push(Path::new("/Applications").join(app));
            candidates.push(home.join("Applications").join(app));
        }
    }
    if cfg!(windows) {
        for root in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
        {
            for exe in [
                r"Google\Chrome\Application\chrome.exe",
                r"Microsoft\Edge\Application\msedge.exe",
                r"Chromium\Application\chrome.exe",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
            ] {
                candidates.push(Path::new(&root).join(exe));
            }
        }
    }
    let names = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "chrome",
        "microsoft-edge",
        "microsoft-edge-stable",
        "msedge",
        "brave-browser",
    ];
    for dir in std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        for name in names {
            candidates.push(dir.join(name));
            if cfg!(windows) {
                candidates.push(dir.join(format!("{name}.exe")));
            }
        }
    }
    candidates.push("/snap/bin/chromium".into());
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "Rendering needs a Chromium-based browser and none was found. Install Chromium, Google Chrome or Microsoft Edge, or set CARDBOARD_BROWSER to its executable.".into()
    })
}

struct Browser {
    child: Child,
    dir: PathBuf,
    ws: tungstenite::WebSocket<TcpStream>,
    next: u64,
    session: Option<String>,
    events: Vec<String>,
}

impl Drop for Browser {
    fn drop(&mut self) {
        // A graceful close lets helper processes exit before the profile is removed;
        // killing only the main process leaves them writing into it.
        self.session = None;
        self.next += 1;
        let close = json!({"id":self.next,"method":"Browser.close"}).to_string();
        let _ = self.ws.send(tungstenite::Message::text(close));
        for _ in 0..50 {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Browser {
    fn launch() -> Result<Self> {
        let exe = find_browser()?;
        let dir = std::env::temp_dir().join(format!("cardboard-render-{}", random_hex(8)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut child = Command::new(&exe)
            .args([
                "--headless=new",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-networking",
                "--disable-sync",
                "--hide-scrollbars",
                "--mute-audio",
            ])
            .arg(format!("--user-data-dir={}", dir.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot start {}: {e}", exe.display()))?;
        let (tx, rx) = mpsc::channel();
        let stderr = child.stderr.take().unwrap();
        // Keep draining stderr after the handshake line so the browser never blocks on it.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok)
            {
                let _ = tx.send(line);
            }
        });
        let mut log = vec![];
        let url = loop {
            match rx.recv_timeout(TIMEOUT) {
                Ok(line) => match line.split_once("DevTools listening on ") {
                    Some((_, url)) => break url.trim().to_string(),
                    None => log.push(line),
                },
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_dir_all(&dir);
                    let tail = log[log.len().saturating_sub(5)..].join("\n");
                    return Err(format!(
                        "{} did not start a headless session. {tail}",
                        exe.display()
                    ));
                }
            }
        };
        let host = url
            .strip_prefix("ws://")
            .and_then(|r| r.split('/').next())
            .ok_or_else(|| format!("Unexpected DevTools address {url}"))?
            .to_string();
        let connect = || -> Result<tungstenite::WebSocket<TcpStream>> {
            let stream = TcpStream::connect(&host).map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(TIMEOUT))
                .map_err(|e| e.to_string())?;
            Ok(tungstenite::client(url.as_str(), stream)
                .map_err(|e| format!("DevTools handshake failed: {e}"))?
                .0)
        };
        let ws = match connect() {
            Ok(ws) => ws,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        // From here on, Drop kills the browser and removes the profile on any error.
        let mut browser = Browser {
            child,
            dir,
            ws,
            next: 0,
            session: None,
            events: vec![],
        };
        let target = browser.call("Target.createTarget", json!({"url":"about:blank"}))?;
        let session = browser.call(
            "Target.attachToTarget",
            json!({"targetId":target["targetId"],"flatten":true}),
        )?;
        browser.session = session["sessionId"].as_str().map(String::from);
        Ok(browser)
    }

    fn read(&mut self) -> Result<Value> {
        loop {
            let message = self.ws.read().map_err(|e| match e {
                tungstenite::Error::Io(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    "Rendering timed out".to_string()
                }
                e => format!("Lost the headless browser: {e}"),
            })?;
            if let Ok(text) = message.to_text()
                && !text.is_empty()
            {
                return serde_json::from_str(text).map_err(|e| e.to_string());
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let mut message = json!({"id":self.next,"method":method,"params":params});
        if let Some(session) = &self.session {
            message["sessionId"] = json!(session);
        }
        self.ws
            .send(tungstenite::Message::text(message.to_string()))
            .map_err(|e| format!("Lost the headless browser: {e}"))?;
        loop {
            let mut reply = self.read()?;
            if reply["id"] == json!(self.next) {
                if let Some(error) = reply.get("error") {
                    return Err(format!("{method} failed: {}", error["message"]));
                }
                return Ok(reply["result"].take());
            }
            if let Some(event) = reply["method"].as_str() {
                self.events.push(event.to_string());
            }
        }
    }

    fn wait_for(&mut self, event: &str) -> Result<()> {
        while !self.events.iter().any(|e| e == event) {
            let reply = self.read()?;
            if let Some(name) = reply["method"].as_str() {
                self.events.push(name.to_string());
            }
        }
        Ok(())
    }

    fn viewport(&mut self, (width, height): (f64, f64), scale: f64) -> Result<()> {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width.round(),"height":height.round(),"deviceScaleFactor":scale,"mobile":false}),
        )?;
        Ok(())
    }

    /// Load the page from a file (no data: URL size limit) and return the measured boxes.
    fn load(
        &mut self,
        html: &str,
        viewport: (f64, f64),
        args: Value,
    ) -> Result<HashMap<String, Value>> {
        let file = self.dir.join("page.html");
        std::fs::write(&file, html).map_err(|e| e.to_string())?;
        self.call("Page.enable", json!({}))?;
        self.viewport(viewport, 1.0)?;
        let nav = self.call("Page.navigate", json!({"url":file_url(&file)}))?;
        if let Some(error) = nav["errorText"].as_str() {
            return Err(format!("Rendering failed to load the page: {error}"));
        }
        self.wait_for("Page.loadEventFired")?;
        let mut value = self.call(
            "Runtime.evaluate",
            json!({"expression":format!("{MEASURE}({args})"),"awaitPromise":true,"returnByValue":true}),
        )?;
        if let Some(e) = value.get("exceptionDetails") {
            return Err(format!("Measuring the page failed: {e}"));
        }
        serde_json::from_value(value["result"]["value"].take()).map_err(|e| e.to_string())
    }

    /// Base64 image data for a page-space rectangle, rendered at `scale` device pixels per px.
    fn capture(
        &mut self,
        (x, y, width, height): (f64, f64, f64, f64),
        viewport: (f64, f64),
        scale: f64,
        format: &str,
    ) -> Result<String> {
        self.viewport(viewport, scale)?;
        let mut params = json!({"format":format,"captureBeyondViewport":true,"clip":{"x":x,"y":y,"width":width,"height":height,"scale":1}});
        if format == "jpeg" {
            params["quality"] = json!(85);
        }
        let mut shot = self.call("Page.captureScreenshot", params)?;
        shot["data"]
            .take()
            .as_str()
            .map(String::from)
            .ok_or_else(|| "The browser returned no image".into())
    }
}

fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let encoded: String = path
        .chars()
        .map(|c| match c {
            ' ' => "%20".into(),
            '%' => "%25".into(),
            '#' => "%23".into(),
            '?' => "%3F".into(),
            c => c.to_string(),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}
