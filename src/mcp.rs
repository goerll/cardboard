use crate::{hub::Hub, tools};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::Value;
use std::sync::Arc;

pub type SharedStore = Arc<Hub>;
#[derive(Clone)]
pub struct CardboardServer {
    pub store: SharedStore,
}

impl ServerHandler for CardboardServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
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
        Ok(ListToolsResult {
            tools,
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
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = match self.store.call(&request.name, &args) {
            Ok(value) => {
                let mut r = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
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

/// Bind before starting the UI, so a second editor cannot accidentally unlink a live socket.
#[cfg(unix)]
pub fn bind_socket(path: &std::path::Path) -> std::io::Result<Listener> {
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
#[cfg(windows)]
pub fn bind_socket(path: &std::path::Path) -> std::io::Result<Listener> {
    Ok(Listener(path.to_path_buf()))
}

fn spawn_session<S>(stream: S, store: SharedStore)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let server = CardboardServer { store };
    tokio::spawn(async move {
        match server.serve(stream).await {
            Ok(service) => {
                let _ = service.waiting().await;
            }
            Err(e) => eprintln!("MCP connection ended: {e}"),
        }
    });
}

#[cfg(unix)]
pub async fn serve_socket(listener: Listener, store: SharedStore) -> std::io::Result<()> {
    let listener = tokio::net::UnixListener::from_std(listener)?;
    loop {
        let (stream, _) = listener.accept().await?;
        spawn_session(stream, store.clone());
    }
}
#[cfg(windows)]
pub async fn serve_socket(listener: Listener, store: SharedStore) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    // first_pipe_instance fails if another editor already owns the name.
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&listener.0)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = ServerOptions::new().create(&listener.0)?;
        spawn_session(connected, store.clone());
    }
}
