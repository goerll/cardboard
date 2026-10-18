//! Open documents, per-file history, the local library and debounced atomic saves.
use crate::{
    assets,
    document::{Document, Page, Result},
    presence::{Actor, Claims},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const EXTENSION: &str = "cardboard";
const HISTORY: usize = 200;
const META: &str = ".cardboard-library.json";
const ARCHIVE: &str = ".archive";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("operating system randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn new_file_id() -> String {
    format!("f-{}", random_hex(8))
}
pub fn random_name() -> String {
    const A: &[&str] = &[
        "Lively",
        "Inventive",
        "Quiet",
        "Bright",
        "Gentle",
        "Curious",
        "Bold",
        "Calm",
        "Golden",
        "Wandering",
        "Hidden",
        "Silver",
        "Misty",
        "Sunny",
        "Velvet",
        "Swift",
        "Patient",
        "Amber",
        "Crisp",
        "Distant",
        "Merry",
        "Tidy",
        "Brave",
        "Lucid",
    ];
    const N: &[&str] = &[
        "iris", "lake", "river", "meadow", "fern", "willow", "harbor", "canyon", "orchard", "moss",
        "pine", "tide", "dune", "glacier", "maple", "lagoon", "valley", "brook", "cedar", "aurora",
        "reef", "prairie", "summit", "clover",
    ];
    let mut b = [0u8; 2];
    getrandom::fill(&mut b).expect("operating system randomness");
    format!(
        "{} {}",
        A[b[0] as usize % A.len()],
        N[b[1] as usize % N.len()]
    )
}

#[derive(Serialize, Deserialize)]
struct FileFormat<D> {
    format: String,
    version: u32,
    document: D,
    /// Base64 copies of the assets the document references, so the file is self-contained.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    assets: BTreeMap<String, String>,
}

pub fn read_document(path: &Path) -> Result<Document> {
    let bytes = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value.get("formatVersion").is_some() {
        return Err(format!(
            "{} is a legacy Cardboard workspace. Open the desktop app or `cardboard serve` once to import it into your library.",
            path.display()
        ));
    }
    if value.get("format").and_then(Value::as_str) != Some("cardboard") {
        return Err(format!("{} is not a Cardboard file", path.display()));
    }
    if value.get("version").and_then(Value::as_u64) != Some(2) {
        return Err("Unsupported Cardboard file version".into());
    }
    let doc: Document =
        serde_json::from_value(value["document"].clone()).map_err(|e| e.to_string())?;
    doc.validate()?;
    if let Some(embedded) = value.get("assets").and_then(Value::as_object) {
        let dir = assets::dir_for_file(path);
        for (name, data) in embedded {
            // A damaged embedded asset only loses that image, not the file.
            if let Err(e) = assets::restore(&dir, name, data.as_str().unwrap_or_default()) {
                eprintln!("{}: {e}", path.display());
            }
        }
    }
    Ok(doc)
}
pub fn write_document(path: &Path, doc: &Document) -> Result<()> {
    let dir = assets::dir_for_file(path);
    let assets = assets::references(doc)
        .into_iter()
        .filter_map(|name| Some((name.clone(), assets::encode(&assets::read(&dir, &name)?))))
        .collect();
    let bytes = serde_json::to_vec(&FileFormat {
        format: "cardboard".into(),
        version: 2,
        document: doc,
        assets,
    })
    .map_err(|e| e.to_string())?;
    write_atomic(path, &bytes)
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let temp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        fs::write(&temp, bytes)?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| format!("Could not save {}: {e}", path.display()))
}

/// Top-level artboard frames as (x, y, width, height): explicit positions, else left to right.
pub fn artboard_frames(page: &Page) -> Vec<(f64, f64, f64, f64)> {
    let mut right: f64 = 0.0;
    let mut first = true;
    page.artboards
        .iter()
        .map(|n| {
            let px = |k: &str| {
                n.styles
                    .get(k)
                    .and_then(|v| v.trim().strip_suffix("px"))
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0)
            };
            let (w, h) = (px("width"), px("height"));
            let [x, y] = n
                .position
                .unwrap_or([if first { 0.0 } else { right + 80.0 }, 0.0]);
            right = if first { x + w } else { right.max(x + w) };
            first = false;
            (x, y, w, h)
        })
        .collect()
}

