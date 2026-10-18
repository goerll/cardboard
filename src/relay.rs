//! `cardboard mcp`: connects an agent's stdio to whoever owns the library, and keeps the agent's
//! session alive while the app starts, restarts or quits.
//!
//! Exactly one process owns a library at a time: the one bound to the MCP socket. When nothing is
//! listening, the relay hosts the library itself (headless) and serves the socket, so other agents
//! share it. When the app starts it asks the headless host to hand over (see `mcp::bind_socket`);
//! the host saves, stops, and every relay reconnects to the app, replaying the MCP handshake so
//! the agent never notices. Requests in flight during a switch fail with a retryable error.
use crate::{hub::Hub, mcp, server, store::Store};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    future::Future,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, Lines};

pub struct Options {
    pub socket: PathBuf,
    /// The library to host when no app is running.
    pub library: PathBuf,
    /// Host the library when nothing is listening, instead of failing.
    pub fallback: bool,
}

const HINT: &str = "No Cardboard app was running, so this agent session hosts the library itself and saves edits there. To watch live, open editorUrl in a browser or start the Cardboard app; the app takes over without interrupting the agent.";
const LOST: &str = "Cardboard switched hosts (the app started, restarted or quit) while this request was running. Read the file again to see whether it applied, then retry.";
const STEP: Duration = Duration::from_millis(200);

type Reader = Lines<BufReader<Box<dyn AsyncRead + Send + Unpin>>>;
type Writer = Box<dyn AsyncWrite + Send + Unpin>;

async fn dial(path: &Path) -> io::Result<(Reader, Writer)> {
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(path).await?;
    #[cfg(windows)]
    let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(path)?;
    let (read, write) = tokio::io::split(stream);
    let read: Box<dyn AsyncRead + Send + Unpin> = Box::new(read);
    Ok((BufReader::new(read).lines(), Box::new(write)))
}

/// This process owning the library because no app was running.
pub struct Host {
    hub: Arc<Hub>,
    socket: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Host {
    /// Bind first, then read the library: whoever holds the socket is the only writer.
    pub async fn start(socket: &Path, library: &Path) -> io::Result<Self> {
        let listener = mcp::bind_exclusive(socket)?;
        let store =
            Store::library(library, Some(&Store::legacy_workspace())).map_err(io::Error::other)?;
        let hub = Hub::new(store);
        hub.spawn_autosave();
        let web = server::run_server(hub.clone(), 0).await?;
        hub.set_host(json!({"mode":"headless","editorUrl":web.url,"hint":HINT}));
        let shared = hub.clone();
        tokio::spawn(async move {
            if let Err(e) = mcp::serve_socket(listener, shared).await {
                eprintln!("MCP server ended: {e}");
            }
        });
        eprintln!("Cardboard: hosting {} headless. {HINT}", library.display());
        eprintln!("Cardboard: editor at {}", web.url);
        Ok(Self {
            hub,
            socket: socket.into(),
            server: web.task,
        })
    }
    pub fn hub(&self) -> &Arc<Hub> {
        &self.hub
    }
    /// Save and release the socket, unless the app already took over.
    pub fn shutdown(self) {
        self.server.abort();
        if self.hub.is_retired() {
            return;
        }
        if let Err(e) = self.hub.retire() {
            eprintln!("{e}");
        }
        // Unix sockets leave a file behind; on Windows the pipe disappears by itself.
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket);
        #[cfg(windows)]
        let _ = &self.socket;
    }
}

/// What the relay remembers about the agent's MCP session to replay it on a new connection.
#[derive(Default)]
pub struct Session {
    id: String,
    initialize: Option<Value>,
    initialized: bool,
    /// Requests sent but not answered: JSON id → method.
    pending: HashMap<String, String>,
    /// The replayed initialize whose reply the agent must not see.
    replay: Option<String>,
    replays: u64,
}

