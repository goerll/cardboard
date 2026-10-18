//! The shared editing session: one Store behind a lock, plus a change signal for push clients.
use crate::{document::Result, presence::Actor, store::Store, tools};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};
use tokio::sync::watch;

const RETIRED: &str =
    "This Cardboard session handed the library to the app; retry and the relay will reconnect";

pub struct Hub {
    store: Mutex<Store>,
    changed: watch::Sender<u64>,
    host: OnceLock<Value>,
    retired: watch::Sender<bool>,
}

impl Hub {
    pub fn new(store: Store) -> Arc<Self> {
        let generation = store.generation;
        Arc::new(Self {
            store: Mutex::new(store),
            changed: watch::channel(generation).0,
            host: OnceLock::new(),
            retired: watch::channel(false).0,
        })
    }
    pub fn lock(&self) -> MutexGuard<'_, Store> {
        // A panic mid-command leaves the last committed state; keep serving it.
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Describe who hosts this session (reported by get_basic_info). Defaults to the desktop app.
    pub fn set_host(&self, info: Value) {
        let _ = self.host.set(info);
    }
    pub fn host(&self) -> Value {
        self.host.get().cloned().unwrap_or(json!({"mode":"app"}))
    }
    /// Save everything and stop accepting commands, so another process can own the library.
    pub fn retire(&self) -> Result<()> {
        let mut store = self.lock();
        let result = store.flush();
        self.retired.send_replace(true);
        result
    }
    pub fn is_retired(&self) -> bool {
        *self.retired.borrow()
    }
    pub fn retired(&self) -> watch::Receiver<bool> {
        self.retired.subscribe()
    }
    /// Run a tool as the person in the editor and wake every subscriber if anything changed.
    pub fn call(&self, name: &str, args: &Value) -> Result<Value> {
        self.call_as(None, name, args)
    }
    /// Run a tool for an agent: its edits claim layers and its undo only reverts its own edits.
    pub fn call_as(&self, actor: Option<&Actor>, name: &str, args: &Value) -> Result<Value> {
        // Rendering takes a browser launch; snapshot the document and render without the lock.
        if crate::render::TOOLS.contains(&name) {
            let job = {
                let store = self.lock();
                if self.is_retired() {
                    return Err(RETIRED.into());
                }
                crate::render::prepare(&store, name, args)?
            };
            return job.run();
        }
        // Network work happens before taking the lock so the editor never waits on Iconify.
        if name == "search_icons" {
            return crate::icons::search(args);
        }
        crate::icons::prefetch(name, args)?;
        // Downloads happen outside the lock; the tool then runs on local assets only.
        let prepared;
        let args = if tools::needs_network(name, args) {
            let ctx = tools::context(&self.lock(), args);
            prepared = tools::prepare(name, args, &ctx)?;
            &prepared
        } else if name == "search_images" {
            let key = self.lock().unsplash_key();
            return crate::stock::search(args, key.as_deref());
        } else {
            args
        };
        let (result, generation) = {
            let mut store = self.lock();
            if self.is_retired() {
                return Err(RETIRED.into());
            }
            store.actor = actor.cloned();
            let mut result = tools::call(&mut store, name, args);
            store.actor = None;
            if actor.is_some() {
                let mut connection = self.host();
                connection["library"] = json!(store.library);
                match &mut result {
                    Ok(value) if matches!(name, "get_basic_info" | "open_file") => {
                        value["connection"] = connection;
                        if let Some(file) =
                            value["fileId"].as_str().and_then(|id| store.files.get(id))
                        {
                            value["agents"] = file.claims.to_json();
                        }
                    }
                    Err(e) if name == "get_basic_info" => {
                        e.push_str(&format!(" (connection: {connection})"));
                    }
                    _ => {}
                }
            }
            if store.immediate
                && let Err(e) = store.flush()
            {
                eprintln!("{e}");
            }
            (result, store.generation)
        };
        self.signal(generation);
        result
    }
    /// Wake subscribers after direct Store changes made through `lock`.
    pub fn signal(&self, generation: u64) {
        self.changed.send_if_modified(|g| {
            let changed = *g != generation;
            *g = generation;
            changed
        });
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }
    pub fn flush(&self) -> Result<()> {
        let (result, generation) = {
            let mut store = self.lock();
            if self.is_retired() {
                return Ok(());
            }
            (store.flush(), store.generation)
        };
        self.signal(generation);
        result
    }
    /// Save dirty files once edits have been quiet for ~300ms. Call inside a Tokio runtime.
    pub fn spawn_autosave(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let hub = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(150));
            loop {
                ticker.tick().await;
                if hub.is_retired() {
                    return;
                }
                let (due, generation) = {
                    let mut store = hub.lock();
                    store.expire_claims();
                    let due = store.needs_flush()
                        && store.last_edit.elapsed() >= Duration::from_millis(300);
                    (due, store.generation)
                };
                hub.signal(generation);
                if due && let Err(e) = hub.flush() {
                    eprintln!("Autosave failed: {e}");
                }
            }
        })
    }
}
