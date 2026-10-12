//! The core session: Store + Hub, the editor's HTTP/WebSocket server and the agents' MCP socket.
use cardboard::{hub::Hub, server, store::Store};
use std::{path::PathBuf, sync::Arc};

pub struct Backend {
    pub url: String,
    hub: Arc<Hub>,
    socket: PathBuf,
}

impl Backend {
    pub fn start(runtime: &tokio::runtime::Runtime, library: Option<PathBuf>) -> Result<Self, String> {
        let library = library.unwrap_or_else(Store::default_library);
        let store = Store::library(&library, Some(&Store::legacy_workspace()))?;
        let hub = Hub::new(store);
        // Bind before opening the window, so a second editor cannot unlink a live socket.
        let socket = cardboard::mcp::default_socket();
        // bind_socket reclaims a socket file left behind by a crashed editor.
        let listener = cardboard::mcp::bind_socket(&socket)
            .map_err(|e| format!("Cannot bind {}: {e}. Is Cardboard already running?", socket.display()))?;
        let url = runtime.block_on(async {
            hub.spawn_autosave();
            let shared = hub.clone();
            tokio::spawn(async move {
                if let Err(e) = cardboard::mcp::serve_socket(listener, shared).await {
                    eprintln!("MCP server ended: {e}");
                }
            });
            server::run_server(hub.clone(), 0).await.map(|s| s.url)
        });
        let url = url.map_err(|e| format!("Cannot start the editor server: {e}"))?;
        Ok(Self { url, hub, socket })
    }
    /// Flush pending saves and remove the MCP socket.
    pub fn shutdown(self, runtime: tokio::runtime::Runtime) {
        if let Err(e) = self.hub.flush() {
            eprintln!("{e}");
        }
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
        #[cfg(windows)]
        let _ = &self.socket;
        runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    }
}
