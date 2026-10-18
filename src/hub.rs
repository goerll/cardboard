//! The shared editing session: one Store behind a lock, plus a change signal for push clients.
use crate::{document::Result, store::Store, tools};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::watch;

pub struct Hub {
    store: Mutex<Store>,
    changed: watch::Sender<u64>,
}

impl Hub {
    pub fn new(store: Store) -> Arc<Self> {
        let generation = store.generation;
        Arc::new(Self {
            store: Mutex::new(store),
            changed: watch::channel(generation).0,
        })
    }
    pub fn lock(&self) -> MutexGuard<'_, Store> {
        // A panic mid-command leaves the last committed state; keep serving it.
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Run a tool and wake every subscriber if anything changed.
    pub fn call(&self, name: &str, args: &Value) -> Result<Value> {
        // Rendering takes a browser launch; snapshot the document and render without the lock.
        if crate::render::TOOLS.contains(&name) {
            let job = crate::render::prepare(&self.lock(), name, args)?;
            return job.run();
        }
        let (result, generation) = {
            let mut store = self.lock();
            let result = tools::call(&mut store, name, args);
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
                let due = {
                    let store = hub.lock();
                    store.needs_flush() && store.last_edit.elapsed() >= Duration::from_millis(300)
                };
                if due && let Err(e) = hub.flush() {
                    eprintln!("Autosave failed: {e}");
                }
            }
        })
    }
}
