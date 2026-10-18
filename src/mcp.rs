use crate::{hub::Hub, presence::Actor, tools};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

pub type SharedStore = Arc<Hub>;
/// Clients name their session here in `initialize` capabilities, so claims and undo authorship
/// survive the relay reconnecting.
pub const SESSION_CAPABILITY: &str = "cardboard/session";

#[derive(Clone)]
pub struct CardboardServer {
    pub store: SharedStore,
    /// Fallback session id for clients that don't name one.
    pub session: String,
}

impl CardboardServer {
    pub fn new(store: SharedStore) -> Self {
        Self {
            store,
            session: format!("s-{}", crate::store::random_hex(6)),
        }
    }
    fn actor(&self, context: &RequestContext<RoleServer>) -> Actor {
        let info = context.peer.peer_info();
        let id = info
            .as_ref()
            .and_then(|i| {
                i.capabilities
                    .experimental
                    .as_ref()?
                    .get(SESSION_CAPABILITY)
            })
            .and_then(|c| c.get("id")?.as_str().map(String::from))
            .unwrap_or_else(|| self.session.clone());
        let name = info.map_or("agent".into(), |i| i.client_info.name.clone());
        Actor { id, name }
    }
}

impl ServerHandler for CardboardServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("cardboard", env!("CARGO_PKG_VERSION")))
            .with_instructions(tools::INSTRUCTIONS)
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        let tools = tools::definitions()
            .into_iter()
            .map(serde_json::from_value)
            .collect::<std::result::Result<Vec<Tool>, _>>()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        // Protocol 2026-07-28 requires cache hints on tools/list. rmcp only
        // fills them in after an `initialize` handshake, and newer clients
        // (Claude Code 2.1.296+) negotiate per request instead, so set them here.
        Ok(ListToolsResult {
            tools,
            ttl_ms: Some(0),
            cache_scope: Some(CacheScope::Private),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::definitions()
            .into_iter()
            .find(|t| t["name"] == name)
            .and_then(|t| serde_json::from_value(t).ok())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let actor = self.actor(&context);
        // Off the async workers: rendering launches a browser and icon tools may wait on the network.
        let store = self.store.clone();
        let name = request.name.clone();
        let outcome =
            tokio::task::spawn_blocking(move || store.call_as(Some(&actor), &name, &args))
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let result = match outcome {
            Ok(mut value) => {
                // Images go out as real image content so the agent sees them, not as base64 text.
                let image = value.as_object_mut().and_then(|o| o.remove("image"));
                let mut content = vec![ContentBlock::text(value.to_string())];
                if let Some(image) = image {
                    content.push(ContentBlock::image(
                        image["data"].as_str().unwrap_or_default(),
                        image["mimeType"].as_str().unwrap_or("image/png"),
                    ));
                }
                let mut r = CallToolResult::success(content);
                r.structured_content = Some(value);
                r
            }
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        };
        Ok(result.into())
    }
}

/// Where the running editor listens for agents: a Unix socket, or a named pipe on Windows.
#[cfg(unix)]
pub fn default_socket() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::store::home_dir().join(".local/state/cardboard"));
    dir.join("cardboard.sock")
}
#[cfg(windows)]
pub fn default_socket() -> std::path::PathBuf {
    let user = std::env::var("USERNAME").unwrap_or_default();
    format!(r"\\.\pipe\cardboard-{user}").into()
}

#[cfg(unix)]
pub type Listener = std::os::unix::net::UnixListener;
/// Named pipes are created per connection inside the runtime, so the "listener" is the name.
#[cfg(windows)]
pub struct Listener(std::path::PathBuf);

/// Bind before opening the library, so a second editor cannot unlink a live socket or read files
/// another process is still writing. A headless agent session holding the socket hands it over.
pub fn bind_socket(path: &std::path::Path) -> std::io::Result<Listener> {
    for _ in 0..20 {
        match bind_exclusive(path) {
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                let reply = control(path, &json!({"cardboard":"handover"})).map_err(|_| e)?;
                if reply["ok"] != true {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        reply["error"]
                            .as_str()
                            .unwrap_or("Cardboard is already running")
                            .to_string(),
                    ));
                }
                // The headless host saved and stopped; its socket file is ours to replace.
                #[cfg(unix)]
                let _ = std::fs::remove_file(path);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            other => return other,
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        "Agent sessions keep re-hosting the library; close them and retry",
    ))
}