impl Session {
    pub fn new(id: String) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }
    /// Track an agent → server line. Returns the line to send and its request id, if any.
    pub fn outgoing(&mut self, raw: &str) -> (String, Option<String>) {
        let Ok(mut message) = serde_json::from_str::<Value>(raw) else {
            return (raw.into(), None);
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(String::from);
        match method.as_deref() {
            Some("initialize") => {
                // Name the session so claims and undo authorship survive reconnects.
                if let Some(params) = message.get_mut("params").and_then(Value::as_object_mut) {
                    let capabilities = params.entry("capabilities").or_insert(json!({}));
                    if let Some(c) = capabilities.as_object_mut() {
                        let experimental = c.entry("experimental").or_insert(json!({}));
                        if let Some(e) = experimental.as_object_mut() {
                            e.insert(mcp::SESSION_CAPABILITY.into(), json!({"id":self.id}));
                        }
                    }
                }
                self.initialize = Some(message.clone());
            }
            Some("notifications/initialized") => self.initialized = true,
            _ => {}
        }
        let id = match (message.get("id"), method) {
            (Some(id), Some(method)) => {
                let key = id.to_string();
                self.pending.insert(key.clone(), method);
                Some(key)
            }
            _ => None,
        };
        (message.to_string(), id)
    }
    /// Track a server → agent line. Returns None for lines the agent must not see.
    pub fn incoming(&mut self, raw: String) -> Option<String> {
        let Ok(message) = serde_json::from_str::<Value>(&raw) else {
            return Some(raw);
        };
        if message.get("method").is_none()
            && let Some(id) = message.get("id")
        {
            let key = id.to_string();
            if self.replay.as_ref() == Some(&key) {
                self.replay = None;
                return None;
            }
            self.pending.remove(&key);
        }
        Some(raw)
    }
    /// Lines that re-establish an initialized session on a fresh connection.
    pub fn handshake(&mut self) -> Vec<String> {
        let Some(mut initialize) = self.initialize.clone().filter(|_| self.initialized) else {
            return vec![];
        };
        self.replays += 1;
        let id = json!(format!("cardboard-relay-{}", self.replays));
        self.replay = Some(id.to_string());
        initialize["id"] = id;
        vec![
            initialize.to_string(),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        ]
    }
    /// The connection dropped: answer every unanswered request so the agent can carry on.
    pub fn abandon(&mut self) -> Vec<String> {
        self.replay = None;
        self.pending
            .drain()
            .map(|(key, method)| {
                let id: Value = serde_json::from_str(&key).unwrap_or(Value::Null);
                if method == "tools/call" {
                    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":LOST}],"isError":true}})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":LOST}})
                }
                .to_string()
            })
            .collect()
    }
}

/// Connect to the owner of the socket, hosting the library ourselves when nobody answers.
/// `patient` gives a restarting app a moment before we take the library.
async fn connect(
    options: &Options,
    host: &mut Option<Host>,
    patient: bool,
) -> io::Result<(Reader, Writer)> {
    let grace = if patient { 5 } else { 0 };
    let mut attempt = 0;
    loop {
        let error = match dial(&options.socket).await {
            Ok(connection) => return Ok(connection),
            Err(e) => e,
        };
        if options.fallback && host.is_none() && attempt >= grace {
            match Host::start(&options.socket, &options.library).await {
                Ok(h) => {
                    *host = Some(h);
                    continue;
                }
                // Another agent session became the host first; connect to it.
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => {}
                Err(e) => return Err(e),
            }
        }
        let limit = if options.fallback || patient { 150 } else { 0 };
        if attempt >= limit {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "Cannot connect to Cardboard at {}: {error}. Open the desktop app or `cardboard serve`, run `cardboard doctor`, or use mcp --document PATH for standalone editing.",
                    options.socket.display()
                ),
            ));
        }
        attempt += 1;
        tokio::time::sleep(STEP).await;
    }
}

async fn send(writer: &mut Writer, line: &str) -> io::Result<()> {
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

/// Relay stdin/stdout until the agent closes stdin or `shutdown` resolves.
pub async fn run(options: Options, shutdown: impl Future<Output = ()>) -> io::Result<()> {
    let (tx, mut agent) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut stdout = tokio::io::stdout();
    let mut session = Session::new(format!("r-{}", crate::store::random_hex(6)));
    let mut host: Option<Host> = None;
    let mut unsent: Option<String> = None;
    let mut connected_before = false;
    tokio::pin!(shutdown);
    let result: io::Result<()> = async {
        'session: loop {
            let (mut from, mut to) = tokio::select! {
                c = connect(&options, &mut host, connected_before) => c?,
                _ = &mut shutdown => break,
            };
            connected_before = true;
            let mut alive = true;
            for line in session.handshake() {
                alive = alive && send(&mut to, &line).await.is_ok();
            }
            if alive && let Some(raw) = unsent.take() {
                let (line, id) = session.outgoing(&raw);
                if send(&mut to, &line).await.is_err() {
                    if let Some(id) = id {
                        session.pending.remove(&id);
                    }
                    unsent = Some(raw);
                    alive = false;
                }
            }
            while alive {
                tokio::select! {
                    raw = agent.recv() => {
                        let Some(raw) = raw else { break 'session };
                        let (line, id) = session.outgoing(&raw);
                        if send(&mut to, &line).await.is_err() {
                            // Never delivered: send it again after reconnecting.
                            if let Some(id) = id {
                                session.pending.remove(&id);
                            }
                            unsent = Some(raw);
                            alive = false;
                        }
                    }
                    line = from.next_line() => match line {
                        Ok(Some(line)) => {
                            if let Some(line) = session.incoming(line) {
                                stdout.write_all(format!("{line}\n").as_bytes()).await?;
                                stdout.flush().await?;
                            }
                        }
                        _ => alive = false,
                    },
                    _ = &mut shutdown => break 'session,
                }
            }
            for line in session.abandon() {
                stdout.write_all(format!("{line}\n").as_bytes()).await?;
            }
            stdout.flush().await?;
            // A retired host handed the library to the app, which owns the socket now.
            if let Some(retired) = host.take_if(|h| h.hub().is_retired()) {
                retired.shutdown();
                eprintln!("Cardboard: the app took over the library; reconnecting");
            } else {
                eprintln!("Cardboard: connection lost; reconnecting");
            }
        }
        Ok(())
    }
    .await;
    if let Some(host) = host {
        host.shutdown();
    }
    result
}