pub struct OpenFile {
    pub doc: Document,
    pub path: Option<PathBuf>,
    /// Snapshots with the agent whose edit they undo (None: the person in the editor).
    undo: Vec<(Document, Option<Actor>)>,
    redo: Vec<(Document, Option<Actor>)>,
    pub dirty: bool,
    /// Agents' soft locks on layers; not saved, not history.
    pub claims: Claims,
    /// Bumps on every observable change, including selection and page switches.
    pub seq: u64,
}
impl OpenFile {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        Self {
            doc,
            path,
            undo: vec![],
            redo: vec![],
            dirty: false,
            claims: Claims::default(),
            seq: 1,
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryMeta {
    /// Path → last opened (unix ms).
    pub recent: BTreeMap<String, u64>,
    pub tabs: Vec<String>,
    pub active: Option<String>,
    pub settings: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyWorkspace {
    files: BTreeMap<String, Document>,
}

pub struct Store {
    /// Directory holding `.cardboard` files; None keeps everything in memory.
    pub library: Option<PathBuf>,
    pub files: BTreeMap<String, OpenFile>,
    pub tabs: Vec<String>,
    pub active_file: Option<String>,
    pub meta: LibraryMeta,
    meta_dirty: bool,
    /// Save each edit before acknowledging it and roll back on failure (standalone MCP).
    pub immediate: bool,
    /// Bumps on any change; servers watch it.
    pub generation: u64,
    /// Bumps when the library listing may have changed.
    pub library_seq: u64,
    pub last_edit: Instant,
    /// The agent running the current command, None for the person in the editor.
    pub actor: Option<Actor>,
}

impl Store {
    fn empty(library: Option<PathBuf>) -> Self {
        Self {
            library,
            files: BTreeMap::new(),
            tabs: vec![],
            active_file: None,
            meta: LibraryMeta::default(),
            meta_dirty: false,
            immediate: false,
            generation: 0,
            library_seq: 0,
            last_edit: Instant::now(),
            actor: None,
        }
    }
    /// An in-memory store with one open, active "Untitled" file.
    pub fn memory() -> Self {
        let mut s = Self::empty(None);
        let id = new_file_id();
        s.files.insert(
            id.clone(),
            OpenFile::new(Document::new(id.clone(), "Untitled"), None),
        );
        s.tabs.push(id.clone());
        s.active_file = Some(id);
        s
    }
    /// Standalone editing of one `.cardboard` file. It is created on the first saved edit.
    pub fn open_document(path: &Path) -> Result<Self> {
        let mut s = Self::empty(path.parent().map(Path::to_path_buf));
        s.immediate = true;
        let doc = if path.exists() {
            read_document(path)?
        } else {
            let name = path.file_stem().map_or("Untitled".into(), |n| {
                n.to_string_lossy()
                    .trim_end_matches(".cardboard")
                    .to_string()
            });
            Document::new(new_file_id(), &name)
        };
        let id = doc.id.clone();
        s.files
            .insert(id.clone(), OpenFile::new(doc, Some(path.into())));
        s.tabs.push(id.clone());
        s.active_file = Some(id.clone());
        s.migrate_images(&id);
        Ok(s)
    }
    /// A library directory: restores tabs and imports a legacy workspace on first run.
    pub fn library(dir: &Path, legacy: Option<&Path>) -> Result<Self> {
        fs::create_dir_all(dir)
            .map_err(|e| format!("Could not create library {}: {e}", dir.display()))?;
        let dir = dir.canonicalize().map_err(|e| e.to_string())?;
        let mut s = Self::empty(Some(dir.clone()));
        let meta_path = dir.join(META);
        if meta_path.exists() {
            s.meta = serde_json::from_slice(&fs::read(&meta_path).map_err(|e| e.to_string())?)
                .unwrap_or_default();
        } else if let Some(legacy) = legacy
            && legacy.exists()
            && s.scan(false).is_empty()
        {
            if let Err(e) = s.import_legacy(legacy) {
                eprintln!("Could not import legacy workspace: {e}");
            }
            s.meta_dirty = true;
        }
        let (tabs, active) = (s.meta.tabs.clone(), s.meta.active.clone());
        for path in tabs {
            if let Ok(id) = s.open_path(Path::new(&path), false)
                && active.as_deref() == Some(path.as_str())
            {
                s.active_file = Some(id);
            }
        }
        Ok(s)
    }
    pub fn default_library() -> PathBuf {
        if let Some(dir) = std::env::var_os("CARDBOARD_LIBRARY") {
            return dir.into();
        }
        home_dir().join("Documents").join("Cardboard")
    }
    pub fn legacy_workspace() -> PathBuf {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local/share"))
            .join("cardboard/workspace.json")
    }
    /// Convert each file of a v1 workspace into its own `.cardboard`. Returns the count.
    pub fn import_legacy(&mut self, path: &Path) -> Result<usize> {
        let dir = self.library.clone().ok_or("No library directory")?;
        let legacy: LegacyWorkspace =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut count = 0;
        for (_, mut doc) in legacy.files {
            doc.id = new_file_id();
            doc.validate()?;
            let target = unique_path(&dir, &doc.name);
            write_document(&target, &doc)?;
            count += 1;
        }
        self.library_seq += 1;
        Ok(count)
    }

    /// The library-wide asset store, if this session has a directory.
    pub fn assets_dir(&self) -> Option<PathBuf> {
        self.library.as_ref().map(|l| l.join(assets::DIR))
    }
    /// Where an open file's assets live: next to its library (also for files opened from
    /// elsewhere), else the session's store.
    pub fn assets_for(&self, id: &str) -> Option<PathBuf> {
        match self.path_of(id) {
            Some(path) => Some(assets::dir_for_file(path)),
            None => self.assets_dir(),
        }
    }
    /// Every directory the editor may serve `/asset/` requests from.
    pub fn asset_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = self.assets_dir().into_iter().collect();
        for id in self.files.keys() {
            if let Some(dir) = self.assets_for(id)
                && !dirs.contains(&dir)
            {
                dirs.push(dir);
            }
        }
        dirs
    }
    /// The Unsplash access key: `UNSPLASH_ACCESS_KEY`, else the library setting `unsplashAccessKey`.
    pub fn unsplash_key(&self) -> Option<String> {
        std::env::var("UNSPLASH_ACCESS_KEY")
            .ok()
            .or_else(|| {
                self.meta.settings["unsplashAccessKey"]
                    .as_str()
                    .map(String::from)
            })
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }
    /// Older files embed images as base64 `data:` URLs; move them into the asset store when the
    /// file opens. The rewrite is saved like an edit but is not an undo step.
    fn migrate_images(&mut self, id: &str) {
        let Some(dir) = self.assets_for(id) else {
            return;
        };
        let Some(file) = self.files.get_mut(id) else {
            return;
        };
        match assets::externalize(&mut file.doc, &dir) {
            Ok(true) => {
                file.dirty = true;
                self.last_edit = Instant::now();
            }
            Ok(false) => {}
            Err(e) => eprintln!(
                "Could not move images of {} into the asset store: {e}",
                file.doc.name
            ),
        }
    }

    pub fn file_id(&self, args: &Value) -> Result<String> {
        let id = match args.get("fileId") {
            Some(v) => v.as_str().ok_or("fileId must be a string")?,
            None => self
                .active_file
                .as_deref()
                .ok_or("No file is open; call open_file or create_file")?,
        };
        if self.files.contains_key(id) {
            Ok(id.into())
        } else {
            Err(format!("File {id} is not open"))
        }
    }
    pub fn file(&self, id: &str) -> Result<&Document> {
        self.files
            .get(id)
            .map(|f| &f.doc)
            .ok_or_else(|| format!("File {id} is not open"))
    }
    pub fn open(&self, id: &str) -> Result<&OpenFile> {
        self.files
            .get(id)
            .ok_or_else(|| format!("File {id} is not open"))
    }
    pub fn active(&self) -> Result<&Document> {
        self.file(self.active_file.as_deref().ok_or("No file is open")?)
    }
    fn touched(&mut self, id: &str) {
        if let Some(f) = self.files.get_mut(id) {
            f.seq += 1;
            f.dirty = true;
        }
        self.generation += 1;
        self.last_edit = Instant::now();
    }
    fn tabs_changed(&mut self) {
        self.meta.tabs = self
            .tabs
            .iter()
            .filter_map(|id| {
                self.files
                    .get(id)?
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
            })
            .collect();
        self.meta.active = self
            .active_file
            .as_ref()
            .and_then(|id| self.files.get(id)?.path.as_ref())
            .map(|p| p.to_string_lossy().into_owned());
        self.meta_dirty = true;
        self.generation += 1;
        self.last_edit = Instant::now();
    }

    /// Apply an undoable edit to one file. Failed operations and invalid results leave no trace.
    pub fn transaction<T>(
        &mut self,
        id: &str,
        operation: impl FnOnce(&mut Document) -> Result<T>,
    ) -> Result<T> {
        let file = self
            .files
            .get_mut(id)
            .ok_or_else(|| format!("File {id} is not open"))?;
        let before = file.doc.clone();
        let value = match operation(&mut file.doc).and_then(|v| file.doc.validate().map(|_| v)) {
            Ok(v) => v,
            Err(e) => {
                file.doc = before;
                return Err(e);
            }
        };
        if file.doc == before {
            return Ok(value);
        }
        file.doc.revision = before.revision + 1;
        if self.immediate
            && let Some(path) = &file.path
            && let Err(e) = write_document(path, &file.doc)
        {
            file.doc = before;
            return Err(e);
        }
        file.undo.push((before, self.actor.clone()));
        if file.undo.len() > HISTORY {
            file.undo.remove(0);
        }
        file.redo.clear();
        let immediate = self.immediate;
        self.touched(id);
        if immediate && let Some(f) = self.files.get_mut(id) {
            f.dirty = false;
        }
        Ok(value)
    }
    /// A change that is visible to clients but not part of history (selection, active page).
    pub fn set_view<T>(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Document) -> Result<T>,
    ) -> Result<T> {
        let file = self
            .files
            .get_mut(id)
            .ok_or_else(|| format!("File {id} is not open"))?;
        let before = (file.doc.selection.clone(), file.doc.active_page.clone());
        let value = change(&mut file.doc)?;
        if (file.doc.selection.clone(), file.doc.active_page.clone()) != before {
            self.touched(id);
        }
        Ok(value)
    }
    pub fn undo(&mut self, id: &str) -> Result<()> {
        self.restore(id, false)
    }
    pub fn redo(&mut self, id: &str) -> Result<()> {
        self.restore(id, true)
    }
    fn restore(&mut self, id: &str, redo: bool) -> Result<()> {
        let file = self
            .files
            .get_mut(id)
            .ok_or_else(|| format!("File {id} is not open"))?;
        let stack = if redo { &mut file.redo } else { &mut file.undo };
        let author = &stack.last().ok_or("No history available")?.1;
        // An agent may only step back over its own edits, never the person's or another agent's.
        if let Some(actor) = &self.actor
            && author.as_ref().map(|a| &a.id) != Some(&actor.id)
        {
            let who = author
                .as_ref()
                .map_or("the person in the editor".to_string(), |a| {
                    format!("another agent ({})", a.name)
                });
            return Err(format!(
                "The latest {} step in this file was made by {who}; undoing it would revert their work. Fix forward with an edit instead.",
                if redo { "redo" } else { "undo" }
            ));
        }
        let (mut previous, author) = stack.pop().unwrap();
        let current = file.doc.clone();
        previous.revision = current.revision + 1;
        previous.next_id = previous.next_id.max(current.next_id);
        // View state is not history: keep the current page and selection where they still exist.
        if previous.pages.iter().any(|p| p.id == current.active_page) {
            previous.active_page = current.active_page.clone();
        }
        previous.selection = current
            .selection
            .iter()
            .filter(|n| previous.node(n).is_ok())
            .cloned()
            .collect();
        if self.immediate
            && let Some(path) = &file.path
            && let Err(e) = write_document(path, &previous)
        {
            if redo {
                file.redo.push((previous, author))
            } else {
                file.undo.push((previous, author))
            }
            return Err(e);
        }
        file.doc = previous;
        if redo {
            file.undo.push((current, author))
        } else {
            file.redo.push((current, author))
        }
        self.touched(id);
        Ok(())
    }

