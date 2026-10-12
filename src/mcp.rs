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

#[cfg(unix)]
pub fn default_socket() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default()
                .join(".local/state/cardboard")
        });
    dir.join("cardboard.sock")
}

/// Bind before starting the UI, so a second editor cannot accidentally unlink a live socket.
#[cfg(unix)]
pub fn bind_socket(path: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixListener> {
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

#[cfg(unix)]
pub async fn serve_socket(
    listener: std::os::unix::net::UnixListener,
    store: SharedStore,
) -> std::io::Result<()> {
    let listener = tokio::net::UnixListener::from_std(listener)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let server = CardboardServer {
            store: store.clone(),
        };
        tokio::spawn(async move {
            match server.serve(stream).await {
                Ok(service) => {
                    let _ = service.waiting().await;
                }
                Err(e) => eprintln!("MCP connection ended: {e}"),
            }
        });
    }
}