/// Bind only if nobody is serving, reclaiming a socket left behind by a crash.
#[cfg(unix)]
pub fn bind_exclusive(path: &std::path::Path) -> std::io::Result<Listener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = match std::os::unix::net::UnixListener::bind(path) {
        // A socket file nobody answers on is left over from a crash; reclaim it.
        Err(e)
            if e.kind() == std::io::ErrorKind::AddrInUse
                && std::os::unix::net::UnixStream::connect(path).is_err() =>
        {
            std::fs::remove_file(path)?;
            std::os::unix::net::UnixListener::bind(path)?
        }
        other => other?,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}
/// Named pipes refuse a second first instance; that check happens in `serve_socket`.
#[cfg(windows)]
pub fn bind_exclusive(path: &std::path::Path) -> std::io::Result<Listener> {
    // Opening the pipe as a client tells us whether someone is already serving this name.
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path);
    match probe {
        Ok(_) => Err(std::io::ErrorKind::AddrInUse.into()),
        Err(_) => Ok(Listener(path.to_path_buf())),
    }
}

/// Send one control line to whoever owns the socket and read its one-line JSON reply.
/// Control lines are not JSON-RPC (no `jsonrpc` field), so they never collide with MCP.
pub fn control(path: &std::path::Path, message: &Value) -> std::io::Result<Value> {
    use std::io::{BufRead, Write};
    #[cfg(unix)]
    let stream = {
        let s = std::os::unix::net::UnixStream::connect(path)?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        s
    };
    #[cfg(windows)]
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let mut writer = &stream;
    writer.write_all(format!("{message}\n").as_bytes())?;
    let mut line = String::new();
    std::io::BufReader::new(&stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the owner does not understand control messages (an older Cardboard?)",
        )
    })
}

/// What `{"cardboard":"status"}` reports about this process.
pub fn status(hub: &Hub) -> Value {
    let mut status = hub.host();
    status["pid"] = json!(std::process::id());
    status["version"] = json!(env!("CARGO_PKG_VERSION"));
    status["library"] = json!(hub.lock().library);
    status
}

/// Answer control lines, then hand the rest of the stream to an MCP session.
async fn spawn_session<S>(stream: S, store: SharedStore)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = tokio::io::BufReader::new(read);
    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }
    let message: Value = serde_json::from_str(&line).unwrap_or_default();
    if let Some(command) = message.get("cardboard").and_then(Value::as_str) {
        let reply = match command {
            "status" => status(&store),
            "handover" if store.host()["mode"] == "headless" => match store.retire() {
                Ok(()) => json!({"ok":true}),
                Err(e) => {
                    json!({"ok":false,"error":format!("Could not save before handing over: {e}")})
                }
            },
            "handover" => {
                json!({"ok":false,"error":"Cardboard is already running; close it first"})
            }
            _ => json!({"ok":false,"error":"Unknown control message"}),
        };
        let _ = write.write_all(format!("{reply}\n").as_bytes()).await;
        return;
    }
    let stream = tokio::io::join(std::io::Cursor::new(line).chain(reader), write);
    let server = CardboardServer::new(store.clone());
    let mut retired = store.retired();
    match server.serve(stream).await {
        Ok(service) => {
            let token = service.cancellation_token();
            tokio::select! {
                _ = service.waiting() => {}
                // Drop agents off a retired host so their relays reconnect to the new owner.
                _ = retired.wait_for(|r| *r) => token.cancel(),
            }
        }
        Err(e) => eprintln!("MCP connection ended: {e}"),
    }
}

/// Serve agents until the hub is retired (handed over to the app).
#[cfg(unix)]
pub async fn serve_socket(listener: Listener, store: SharedStore) -> std::io::Result<()> {
    let listener = tokio::net::UnixListener::from_std(listener)?;
    let mut retired = store.retired();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                tokio::spawn(spawn_session(stream, store.clone()));
            }
            _ = retired.wait_for(|r| *r) => return Ok(()),
        }
    }
}
#[cfg(windows)]
pub async fn serve_socket(listener: Listener, store: SharedStore) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    // first_pipe_instance fails if another editor already owns the name.
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&listener.0)?;
    let mut retired = store.retired();
    loop {
        tokio::select! {
            connected = server.connect() => connected?,
            _ = retired.wait_for(|r| *r) => return Ok(()),
        }
        let connected = server;
        server = ServerOptions::new().create(&listener.0)?;
        tokio::spawn(spawn_session(connected, store.clone()));
    }
}