    /// Claims changed: push to clients without saving or touching history.
    pub fn presence_changed(&mut self, id: &str) {
        if let Some(f) = self.files.get_mut(id) {
            f.seq += 1;
            self.generation += 1;
        }
    }
    /// Drop expired claims everywhere. Returns whether clients need a push.
    pub fn expire_claims(&mut self) -> bool {
        let now = Instant::now();
        let changed: Vec<String> = self
            .files
            .iter_mut()
            .filter(|(_, f)| !f.claims.is_empty())
            .filter_map(|(id, f)| f.claims.prune(&f.doc, now).then(|| id.clone()))
            .collect();
        for id in &changed {
            self.presence_changed(id);
        }
        !changed.is_empty()
    }

    /// Write every dirty file and the library metadata. Files that fail stay dirty.
    pub fn flush(&mut self) -> Result<()> {
        let mut errors = vec![];
        let mut saved = false;
        for file in self.files.values_mut() {
            if !file.dirty {
                continue;
            }
            match &file.path {
                Some(path) => match write_document(path, &file.doc) {
                    Ok(()) => {
                        file.dirty = false;
                        saved = true;
                    }
                    Err(e) => errors.push(e),
                },
                None => file.dirty = false,
            }
        }
        if saved {
            self.library_seq += 1;
            self.generation += 1;
        }
        if self.meta_dirty
            && !self.immediate
            && let Some(dir) = &self.library
        {
            match serde_json::to_vec_pretty(&self.meta) {
                Ok(bytes) => match write_atomic(&dir.join(META), &bytes) {
                    Ok(()) => self.meta_dirty = false,
                    Err(e) => errors.push(e),
                },
                Err(e) => errors.push(e.to_string()),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    pub fn needs_flush(&self) -> bool {
        self.meta_dirty || self.files.values().any(|f| f.dirty)
    }

    // Library

    fn dir(&self) -> Result<&Path> {
        self.library
            .as_deref()
            .ok_or_else(|| "This session has no library directory".into())
    }
    /// Resolve a path argument to a `.cardboard` file inside the library (or its archive).
    pub fn library_path(&self, path: &str) -> Result<PathBuf> {
        let dir = self.dir()?;
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        let resolved = path
            .canonicalize()
            .map_err(|_| format!("File {} does not exist", path.display()))?;
        let parent = resolved.parent().unwrap_or(Path::new(""));
        if (parent != dir && parent != dir.join(ARCHIVE))
            || resolved.extension().is_none_or(|e| e != EXTENSION)
        {
            return Err("Path must be a .cardboard file in the library".into());
        }
        Ok(resolved)
    }
    pub fn is_archived(&self, path: &Path) -> bool {
        self.library
            .as_ref()
            .is_some_and(|d| path.parent() == Some(&d.join(ARCHIVE)))
    }
    pub fn path_of(&self, id: &str) -> Option<&PathBuf> {
        self.files.get(id)?.path.as_ref()
    }
    pub fn id_for_path(&self, path: &Path) -> Option<String> {
        self.files
            .iter()
            .find(|(_, f)| f.path.as_deref() == Some(path))
            .map(|(id, _)| id.clone())
    }
    /// `.cardboard` files in the library or its archive.
    pub fn scan(&self, archived: bool) -> Vec<PathBuf> {
        let Some(dir) = &self.library else {
            return vec![];
        };
        let dir = if archived {
            dir.join(ARCHIVE)
        } else {
            dir.clone()
        };
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == EXTENSION))
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Open (or focus) a file by path; returns its id.
    pub fn open_path(&mut self, path: &Path, activate: bool) -> Result<String> {
        let path = path
            .canonicalize()
            .map_err(|_| format!("File {} does not exist", path.display()))?;
        if path.extension().is_none_or(|e| e != EXTENSION) {
            return Err("Only .cardboard files can be opened".into());
        }
        let id = match self.id_for_path(&path) {
            Some(id) => id,
            None => {
                let mut doc = read_document(&path)?;
                // A copied file keeps its id; give it a fresh one so both can be open.
                if self.files.contains_key(&doc.id) {
                    doc.id = new_file_id();
                }
                let id = doc.id.clone();
                self.files
                    .insert(id.clone(), OpenFile::new(doc, Some(path.clone())));
                self.migrate_images(&id);
                id
            }
        };
        if !self.tabs.contains(&id) {
            self.tabs.push(id.clone());
        }
        if activate {
            self.active_file = Some(id.clone());
            self.meta
                .recent
                .insert(path.to_string_lossy().into_owned(), now_ms());
        }
        self.tabs_changed();
        Ok(id)
    }
    pub fn activate(&mut self, id: Option<&str>) -> Result<()> {
        if let Some(id) = id {
            self.open(id)?;
            if let Some(path) = self.path_of(id).map(|p| p.to_string_lossy().into_owned()) {
                self.meta.recent.insert(path, now_ms());
            }
        }
        self.active_file = id.map(String::from);
        self.tabs_changed();
        Ok(())
    }
    pub fn close(&mut self, id: &str) -> Result<()> {
        let file = self
            .files
            .get_mut(id)
            .ok_or_else(|| format!("File {id} is not open"))?;
        if file.dirty
            && let Some(path) = &file.path
        {
            write_document(path, &file.doc)?;
            self.library_seq += 1;
        }
        let index = self.tabs.iter().position(|t| t == id);
        self.files.remove(id);
        self.tabs.retain(|t| t != id);
        if self.active_file.as_deref() == Some(id) {
            self.active_file = index
                .and_then(|i| self.tabs.get(i.min(self.tabs.len().saturating_sub(1))))
                .cloned();
        }
        self.tabs_changed();
        Ok(())
    }
    /// Create and open a new file. Memory stores keep it unsaved.
    pub fn create(&mut self, name: Option<&str>) -> Result<String> {
        let name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map_or_else(random_name, String::from);
        let doc = Document::new(new_file_id(), &name);
        let id = doc.id.clone();
        let path = match &self.library {
            Some(dir) => {
                let path = unique_path(dir, &name);
                write_document(&path, &doc)?;
                Some(path)
            }
            None => None,
        };
        if path.is_some() {
            self.library_seq += 1;
        }
        self.files
            .insert(id.clone(), OpenFile::new(doc, path.clone()));
        self.tabs.push(id.clone());
        self.active_file = Some(id.clone());
        if let Some(p) = path {
            self.meta
                .recent
                .insert(p.to_string_lossy().into_owned(), now_ms());
        }
        self.tabs_changed();
        Ok(id)
    }
    /// Rename a file's document and its path on disk. Returns the new path, if any.
    pub fn rename(&mut self, target: Target, name: &str) -> Result<Option<PathBuf>> {
        let name = name.trim();
        if name.is_empty() {
            return Err("File name is empty".into());
        }
        match target {
            Target::Open(id) => {
                let file = self.files.get_mut(&id).ok_or("File is not open")?;
                file.doc.name = name.into();
                let mut new_path = None;
                let mut moved = None;
                if let Some(old) = file.path.clone() {
                    let dir = old.parent().unwrap_or(Path::new(".")).to_path_buf();
                    let path = if old.file_stem().is_some_and(|s| s == name) {
                        old.clone()
                    } else {
                        unique_path(&dir, name)
                    };
                    write_document(&path, &file.doc)?;
                    if path != old {
                        let _ = fs::remove_file(&old);
                        moved = Some((old, path.clone()));
                    }
                    file.path = Some(path.clone());
                    file.dirty = false;
                    new_path = Some(path);
                }
                file.seq += 1;
                if let Some((old, path)) = moved {
                    self.rename_recent(&old, &path);
                }
                self.library_seq += 1;
                self.tabs_changed();
                Ok(new_path)
            }
            Target::Path(old) => {
                let mut doc = read_document(&old)?;
                doc.name = name.into();
                let dir = old.parent().unwrap_or(Path::new(".")).to_path_buf();
                let path = unique_path(&dir, name);
                write_document(&path, &doc)?;
                fs::remove_file(&old).map_err(|e| e.to_string())?;
                self.rename_recent(&old, &path);
                self.library_seq += 1;
                self.generation += 1;
                Ok(Some(path))
            }
        }
    }
    fn rename_recent(&mut self, old: &Path, new: &Path) {
        if let Some(t) = self.meta.recent.remove(&*old.to_string_lossy()) {
            self.meta
                .recent
                .insert(new.to_string_lossy().into_owned(), t);
        }
        self.meta_dirty = true;
    }
    pub fn duplicate(&mut self, path: &Path) -> Result<PathBuf> {
        let mut doc = match self.id_for_path(path) {
            Some(id) => self.file(&id)?.clone(),
            None => read_document(path)?,
        };
        doc.id = new_file_id();
        doc.name = format!("{} copy", doc.name);
        doc.selection.clear();
        let target = unique_path(self.dir()?, &doc.name);
        write_document(&target, &doc)?;
        self.library_seq += 1;
        self.generation += 1;
        Ok(target)
    }
    /// Move a file into or out of the archive, closing it first.
    pub fn archive(&mut self, path: &Path, archive: bool) -> Result<PathBuf> {
        if self.is_archived(path) == archive {
            return Err(if archive {
                "File is already archived"
            } else {
                "File is not archived"
            }
            .into());
        }
        if let Some(id) = self.id_for_path(path) {
            self.close(&id)?;
        }
        let dir = self.dir()?.to_path_buf();
        let target_dir = if archive { dir.join(ARCHIVE) } else { dir };
        fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = unique_path(&target_dir, &stem);
        fs::rename(path, &target).map_err(|e| e.to_string())?;
        self.meta.recent.remove(&*path.to_string_lossy());
        self.meta_dirty = true;
        self.library_seq += 1;
        self.generation += 1;
        Ok(target)
    }
    pub fn delete(&mut self, path: &Path) -> Result<()> {
        if !self.is_archived(path) {
            return Err("Archive a file before deleting it".into());
        }
        fs::remove_file(path).map_err(|e| e.to_string())?;
        self.library_seq += 1;
        self.generation += 1;
        Ok(())
    }
    /// The library listing, most recent first. `previews` adds each artboard's HTML (the dashboard
    /// thumbnails); agents rarely need it and it dominates the response size.
    pub fn list(&self, archived: bool, previews: bool) -> Vec<Value> {
        const BUDGET: usize = 200 * 1024;
        let mut rows: Vec<(u64, Value)> = self
            .scan(archived)
            .into_iter()
            .filter_map(|path| {
                let open = self.id_for_path(&path);
                let loaded;
                let doc = match &open {
                    Some(id) => &self.files[id].doc,
                    None => {
                        loaded = read_document(&path).ok()?;
                        &loaded
                    }
                };
                let modified = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as u64);
                let key = path.to_string_lossy().into_owned();
                let opened = self.meta.recent.get(&key).copied();
                let mut budget = BUDGET;
                let page = doc.pages.first();
                let artboards = page
                    .map(|p| {
                        p.artboards
                            .iter()
                            .zip(artboard_frames(p))
                            .map(|(n, (x, y, w, h))| {
                                let mut row = json!({"id":n.id,"name":n.name,"x":x,"y":y,"w":w,"h":h});
                                if previews {
                                    let html = crate::html::node_html(n, false);
                                    row["html"] = json!(if html.len() <= budget {
                                        budget -= html.len();
                                        html
                                    } else {
                                        budget = 0;
                                        String::new()
                                    });
                                }
                                row
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Some((
                    opened.unwrap_or(0).max(modified),
                    json!({"path":key,"name":doc.name,"fileId":doc.id,"open":open.is_some(),"modified":modified,
                        "opened":opened,"archived":archived,"tokens":doc.tokens,"fonts":doc.fonts,"artboards":artboards,
                        "background":page.and_then(|p| p.background.clone())}),
                ))
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        rows.into_iter().map(|(_, v)| v).collect()
    }
    /// What the UI shows as tabs.
    pub fn workspace_info(&self) -> Value {
        json!({
            "tabs": self.tabs.iter().filter_map(|id| {
                let f = self.files.get(id)?;
                Some(json!({"fileId":id,"name":f.doc.name,"path":f.path.as_ref().map(|p| p.to_string_lossy())}))
            }).collect::<Vec<_>>(),
            "activeFile": self.active_file,
            "library": self.library.as_ref().map(|p| p.to_string_lossy()),
        })
    }
}

pub enum Target {
    Open(String),
    Path(PathBuf),
}

/// `<dir>/<name>.cardboard`, adding " 2", " 3"… when taken. Path separators are replaced.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let clean: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_start_matches('.');
    let clean = if clean.is_empty() { "Untitled" } else { clean };
    let clean: String = clean.chars().take(120).collect();
    let mut path = dir.join(format!("{clean}.{EXTENSION}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{clean} {n}.{EXTENSION}"));
        n += 1;
    }
    path
}

/// The user's home folder: `HOME` on Linux and macOS, `USERPROFILE` on Windows.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}
