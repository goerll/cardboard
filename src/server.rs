//! Local HTTP + WebSocket server for the editor UI (see docs/architecture.md).
//!
//! ```ignore
//! let hub = cardboard::hub::Hub::new(Store::library(&dir, Some(&Store::legacy_workspace()))?);
//! hub.spawn_autosave();
//! let server = cardboard::server::run_server(hub.clone(), 0).await?; // 0 = random port
//! open_window(&server.url); // http://127.0.0.1:<port>/?t=<token>
//! // on exit: hub.flush()
//! ```
//!
//! Binds 127.0.0.1 only. `/` needs the token (query `t` or the `cardboard_token` cookie it sets);
//! static assets do not. `/ws` needs the token and, when present, a local Origin.
use crate::hub::Hub;
use axum::{
    Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use rust_embed::RustEmbed;
use serde_json::{Value, json};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
#[allow_missing = true]
struct Assets;

pub struct Server {
    pub addr: SocketAddr,
    pub token: String,
    /// The URL to open, including the token.
    pub url: String,
    pub task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
struct App {
    hub: Arc<Hub>,
    token: Arc<String>,
    port: u16,
}

pub async fn run_server(hub: Arc<Hub>, port: u16) -> std::io::Result<Server> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let token = crate::store::random_hex(16);
    let app = App {
        hub,
        token: Arc::new(token.clone()),
        port: addr.port(),
    };
    let router = Router::new()
        .route("/ws", get(socket))
        .route("/asset/{name}", get(asset))
        .fallback(get(page))
        .with_state(app);
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("Cardboard server stopped: {e}");
        }
    });
    Ok(Server {
        addr,
        url: format!("http://127.0.0.1:{}/?t={token}", addr.port()),
        token,
        task,
    })
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix("cardboard_token=").map(String::from))
}
fn equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}
fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn page(
    State(app): State<App>,
    uri: Uri,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Static files carry no data; only the app page is gated.
    if !path.is_empty()
        && path != "index.html"
        && let Some(file) = Assets::get(path)
    {
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, content_type(path)),
                (header::CACHE_CONTROL, cache),
            ],
            file.data,
        )
            .into_response();
    }
    if path.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let by_query = q.get("t").is_some_and(|t| equal(t, &app.token));
    if !by_query && !cookie_token(&headers).is_some_and(|t| equal(&t, &app.token)) {
        return (
            StatusCode::FORBIDDEN,
            "Open Cardboard from the desktop app or the URL printed by `cardboard serve`.",
        )
            .into_response();
    }
    let Some(index) = Assets::get("index.html") else {
        return (
            StatusCode::NOT_FOUND,
            "The UI is not built. Run `pnpm build` in ui/.",
        )
            .into_response();
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        index.data,
    )
        .into_response();
    if by_query
        && let Ok(cookie) = format!(
            "cardboard_token={}; Path=/; HttpOnly; SameSite=Strict",
            app.token
        )
        .parse()
    {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// Stored images and fonts. Not token-gated: names are SHA-256 content hashes, so a URL can only
/// be formed by someone who already has the bytes, and the dashboard's opaque-origin thumbnail
/// iframes cannot send the session cookie. The CSP sandbox keeps an SVG opened directly from
/// running script on this origin.
async fn asset(State(app): State<App>, Path(name): Path<String>) -> Response {
    let dirs = app.hub.lock().asset_dirs();
    for dir in dirs {
        let Some(path) = crate::assets::path(&dir, &name) else {
            break;
        };
        if let Ok(Ok(bytes)) = tokio::task::spawn_blocking(move || std::fs::read(path)).await {
            let ext = name.rsplit('.').next().unwrap_or("");
            return (
                [
                    (header::CONTENT_TYPE, crate::assets::mime(ext)),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                    (
                        header::CONTENT_SECURITY_POLICY,
                        "default-src 'none'; style-src 'unsafe-inline'; sandbox",
                    ),
                ],
                bytes,
            )
                .into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn socket(
    State(app): State<App>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let authorized = q
        .get("t")
        .or(cookie_token(&headers).as_ref())
        .is_some_and(|t| equal(t, &app.token));
    let origin_ok = headers.get(header::ORIGIN).is_none_or(|o| {
        o.to_str().is_ok_and(|o| {
            o == format!("http://127.0.0.1:{}", app.port)
                || o == format!("http://localhost:{}", app.port)
        })
    });
    if !authorized || !origin_ok {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(64 << 20)
        .on_upgrade(move |socket| session(socket, app.hub))
}

/// What one connection has already been sent.
#[derive(Default)]
struct Pushed {
    files: HashMap<String, u64>,
    workspace: String,
    library: u64,
}

/// Messages that bring a client up to date, documents first so tabs never point at unknown files.
fn sync(hub: &Hub, pushed: &mut Pushed) -> Vec<String> {
    let store = hub.lock();
    let mut out = vec![];
    for (id, file) in &store.files {
        if pushed.files.get(id) != Some(&file.seq) {
            pushed.files.insert(id.clone(), file.seq);
            out.push(
                json!({"type":"document","fileId":id,"doc":&file.doc,"canUndo":file.can_undo(),"canRedo":file.can_redo(),"agents":file.claims.to_json()})
                    .to_string(),
            );
        }
    }
    let workspace = store.workspace_info().to_string();
    if workspace != pushed.workspace {
        let mut message: Value = serde_json::from_str(&workspace).unwrap_or_default();
        message["type"] = json!("workspace");
        out.push(message.to_string());
        pushed.workspace = workspace;
    }
    pushed.files.retain(|id, _| {
        let open = store.files.contains_key(id);
        if !open {
            out.push(json!({"type":"closed","fileId":id}).to_string());
        }
        open
    });
    if pushed.library != store.library_seq {
        pushed.library = store.library_seq;
        out.push(json!({"type":"library"}).to_string());
    }
    out
}

async fn send_all(socket: &mut WebSocket, messages: Vec<String>) -> bool {
    for m in messages {
        if socket.send(Message::Text(m.into())).await.is_err() {
            return false;
        }
    }
    true
}

async fn session(mut socket: WebSocket, hub: Arc<Hub>) {
    let mut changed = hub.subscribe();
    let mut pushed = Pushed {
        library: hub.lock().library_seq,
        ..Default::default()
    };
    changed.borrow_and_update();
    if !send_all(&mut socket, sync(&hub, &mut pushed)).await {
        return;
    }
    loop {
        tokio::select! {
            message = socket.recv() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                    Some(Ok(_)) => continue,
                };
                let request: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                let reply = match request.get("tool").and_then(Value::as_str) {
                    Some(tool) => {
                        let (hub, tool, args) = (hub.clone(), tool.to_owned(), request.get("args").cloned().unwrap_or(json!({})));
                        match tokio::task::spawn_blocking(move || hub.call(&tool, &args)).await {
                            Ok(Ok(result)) => json!({"type":"result","id":id,"ok":true,"result":result}),
                            Ok(Err(error)) => json!({"type":"result","id":id,"ok":false,"error":error}),
                            Err(_) => json!({"type":"result","id":id,"ok":false,"error":"Command failed unexpectedly"}),
                        }
                    }
                    None => json!({"type":"result","id":id,"ok":false,"error":"Expected {id, tool, args}"}),
                };
                // Pushes caused by this request go out before its result.
                changed.borrow_and_update();
                let mut messages = sync(&hub, &mut pushed);
                messages.push(reply.to_string());
                if !send_all(&mut socket, messages).await {
                    return;
                }
            }
            result = changed.changed() => {
                if result.is_err() {
                    return;
                }
                changed.borrow_and_update();
                if !send_all(&mut socket, sync(&hub, &mut pushed)).await {
                    return;
                }
            }
        }
    }
}
