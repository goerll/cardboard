use crate::{
    assets, components,
    document::{Document, Node, Result, Store, Styles},
    html, presence, stock,
};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "Cardboard is a local HTML/CSS design editor; the person sees your edits live and shares your undo history (undo only reverts your own latest edit). Before anything else, read get_guide with topic cardboard-design. Start with get_basic_info and get_selection, and pass fileId and pageId explicitly. Post a short design plan before starting something new, add one visual group per write_html call, make repeated UI a component (create_component, insert_instance) and reuse layers with duplicate_nodes and move_nodes, and use the file's tokens. Most tools report authored CSS; use screenshot to see your work and get_bounds to measure it after each visual group, and only claim visual checks you actually looked at. Edited layers are marked as yours and other agents can't change them; call finish_working_on_nodes when done.";
pub const GUIDE: &str = include_str!("../guides/design.md");

pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{key} must be an array"))
}
fn css_value(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err("CSS value must be a string or number".into()),
    }
}
fn styles(v: &Value) -> Result<Styles> {
    let mut result = Styles::new();
    for (k, v) in v.as_object().ok_or("styles must be an object")? {
        let k = html::css_name(k);
        let v = css_value(v)?;
        html::validate_style(&k, &v)?;
        result.insert(k, v);
    }
    Ok(result)
}
fn node_info(n: &Node) -> Value {
    let mut info = json!({"id":n.id,"name":n.name,"componentType":n.tag,"textContent":n.text,"children":n.children.iter().map(|n| &n.id).collect::<Vec<_>>(),"visible":n.visible,"locked":n.locked,"styles":n.styles});
    if n.markup.is_some() {
        info["icon"] = json!(n.attributes.get("data-icon"));
    }
    if n.component {
        info["isComponent"] = json!(true);
    }
    if let Some(i) = &n.instance {
        info["instanceOf"] = json!(i.component_id);
        info["overrides"] = json!(i.overrides);
    }
    info
}
/// " (component)" / " (instance of node-4)" markers for compact listings.
fn component_mark(n: &Node) -> String {
    match (&n.instance, n.component) {
        (Some(i), _) => format!(" (instance of {})", i.component_id),
        (None, true) => " (component)".into(),
        _ => String::new(),
    }
}
fn tree(n: &Node, depth: usize, prefix: usize, result: &mut String) {
    result.push_str(&format!(
        "{}{} {} [{}]{}\n",
        "  ".repeat(prefix),
        n.tag,
        n.name,
        n.id,
        component_mark(n)
    ));
    if depth > 0 {
        for c in &n.children {
            tree(c, depth - 1, prefix + 1, result);
        }
    }
}
fn parent_id(doc: &Document, id: &str) -> Option<String> {
    fn parent(n: &Node, id: &str) -> Option<String> {
        if n.children.iter().any(|c| c.id == id) {
            Some(n.id.clone())
        } else {
            n.children.iter().find_map(|c| parent(c, id))
        }
    }
    doc.pages
        .iter()
        .flat_map(|p| &p.artboards)
        .find_map(|n| parent(n, id))
}
fn page_for(doc: &Document, id: &str) -> Result<String> {
    doc.pages
        .iter()
        .find(|p| p.artboards.iter().any(|n| n.contains(id)))
        .map(|p| p.id.clone())
        .ok_or_else(|| format!("Node {id} does not exist"))
}
/// Child indexes count text too: a parent's own text becomes a leading "Text" layer before
/// anything is inserted beside it, so index 0 means before everything.
fn hoist_text(doc: &mut Document, parent: &str) -> Result<()> {
    if parent == "root" || doc.node(parent)?.text.is_empty() {
        return Ok(());
    }
    let mut span = Node::new(doc.allocate(), "span", "Text");
    let node = doc.node_mut(parent)?;
    span.text = std::mem::take(&mut node.text);
    node.children.insert(0, span);
    Ok(())
}
/// `write_html` takes inline `html` or an absolute `path` to a local .html/.htm/.svg file.
pub(crate) fn html_source(args: &Value) -> Result<String> {
    match (args.get("html"), args.get("path")) {
        (Some(html), None) => {
            let html = html.as_str().ok_or("html must be a string")?;
            if html.len() > crate::import::INLINE_LIMIT {
                return Err(
                    "Inline HTML is limited to 1 MiB; write it to a file and pass path".into(),
                );
            }
            Ok(html.into())
        }
        (None, Some(path)) => {
            let path = std::path::Path::new(path.as_str().ok_or("path must be a string")?);
            if !path.is_absolute() {
                return Err("path must be absolute".into());
            }
            if !path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm" | "svg"))
            {
                return Err("path must point to a .html, .htm or .svg file".into());
            }
            let size = std::fs::metadata(path)
                .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
                .len();
            if size > crate::import::FILE_LIMIT as u64 {
                return Err(format!(
                    "{} is {:.1} MiB; file imports are limited to 8 MiB. Split it into sections.",
                    path.display(),
                    size as f64 / 1048576.0
                ));
            }
            std::fs::read_to_string(path)
                .map_err(|e| format!("Cannot read {}: {e}", path.display()))
        }
        (Some(_), Some(_)) => Err("Pass html or path, not both".into()),
        (None, None) => Err("write_html needs html or path".into()),
    }
}
fn insert(
    doc: &mut Document,
    parent: &str,
    page: &str,
    node: Node,
    index: Option<usize>,
) -> Result<()> {
    hoist_text(doc, parent)?;
    let nodes = if parent == "root" {
        &mut doc.page_mut(page)?.artboards
    } else {
        let parent = doc.node_mut(parent)?;
        if html::is_void(&parent.tag) {
            return Err("This element cannot contain children".into());
        }
        &mut parent.children
    };
    let index = index.unwrap_or(nodes.len()).min(nodes.len());
    nodes.insert(index, node);
    Ok(())
}
fn jsx(n: &Node) -> String {
    let mut out = format!("<{}", n.tag);
    for (k, v) in &n.attributes {
        let key = if k == "class" { "className" } else { k };
        out.push_str(&format!(" {key}={{{}}}", serde_json::to_string(v).unwrap()));
    }
    if !n.styles.is_empty() || !n.visible {
        let mut map = serde_json::Map::new();
        for (k, v) in &n.styles {
            let mut key = String::new();
            let mut upper = false;
            if k.starts_with("--") {
                key = k.clone();
            } else {
                for c in k.chars() {
                    if c == '-' {
                        upper = true;
                    } else if upper {
                        key.push(c.to_ascii_uppercase());
                        upper = false;
                    } else {
                        key.push(c);
                    }
                }
            }
            map.insert(key, json!(v));
        }
        if !n.visible {
            map.insert("display".into(), json!("none"));
        }
        out.push_str(&format!(" style={{{}}}", Value::Object(map)));
    }
    if html::is_void(&n.tag) {
        out.push_str(" />");
        return out;
    }
    out.push('>');
    if !n.text.is_empty() {
        out.push_str(&format!("{{{}}}", json!(n.text)));
    }
    if let Some(markup) = &n.markup {
        out.push_str(&crate::icons::jsx(markup));
    }
    for child in &n.children {
        out.push_str(&jsx(child));
    }
    out.push_str(&format!("</{}>", n.tag));
    out
}

fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}
fn string_ids(args: &Value, key: &str) -> Result<Vec<String>> {
    array(args, key)?
        .iter()
        .map(|v| {
            v.as_str()
                .map(String::from)
                .ok_or_else(|| format!("{key} must contain strings"))
        })
        .collect()
}
/// The library path named by `path`, or the path of the open file named by `fileId`.
fn target_path(store: &Store, args: &Value) -> Result<std::path::PathBuf> {
    if let Some(path) = args.get("path") {
        return store.library_path(path.as_str().ok_or("path must be a string")?);
    }
    let id = store.file_id(args)?;
    store
        .path_of(&id)
        .cloned()
        .ok_or_else(|| "This file is not saved in the library".into())
}
fn opened(store: &Store, id: &str) -> Result<Value> {
    let mut info = store.file(id)?.info();
    info["path"] = json!(store.path_of(id).map(|p| p.to_string_lossy()));
    Ok(info)
}

/// Agents often drop the prefix (`581` for `node-581`). Node and page ids share one counter, so
/// the number alone is unambiguous. Style, attribute and HTML payloads are left untouched.
pub(crate) fn normalize_ids(v: &mut Value) {
    fn fix(v: &mut Value, prefix: &str) {
        let n = match v {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse::<u64>().ok(),
            Value::Array(items) => {
                items.iter_mut().for_each(|v| fix(v, prefix));
                None
            }
            _ => None,
        };
        if let Some(n) = n {
            *v = json!(format!("{prefix}{n}"));
        }
    }
    match v {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                match k.as_str() {
                    "nodeId" | "nodeIds" | "targetNodeId" | "parentId" | "componentId" => {
                        fix(v, "node-")
                    }
                    "pageId" | "pageIds" => fix(v, "page-"),
                    "id" => fix(v, "node-"),
                    "styles" | "attributes" | "tokens" | "html" => {}
                    _ => normalize_ids(v),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_ids),
        _ => {}
    }
}

/// Default output budget for text-heavy reads, sized to fit an agent's tool-result window.
const MAX_CHARS: usize = 50_000;
fn max_chars(args: &Value) -> usize {
    args.get("maxChars")
        .and_then(Value::as_u64)
        .map_or(MAX_CHARS, |n| if n == 0 { usize::MAX } else { n as usize })
}
/// Cut `value[key]` to the caller's budget, preferring a line break, and say how to narrow.
fn limited(mut value: Value, key: &str, args: &Value, hint: &str) -> Value {
    let max = max_chars(args);
    let text = value[key].as_str().unwrap_or_default();
    if text.len() <= max {
        return value;
    }
    let total = text.len();
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    if let Some(line) = text[..cut].rfind('\n') {
        cut = line + 1;
    }
    let clipped = format!(
        "{}\n[truncated: {cut} of {total} chars shown; {hint}, or pass maxChars: 0 for all of it]",
        &text[..cut]
    );
    value[key] = json!(clipped);
    value["truncated"] = json!(true);
    value["totalChars"] = json!(total);
    value
}
/// Serialize a node, replacing children below `depth` with a count.
fn pruned(n: &Node, depth: Option<u64>) -> Value {
    let mut v = json!(n);
    fn prune(v: &mut Value, depth: u64) {
        let Some(children) = v.get_mut("children").and_then(Value::as_array_mut) else {
            return;
        };
        if depth == 0 {
            let count = children.len();
            children.clear();
            if count > 0 {
                v["omittedChildren"] = json!(count);
            }
        } else {
            children.iter_mut().for_each(|c| prune(c, depth - 1));
        }
    }
    if let Some(depth) = depth {
        prune(&mut v, depth);
    }
    v
}

/// Node search shared by `find_nodes` and the `query` form of bulk edits.
struct Query {
    page: Option<String>,
    node: Option<String>,
    text: Option<String>,
    name: Option<String>,
    tag: Option<String>,
    styles: Vec<(String, StyleTest)>,
}
enum StyleTest {
    Present(bool),
    Equals(String),
    Contains(String),
}
struct Hit<'a> {
    page: &'a str,
    artboard: &'a str,
    node: &'a Node,
}
fn fold(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
impl Query {
    fn parse(v: &Value) -> Result<Self> {
        let raw = |key: &str| -> Result<Option<String>> {
            v.get(key)
                .map(|s| {
                    s.as_str()
                        .map(String::from)
                        .ok_or(format!("{key} must be a string"))
                })
                .transpose()
        };
        let text = |key: &str| Ok::<_, String>(raw(key)?.map(|s| fold(&s)));
        let mut styles = vec![];
        if let Some(map) = v.get("styles") {
            for (k, test) in map.as_object().ok_or("styles must be an object")? {
                let test = match test {
                    Value::Bool(b) => StyleTest::Present(*b),
                    Value::String(_) | Value::Number(_) => StyleTest::Equals(fold(&css_value(test)?)),
                    Value::Object(o) => StyleTest::Contains(fold(
                        o.get("contains").and_then(Value::as_str).ok_or(
                            "A style test is a value, true/false (present/absent) or {\"contains\": text}",
                        )?,
                    )),
                    _ => return Err("A style test is a value, true/false (present/absent) or {\"contains\": text}".into()),
                };
                let k = if k == "*" {
                    k.clone()
                } else {
                    html::css_name(k)
                };
                styles.push((k, test));
            }
        }
        Ok(Self {
            page: raw("pageId")?,
            node: raw("nodeId")?,
            text: text("text")?,
            name: text("name")?,
            tag: text("tag")?,
            styles,
        })
    }
    fn has_filter(&self) -> bool {
        self.text.is_some() || self.name.is_some() || self.tag.is_some() || !self.styles.is_empty()
    }
    fn matches(&self, n: &Node) -> bool {
        let style = |k: &str, test: &StyleTest| {
            let values: Vec<&String> = if k == "*" {
                n.styles.values().collect()
            } else {
                n.styles.get(k).into_iter().collect()
            };
            match test {
                StyleTest::Present(b) => values.is_empty() != *b,
                StyleTest::Equals(want) => values.iter().any(|v| fold(v) == *want),
                StyleTest::Contains(want) => values.iter().any(|v| fold(v).contains(want)),
            }
        };
        // Icon layers match tag "icon" and their icon name (lucide:arrow-right), never their paths.
        let icon = n.markup.as_ref().and(n.attributes.get("data-icon"));
        self.text.as_ref().is_none_or(|t| fold(&n.text).contains(t))
            && self.name.as_ref().is_none_or(|t| {
                fold(&n.name).contains(t) || icon.is_some_and(|i| fold(i).contains(t))
            })
            && self.tag.as_ref().is_none_or(|t| {
                n.tag.eq_ignore_ascii_case(t) || (t == "icon" && n.markup.is_some())
            })
            && self.styles.iter().all(|(k, test)| style(k, test))
    }
    /// Matching nodes in document order, scoped to a page or subtree when given.
    fn run<'a>(&self, doc: &'a Document) -> Result<Vec<Hit<'a>>> {
        fn walk<'a>(
            q: &Query,
            page: &'a str,
            artboard: &'a str,
            n: &'a Node,
            out: &mut Vec<Hit<'a>>,
        ) {
            if q.matches(n) {
                out.push(Hit {
                    page,
                    artboard,
                    node: n,
                });
            }
            for c in &n.children {
                walk(q, page, artboard, c, out);
            }
        }
        if let Some(page) = &self.page
            && !doc.pages.iter().any(|p| &p.id == page)
        {
            return Err(format!("Page {page} does not exist"));
        }
        if let Some(id) = &self.node {
            doc.node(id)?;
        }
        let mut out = vec![];
        for p in doc
            .pages
            .iter()
            .filter(|p| self.page.as_ref().is_none_or(|id| &p.id == id))
        {
            for a in &p.artboards {
                match &self.node {
                    Some(id) => {
                        if let Some(n) = a.find(id) {
                            walk(self, &p.id, &a.id, n, &mut out);
                        }
                    }
                    None => walk(self, &p.id, &a.id, a, &mut out),
                }
            }
        }
        Ok(out)
    }
}
/// What an edit item's `query` matches, or nothing without one (or with an invalid one, which the
/// edit itself reports).
pub(crate) fn query_ids(doc: &Document, item: &Value) -> Vec<String> {
    let Some(query) = item.get("query").and_then(|q| Query::parse(q).ok()) else {
        return vec![];
    };
    if !query.has_filter() && query.node.is_none() {
        return vec![];
    }
    query.run(doc).map_or(vec![], |hits| {
        hits.iter().map(|h| h.node.id.clone()).collect()
    })
}
/// The ids an edit item targets: its explicit `key` ids, or everything its `query` matches.
fn targets(doc: &Document, item: &Value, key: &str) -> Result<Vec<String>> {
    match item.get("query") {
        Some(q) => {
            let q = Query::parse(q)?;
            if !q.has_filter() && q.node.is_none() {
                return Err("query needs text, name, tag or styles (or a nodeId subtree) so it cannot match the whole file".into());
            }
            Ok(q.run(doc)?.iter().map(|h| h.node.id.clone()).collect())
        }
        None if key == "nodeId" => Ok(vec![
            string(item, key)
                .map_err(|_| "Give nodeId or query".to_string())?
                .into(),
        ]),
        None => string_ids(item, key).map_err(|_| format!("Give {key} or query")),
    }
}

/// Tools that only read one document; they may also run inside `batch`, seeing earlier edits.
const READS: &[&str] = &[
    "get_document",
    "get_basic_info",
    "get_selection",
    "get_node_info",
    "get_children",
    "get_tree_summary",
    "get_styles",
    "get_tokens",
    "get_jsx",
    "export_html",
    "find_nodes",
];

/// Tools that change one document and therefore can run inside `batch`.
pub const MUTATIONS: &[&str] = &[
    "create_page",
    "rename_pages",
    "update_pages",
    "delete_pages",
    "duplicate_page",
    "create_artboard",
    "write_html",
    "update_styles",
    "set_text_content",
    "rename_nodes",
    "delete_nodes",
    "duplicate_nodes",
    "move_nodes",
    "create_tokens",
    "set_tokens",
    "set_attributes",
    "set_node_state",
    "set_positions",
    "select_nodes",
    "set_active_page",
    "insert_icon",
    "remove_font",
    "create_component",
    "insert_instance",
    "detach_instance",
    "reset_overrides",
];

pub fn call(store: &mut Store, name: &str, args: &Value) -> Result<Value> {
    let mut args = args.clone();
    normalize_ids(&mut args);
    let args = &args;
    if needs_network(name, args) {
        let ctx = context(store, args);
        return call(store, name, &prepare(name, args, &ctx)?);
    }
    match name {
        "search_images" => return stock::search(args, store.unsplash_key().as_deref()),
        "import_image" => return import_image(store, args),
        "add_font" => return add_font(store, args),
        "list_fonts" => return list_fonts(store, args),
        "get_guide" => {
            return match string(args, "topic")? {
                "cardboard-design" => Ok(json!({"guide":GUIDE})),
                _ => Err("Available guide: cardboard-design".into()),
            };
        }
        "list_resources" => {
            return Ok(
                json!({"resources":store.tabs.iter().filter_map(|id| store.files.get(id)).map(|f| json!({"id":f.doc.id,"name":f.doc.name,"type":"file","open":true,"active":store.active_file.as_ref()==Some(&f.doc.id),"path":f.path.as_ref().map(|p| p.to_string_lossy())})).collect::<Vec<_>>()}),
            );
        }
        "list_files" => {
            return Ok(
                json!({"files":store.list(bool_arg(args, "archived"), bool_arg(args, "previews"))}),
            );
        }
        "search_icons" => return crate::icons::search(args),
        "create_file" => {
            let id = store.create(args.get("name").and_then(Value::as_str))?;
            return Ok(json!({"fileId":id,"path":store.path_of(&id).map(|p| p.to_string_lossy())}));
        }
        "open_file" => {
            let id = match (args.get("path"), args.get("fileId")) {
                (Some(path), _) => store.open_path(
                    std::path::Path::new(path.as_str().ok_or("path must be a string")?),
                    true,
                )?,
                (None, Some(_)) => {
                    let id = store.file_id(args)?;
                    store.activate(Some(&id))?;
                    id
                }
                _ => return Err("open_file needs path or fileId".into()),
            };
            return opened(store, &id);
        }
        "close_file" => {
            let id = store.file_id(args)?;
            store.close(&id)?;
            return Ok(store.workspace_info());
        }
        "show_dashboard" => {
            store.activate(None)?;
            return Ok(store.workspace_info());
        }
        "rename_file" => {
            let name = string(args, "name")?;
            let target = match (args.get("path"), args.get("fileId")) {
                (Some(_), _) => {
                    let path = target_path(store, args)?;
                    match store.id_for_path(&path) {
                        Some(id) => crate::store::Target::Open(id),
                        None => crate::store::Target::Path(path),
                    }
                }
                _ => crate::store::Target::Open(store.file_id(args)?),
            };
            let path = store.rename(target, name)?;
            return Ok(
                json!({"path":path.map(|p| p.to_string_lossy().into_owned()),"name":name.trim()}),
            );
        }
        "duplicate_file" => {
            let path = target_path(store, args)?;
            return Ok(json!({"path":store.duplicate(&path)?.to_string_lossy()}));
        }
        "archive_file" | "restore_file" => {
            let path = target_path(store, args)?;
            return Ok(
                json!({"path":store.archive(&path, name == "archive_file")?.to_string_lossy()}),
            );
        }
        "delete_file" => {
            let path = target_path(store, args)?;
            store.delete(&path)?;
            return Ok(json!({"deleted":path.to_string_lossy()}));
        }
        "screenshot" | "get_bounds" => return crate::render::prepare(store, name, args)?.run(),
        _ => {}
    }
    let id = store.file_id(args)?;
    let current = store.file(&id)?.revision;
    if let Some(expected) = args.get("expectedRevision")
        && expected.as_u64() != Some(current)
    {
        return Err(format!(
            "Stale edit rejected: expectedRevision is {expected} but the file is at revision {current}, \
             so it changed since you read it (the person or another agent edited it). Re-read the nodes \
             you are changing, then retry with expectedRevision {current}, or omit expectedRevision"
        ));
    }
    let doc = store.file(&id)?;
    let asset_dir = store.assets_for(&id);
    if let Some(mut value) = read(doc, name, args, asset_dir.as_deref())? {
        value["fileId"] = json!(id);
        value["revision"] = json!(doc.revision);
        return Ok(value);
    }
    let actor = store.actor.clone();
    let claims_apply = actor.is_some()
        && (name == "batch" || name == "start_working_on_nodes" || MUTATIONS.contains(&name))
        && !matches!(name, "select_nodes" | "set_active_page");
    let mut queried = vec![];
    if let Some(actor) = actor.as_ref().filter(|_| claims_apply) {
        let file = store.open(&id)?;
        queried = presence::check(&file.doc, &file.claims, actor, name, args)?;
    }
    // Pasted or agent-written base64 images move into the asset store as part of the same edit.
    let externalize = |doc: &mut Document| match &asset_dir {
        Some(dir) => assets::externalize(doc, dir).map(|_| ()),
        None => Ok(()),
    };
    let mut result = match name {
        "start_working_on_nodes" => {
            let ids = string_ids(args, "nodeIds")?;
            for node in &ids {
                store.file(&id)?.node(node)?;
            }
            json!({"claimed":ids,"expiresInSeconds":presence::CLAIM_TTL.as_secs()})
        }
        "finish_working_on_nodes" => {
            let ids = args
                .get("nodeIds")
                .map(|_| string_ids(args, "nodeIds"))
                .transpose()?;
            if let Some(actor) = &actor {
                let file = store.files.get_mut(&id).ok_or("File is not open")?;
                if file.claims.release(&file.doc, actor, ids.as_deref()) {
                    store.presence_changed(&id);
                }
            }
            json!({"finished":true})
        }
        "undo" | "redo" => {
            if name == "undo" {
                store.undo(&id)?
            } else {
                store.redo(&id)?
            }
            store.file(&id)?.info()
        }
        // View state: visible to every client, but outside undo history and revisions.
        "select_nodes" | "set_active_page" => store.set_view(&id, |doc| mutate(doc, name, args))?,
        "batch" => store.transaction(&id, |doc| {
            let mut results = vec![];
            for op in array(args, "operations")? {
                let tool = string(op, "tool")?;
                let op_args = op.get("args").cloned().unwrap_or(json!({}));
                if op_args.get("fileId").is_some_and(|f| f != &json!(id)) {
                    return Err("Batch operations must target the batch's file".into());
                }
                let result = if READS.contains(&tool) {
                    read(doc, tool, &op_args, asset_dir.as_deref()).map(Option::unwrap)
                } else if MUTATIONS.contains(&tool) {
                    apply(doc, tool, &op_args)
                } else {
                    return Err(format!(
                        "{tool} cannot run inside batch: batch takes one file's edits and reads \
                         (get_* except get_bounds, find_nodes, export_html). Call file, undo, redo, screenshot and \
                         get_bounds on their own"
                    ));
                };
                results.push(result.map_err(|e| format!("{tool}: {e}"))?);
            }
            externalize(doc)?;
            Ok(json!({"results":results}))
        })?,
        _ if MUTATIONS.contains(&name) => store.transaction(&id, |doc| {
            let value = apply(doc, name, args)?;
            externalize(doc)?;
            Ok(value)
        })?,
        _ => return Err(format!("Unknown tool: {name}")),
    };
    if let Some(actor) = actor.as_ref().filter(|_| claims_apply) {
        let file = store.files.get_mut(&id).ok_or("File is not open")?;
        presence::record(
            &file.doc,
            &mut file.claims,
            actor,
            name,
            args,
            &result,
            queried,
        );
        store.presence_changed(&id);
    }
    result["fileId"] = json!(id);
    result["revision"] = json!(store.file(&id)?.revision);
    Ok(result)
}

/// Read tools, or `None` when `name` is not one.
fn read(
    doc: &Document,
    name: &str,
    args: &Value,
    assets: Option<&std::path::Path>,
) -> Result<Option<Value>> {
    let depth = args.get("depth").and_then(Value::as_u64);
    let page = |id: &str| {
        doc.pages
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("Page {id} does not exist"))
    };
    Ok(Some(match name {
        "get_document" => {
            let value = if let Ok(id) = string(args, "nodeId") {
                json!({"node":pruned(doc.node(id)?, depth)})
            } else if let Ok(id) = string(args, "pageId") {
                let p = page(id)?;
                json!({"page":{"id":p.id,"name":p.name,"background":p.background,
                    "artboards":p.artboards.iter().map(|n| pruned(n, depth)).collect::<Vec<_>>()}})
            } else if depth.is_some() {
                let mut v = json!(doc);
                for (i, p) in doc.pages.iter().enumerate() {
                    v["pages"][i]["artboards"] = json!(
                        p.artboards
                            .iter()
                            .map(|n| pruned(n, depth))
                            .collect::<Vec<_>>()
                    );
                }
                json!({"document":v})
            } else {
                json!({"document":doc})
            };
            let size = value.to_string().len();
            let max = max_chars(args);
            if size > max {
                return Err(format!(
                    "get_document would return {size} chars, over the {max} limit. Narrow it with pageId, \
                     nodeId or depth; locate nodes with get_tree_summary or find_nodes; or pass maxChars: 0"
                ));
            }
            value
        }
        "get_basic_info" => doc.info(),
        "get_selection" => {
            json!({"nodes":doc.selection.iter().filter_map(|id| doc.node(id).ok().map(node_info)).collect::<Vec<_>>()})
        }
        "get_node_info" => node_info(doc.node(string(args, "nodeId")?)?),
        "get_children" => {
            json!({"children":doc.node(string(args,"nodeId")?)?.children.iter().map(node_info).collect::<Vec<_>>() })
        }
        "get_tree_summary" => {
            let roots: Vec<&Node> = match (string(args, "nodeId"), string(args, "pageId")) {
                (Ok(id), _) => vec![doc.node(id)?],
                (_, Ok(id)) => page(id)?.artboards.iter().collect(),
                _ => page(&doc.active_page)?.artboards.iter().collect(),
            };
            let mut text = String::new();
            for n in roots {
                tree(n, depth.unwrap_or(3).min(10) as usize, 0, &mut text);
            }
            limited(
                json!({"tree":text}),
                "tree",
                args,
                "pass a nodeId or a smaller depth",
            )
        }
        "get_styles" => {
            let mut map = serde_json::Map::new();
            for id in string_ids(args, "nodeIds")? {
                map.insert(id.clone(), json!(doc.node(&id)?.styles));
            }
            json!({"styles":map,"kind":"authored"})
        }
        "get_tokens" => {
            let format = args.get("format").and_then(Value::as_str).unwrap_or("json");
            match format {
                "json" => {
                    json!({"tokens":doc.tokens.iter().map(|(name,value)| json!({"name":name,"value":value})).collect::<Vec<_>>() })
                }
                "css" | "tailwind" => {
                    json!({"css":format!("{} {{\n{}\n}}",if format=="css" {":root"} else {"@theme"},doc.tokens.iter().map(|(k,v)|format!("  {k}: {v};")).collect::<Vec<_>>().join("\n"))})
                }
                _ => return Err("format must be json, css or tailwind".into()),
            }
        }
        "get_jsx" => {
            if args
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|f| f != "inline-styles")
            {
                return Err("This version exports inline-styles JSX; Tailwind conversion is not implemented yet".into());
            }
            limited(
                json!({"jsx":jsx(doc.node(string(args,"nodeId")?)?),"format":"inline-styles"}),
                "jsx",
                args,
                "export a smaller nodeId",
            )
        }
        "export_html" => {
            let html = match (string(args, "nodeId"), string(args, "pageId")) {
                (Ok(id), _) => html::nodes_html(doc, vec![doc.node(id)?], false),
                (_, Ok(id)) => html::nodes_html(doc, page(id)?.artboards.iter().collect(), false),
                _ => html::document_html(doc, false),
            };
            let html = match (assets, args.get("assets").and_then(Value::as_str)) {
                (None, _) => html,
                (Some(dir), None | Some("inline")) => {
                    assets::embed(&html, dir, assets::Embed::Inline)
                }
                (Some(dir), Some("files")) => assets::embed(&html, dir, assets::Embed::Files(dir)),
                _ => return Err("assets must be inline or files".into()),
            };
            limited(
                json!({"html":html}),
                "html",
                args,
                "export one artboard or node with nodeId, or link images with assets: \"files\"",
            )
        }
        "find_nodes" => {
            let hits = Query::parse(args)?.run(doc)?;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 500) as usize;
            let queried: Vec<&str> = args
                .get("styles")
                .and_then(Value::as_object)
                .map(|m| m.keys().map(String::as_str).collect())
                .unwrap_or_default();
            let nodes = hits
                .iter()
                .take(limit)
                .map(|h| {
                    let n = h.node;
                    let mut row = json!({"id":n.id,"name":n.name,"tag":n.tag,"pageId":h.page,"artboardId":h.artboard});
                    if !n.text.is_empty() {
                        let text: String = n.text.chars().take(80).collect();
                        row["text"] = json!(text);
                    }
                    if n.markup.is_some() {
                        row["icon"] = json!(n.attributes.get("data-icon"));
                    }
                    if n.component {
                        row["isComponent"] = json!(true);
                    }
                    if let Some(i) = &n.instance {
                        row["instanceOf"] = json!(i.component_id);
                    } else if let Some((instance, _)) = n.id.split_once(':') {
                        // Edits to this layer become overrides of that instance.
                        row["inInstance"] = json!(instance);
                    }
                    if !queried.is_empty() {
                        let styles: Styles = n
                            .styles
                            .iter()
                            .filter(|(k, _)| queried.iter().any(|q| *q == "*" || html::css_name(q) == **k))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        row["styles"] = json!(styles);
                    }
                    row
                })
                .collect::<Vec<_>>();
            json!({"nodes":nodes,"total":hits.len(),"truncated":hits.len() > limit})
        }
        _ => return Ok(None),
    }))
}

/// Run one editing tool, then bring component instances in line with their mains.
fn apply(doc: &mut Document, name: &str, args: &Value) -> Result<Value> {
    let before = components::snapshot(doc);
    let result = mutate(doc, name, args)?;
    components::reconcile(doc, before.as_ref())?;
    Ok(result)
}

/// Where a top-level copy lands on `page` when no x/y is given: right of the rightmost artboard.
fn place_right(doc: &Document, page: &str, y: f64) -> Option<[f64; 2]> {
    let page = doc.pages.iter().find(|p| p.id == page)?;
    crate::store::artboard_frames(page)
        .iter()
        .map(|f| f.0 + f.2)
        .reduce(f64::max)
        .map(|right| [right + 80.0, y])
}
fn xy(v: &Value) -> Option<[f64; 2]> {
    match (
        v.get("x").and_then(Value::as_f64),
        v.get("y").and_then(Value::as_f64),
    ) {
        (Some(x), Some(y)) => Some([x, y]),
        _ => None,
    }
}

fn mutate(doc: &mut Document, name: &str, args: &Value) -> Result<Value> {
    match name {
        "select_nodes" => {
            let ids = string_ids(args, "nodeIds")?;
            for id in &ids {
                doc.node(id)?;
            }
            doc.selection = ids;
            Ok(json!({"selection":doc.selection}))
        }
        "set_active_page" => {
            let page = string(args, "pageId")?;
            doc.page_mut(page)?;
            doc.active_page = page.into();
            doc.selection.clear();
            Ok(json!({"pageId":page}))
        }
        "create_page" => {
            let page_id = format!("page-{}", doc.next_id);
            doc.next_id += 1;
            doc.pages.push(crate::document::Page {
                id: page_id.clone(),
                name: args
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("New page")
                    .into(),
                artboards: vec![],
                background: None,
            });
            Ok(json!({"pageId":page_id}))
        }
        "rename_pages" => {
            for update in array(args, "updates")? {
                doc.page_mut(string(update, "pageId")?)?.name = string(update, "name")?.into();
            }
            Ok(json!({"updated":true}))
        }
        "update_pages" => {
            for update in array(args, "updates")? {
                let page = doc.page_mut(string(update, "pageId")?)?;
                if let Some(name) = update.get("name") {
                    page.name = name.as_str().ok_or("name must be a string")?.into();
                }
                if let Some(bg) = update.get("background") {
                    let bg = bg.as_str().ok_or("background must be a string")?.trim();
                    if bg.is_empty() {
                        page.background = None;
                    } else {
                        html::validate_style("background-color", bg)?;
                        page.background = Some(bg.into());
                    }
                }
            }
            Ok(json!({"updated":true}))
        }
        "delete_pages" => {
            let ids = string_ids(args, "pageIds")?;
            for id in &ids {
                doc.page_mut(id)?;
            }
            doc.pages.retain(|p| !ids.contains(&p.id));
            if doc.pages.is_empty() {
                return Err("A file needs at least one page".into());
            }
            if !doc.pages.iter().any(|p| p.id == doc.active_page) {
                doc.active_page = doc.pages[0].id.clone();
            }
            let valid = doc
                .selection
                .iter()
                .filter(|id| doc.node(id).is_ok())
                .cloned()
                .collect();
            doc.selection = valid;
            Ok(json!({"deleted":ids}))
        }
        "duplicate_page" => {
            let source = string(args, "pageId")?;
            let index = doc
                .pages
                .iter()
                .position(|p| p.id == source)
                .ok_or_else(|| format!("Page {source} does not exist"))?;
            let mut page = doc.pages[index].clone();
            page.id = format!("page-{}", doc.next_id);
            doc.next_id += 1;
            page.name = format!("{} copy", page.name);
            for n in &mut page.artboards {
                components::rekey(doc, n);
            }
            let id = page.id.clone();
            doc.pages.insert(index + 1, page);
            Ok(json!({"pageId":id}))
        }
        "create_artboard" => {
            let style = styles(args.get("styles").ok_or("styles is required")?)?;
            if !style.contains_key("width") || !style.contains_key("height") {
                return Err("Artboards require width and height".into());
            }
            let mut node = Node::new(doc.allocate(), "div", string(args, "name")?);
            node.styles = BTreeMap::from([
                ("display".into(), "flex".into()),
                ("flex-direction".into(), "column".into()),
            ]);
            node.styles.extend(style);
            if let (Some(x), Some(y)) = (
                args.get("x").and_then(Value::as_f64),
                args.get("y").and_then(Value::as_f64),
            ) {
                node.position = Some([x, y]);
            }
            let node_id = node.id.clone();
            let page = args
                .get("pageId")
                .and_then(Value::as_str)
                .unwrap_or(&doc.active_page)
                .to_owned();
            doc.page_mut(&page)?.artboards.push(node);
            Ok(json!({"nodeId":node_id,"pageId":page}))
        }
        "write_html" => {
            let target = string(args, "targetNodeId")?;
            doc.node(target)?;
            let source = html_source(args)?;
            let import = crate::import::parse(doc, &source)?;
            let nodes = import.nodes;
            let ids = nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
            match string(args, "mode")? {
                "insert-children" => {
                    hoist_text(doc, target)?;
                    let parent = doc.node_mut(target)?;
                    if html::is_void(&parent.tag) {
                        return Err("Target cannot contain children".into());
                    }
                    match args.get("index").and_then(Value::as_u64) {
                        Some(i) => {
                            let i = (i as usize).min(parent.children.len());
                            parent.children.splice(i..i, nodes);
                        }
                        None => parent.children.extend(nodes),
                    }
                }
                "replace" => doc.replace(target, nodes)?,
                "insert-after" | "insert-before" => {
                    let parent = parent_id(doc, target)
                        .ok_or("Use insert-children or replace for top-level artboards")?;
                    let siblings = &mut doc.node_mut(&parent)?.children;
                    let i = siblings.iter().position(|n| n.id == target).unwrap()
                        + usize::from(string(args, "mode")? == "insert-after");
                    siblings.splice(i..i, nodes);
                }
                _ => {
                    return Err(
                        "mode must be insert-children, insert-before, insert-after or replace"
                            .into(),
                    );
                }
            }
            let mut result = json!({"nodeIds":ids});
            if !import.warnings.is_empty() {
                result["warnings"] = json!(import.warnings);
            }
            Ok(result)
        }
        "update_styles" => {
            let (mut matched, mut changed) = (BTreeSet::new(), BTreeSet::new());
            for update in array(args, "updates")? {
                let patch = styles(update.get("styles").ok_or("styles is required")?)?;
                for id in targets(doc, update, "nodeIds")? {
                    let node = doc.node_mut(&id)?;
                    let before = node.styles.clone();
                    for (name, value) in &patch {
                        if value.is_empty() {
                            node.styles.remove(name);
                        } else {
                            node.styles.insert(name.clone(), value.clone());
                        }
                    }
                    if node.styles != before {
                        changed.insert(id.clone());
                    }
                    matched.insert(id);
                }
            }
            Ok(json!({"updated":true,"matched":matched.len(),"changed":changed.len()}))
        }
        "set_text_content" | "rename_nodes" => {
            for update in array(args, "updates")? {
                let node = doc.node_mut(string(update, "nodeId")?)?;
                if name == "rename_nodes" {
                    node.name = string(update, "name")?.into();
                } else {
                    if !node.children.is_empty() {
                        return Err(
                            "Text updates require a leaf text node; edit its text child instead"
                                .into(),
                        );
                    }
                    if html::is_void(&node.tag) {
                        return Err("This element cannot contain text".into());
                    }
                    node.text = string(update, "textContent")?.into();
                }
            }
            Ok(json!({"updated":true}))
        }
        "set_attributes" => {
            let (mut matched, mut changed) = (BTreeSet::new(), BTreeSet::new());
            for update in array(args, "updates")? {
                let attributes = update
                    .get("attributes")
                    .and_then(Value::as_object)
                    .ok_or("attributes must be an object")?;
                for id in targets(doc, update, "nodeId")? {
                    let node = doc.node_mut(&id)?;
                    let before = node.attributes.clone();
                    for (k, v) in attributes {
                        let v = v.as_str().ok_or("Attribute values must be strings")?;
                        if v.is_empty() {
                            node.attributes.remove(k);
                        } else {
                            html::validate_attribute(k, v)?;
                            node.attributes.insert(k.clone(), v.into());
                        }
                    }
                    if node.attributes != before {
                        changed.insert(id.clone());
                    }
                    matched.insert(id);
                }
            }
            Ok(json!({"updated":true,"matched":matched.len(),"changed":changed.len()}))
        }
        "set_node_state" => {
            for update in array(args, "updates")? {
                let node = doc.node_mut(string(update, "nodeId")?)?;
                if let Some(v) = update.get("visible") {
                    node.visible = v.as_bool().ok_or("visible must be a boolean")?;
                }
                if let Some(v) = update.get("locked") {
                    node.locked = v.as_bool().ok_or("locked must be a boolean")?;
                }
            }
            Ok(json!({"updated":true}))
        }
        "set_positions" => {
            for update in array(args, "updates")? {
                let id = string(update, "nodeId")?;
                let (x, y) = (
                    update
                        .get("x")
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .ok_or("x must be a number")?,
                    update
                        .get("y")
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .ok_or("y must be a number")?,
                );
                let node = doc
                    .pages
                    .iter_mut()
                    .flat_map(|p| &mut p.artboards)
                    .find(|n| n.id == id)
                    .ok_or_else(|| format!("{id} is not a top-level artboard"))?;
                node.position = Some([x, y]);
            }
            Ok(json!({"updated":true}))
        }
        "delete_nodes" => {
            let ids = targets(doc, args, "nodeIds")?;
            for id in &ids {
                doc.node(id)?;
            }
            // Ancestor deletion subsumes descendants without turning a valid batch into an error.
            let roots: Vec<_> = ids
                .iter()
                .filter(|id| {
                    !ids.iter()
                        .any(|other| other != *id && doc.node(other).is_ok_and(|n| n.contains(id)))
                })
                .cloned()
                .collect();
            for id in roots {
                doc.detach(&id)?;
            }
            let valid = doc
                .selection
                .iter()
                .filter(|id| doc.node(id).is_ok())
                .cloned()
                .collect();
            doc.selection = valid;
            Ok(json!({"count":ids.len(),"deleted":ids}))
        }
        "duplicate_nodes" => {
            let mut result = vec![];
            for spec in array(args, "nodes")? {
                let source = string(spec, "id")?;
                let source_page = page_for(doc, source)?;
                let page = match spec.get("pageId").and_then(Value::as_str) {
                    Some(p) => {
                        doc.page_mut(p)?;
                        p.to_owned()
                    }
                    None => source_page.clone(),
                };
                let explicit = spec
                    .get("parentId")
                    .and_then(Value::as_str)
                    .map(String::from)
                    .or_else(|| (page != source_page).then(|| "root".into()));
                let parent = explicit
                    .clone()
                    .or_else(|| parent_id(doc, source))
                    .unwrap_or("root".into());
                let mut node = doc.clone_node(source)?;
                // Place an unparented copy right after its source; offset duplicated artboards.
                let index = if let Some(i) = spec.get("index").and_then(Value::as_u64) {
                    Some(i as usize)
                } else if explicit.is_none() {
                    let siblings = if parent == "root" {
                        &doc.page_mut(&page)?.artboards
                    } else {
                        &doc.node(&parent)?.children
                    };
                    siblings.iter().position(|n| n.id == source).map(|i| i + 1)
                } else {
                    None
                };
                if parent == "root" {
                    let page_ref = doc.pages.iter().find(|p| p.id == source_page).unwrap();
                    let frames = crate::store::artboard_frames(page_ref);
                    if let Some(i) = page_ref.artboards.iter().position(|n| n.id == source) {
                        let (x, y, w, _) = frames[i];
                        let right = frames.iter().map(|f| f.0 + f.2).fold(x + w, f64::max);
                        node.position = Some(if page == source_page {
                            [right + 80.0, y]
                        } else {
                            place_right(doc, &page, y).unwrap_or([x, y])
                        });
                    }
                    if let Some(p) = xy(spec) {
                        node.position = Some(p);
                    }
                } else {
                    node.position = None;
                }
                if let Some(styles) = spec.get("styles").and_then(Value::as_object) {
                    for (k, v) in styles {
                        match v.as_str() {
                            Some("") | None => {
                                node.styles.remove(k);
                            }
                            Some(v) => {
                                node.styles.insert(k.clone(), v.to_string());
                            }
                        }
                    }
                }
                result.push(json!({"sourceId":source,"nodeId":node.id}));
                insert(doc, &parent, &page, node, index)?;
            }
            Ok(json!({"duplicates":result}))
        }
        "move_nodes" => {
            for movement in array(args, "moves")? {
                let source = string(movement, "nodeId")?;
                let source_page = page_for(doc, source)?;
                let page = match movement.get("pageId").and_then(Value::as_str) {
                    Some(p) => {
                        doc.page_mut(p)?;
                        p.to_owned()
                    }
                    None => source_page.clone(),
                };
                let parent = string(movement, "parentId")?;
                if doc.node(source)?.contains(parent) {
                    return Err("A node cannot be moved inside itself or a descendant".into());
                }
                if parent != "root" {
                    doc.node(parent)?;
                }
                let mut node = doc.detach(source)?;
                if parent != "root" {
                    node.position = None;
                } else if let Some(p) = xy(movement) {
                    node.position = Some(p);
                } else if page != source_page {
                    // Keep the artboard's place on the canvas unless it would land on others.
                    let y = node.position.map_or(0.0, |p| p[1]);
                    if let Some(p) = place_right(doc, &page, y) {
                        node.position = Some(p);
                    }
                }
                insert(
                    doc,
                    parent,
                    &page,
                    node,
                    movement
                        .get("index")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize),
                )?;
            }
            Ok(json!({"moved":true}))
        }
        "create_tokens" | "set_tokens" => {
            let creating = name == "create_tokens";
            for token in array(args, "tokens")? {
                let name = string(token, "name")?;
                if name.is_empty() {
                    return Err("Token name is empty".into());
                }
                if !creating && !doc.tokens.contains_key(name) {
                    return Err(format!(
                        "Token {name} does not exist; use create_tokens first"
                    ));
                }
                if token.get("delete").and_then(Value::as_bool) == Some(true) {
                    doc.tokens.remove(name);
                    continue;
                }
                if let Some(new_name) = token.get("newName") {
                    let new_name = new_name.as_str().ok_or("newName must be a string")?;
                    let value = doc.tokens.remove(name).ok_or("Token does not exist")?;
                    html::validate_token(new_name, &value)?;
                    if doc.tokens.contains_key(new_name) {
                        return Err("New token name is already used".into());
                    }
                    doc.tokens.insert(new_name.into(), value);
                    // Keep authored references synchronized with a token rename.
                    let old_ref = format!("var({name})");
                    let new_ref = format!("var({new_name})");
                    fn rewrite(n: &mut Node, from: &str, to: &str) {
                        for v in n.styles.values_mut() {
                            *v = v.replace(from, to);
                        }
                        for c in &mut n.children {
                            rewrite(c, from, to);
                        }
                    }
                    for n in doc.pages.iter_mut().flat_map(|p| &mut p.artboards) {
                        rewrite(n, &old_ref, &new_ref);
                    }
                    for v in doc.tokens.values_mut() {
                        *v = v.replace(&old_ref, &new_ref);
                    }
                }
                if let Some(value) = token.get("value") {
                    let target = token.get("newName").and_then(Value::as_str).unwrap_or(name);
                    let value = css_value(value)?;
                    html::validate_token(target, &value)?;
                    if creating && doc.tokens.contains_key(target) {
                        return Err("Token already exists".into());
                    }
                    doc.tokens.insert(target.into(), value);
                } else if creating {
                    return Err("A new token requires a value".into());
                }
            }
            Ok(json!({"tokens":doc.tokens}))
        }
        "insert_icon" => {
            // Same placement rules as write_html; the icon becomes one layer.
            let opt = |k: &str| args.get(k).map(css_value).transpose();
            let mut tag = format!("<icon name=\"{}\"", html::escape(string(args, "icon")?));
            for (attr, value) in [
                ("size", opt("size")?),
                ("stroke-width", opt("strokeWidth")?),
                ("color", opt("color")?),
                ("layer-name", opt("name")?),
            ] {
                if let Some(value) = value {
                    tag.push_str(&format!(" {attr}=\"{}\"", html::escape(&value)));
                }
            }
            tag.push_str(" />");
            let mut write = args.clone();
            write["html"] = json!(tag);
            if write.get("mode").is_none() {
                write["mode"] = json!("insert-children");
            }
            let result = mutate(doc, "write_html", &write)?;
            Ok(json!({"nodeId":result["nodeIds"][0]}))
        }
        "remove_font" => {
            let family = string(args, "family")?;
            let before = doc.fonts.len();
            doc.fonts.retain(|f| f.family != family);
            if doc.fonts.len() == before {
                return Err(format!("This file has no font family {family:?}"));
            }
            Ok(json!({"removed":family}))
        }
        "create_component" => {
            let id = string(args, "nodeId")?;
            let node = doc.node_mut(id)?;
            if components::is_inner(id) {
                return Err(format!(
                    "{id} is a layer inside an instance; detach_instance first or use the main component"
                ));
            }
            if node.instance.is_some() {
                return Err(format!(
                    "{id} is already an instance; detach_instance first or wrap it in a frame"
                ));
            }
            node.component = true;
            if let Some(name) = args.get("name").and_then(Value::as_str) {
                node.name = name.into();
            }
            Ok(json!({"componentId":id}))
        }
        "insert_instance" => {
            let component = string(args, "componentId")?;
            let parent = string(args, "parentId")?;
            let mut node = components::instantiate(doc, component)?;
            let page = match args.get("pageId").and_then(Value::as_str) {
                Some(p) => p.to_owned(),
                None if parent == "root" => doc.active_page.clone(),
                None => page_for(doc, parent)?,
            };
            if parent == "root" {
                node.position = xy(args).or_else(|| place_right(doc, &page, 0.0));
            } else {
                node.position = None;
            }
            let id = node.id.clone();
            let index = args
                .get("index")
                .and_then(Value::as_u64)
                .map(|i| i as usize);
            insert(doc, parent, &page, node, index)?;
            Ok(json!({"nodeId":id,"pageId":page}))
        }
        "detach_instance" | "reset_overrides" => {
            let ids = string_ids(args, "nodeIds")?;
            for id in &ids {
                if name == "detach_instance" {
                    components::detach(doc, id)?;
                } else {
                    components::reset(doc, id)?;
                }
            }
            Ok(json!({"nodeIds":ids}))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

use std::collections::{BTreeMap, BTreeSet};
pub fn definitions() -> Vec<Value> {
    let s = json!({"type":"string"});
    let ids = json!({"type":"array","items":s});
    let obj = json!({"type":"object"});
    let max = json!({"type":"integer","minimum":0,"description":"Output budget in characters (default 50000); 0 means no limit"});
    let query_props = json!({"pageId":s,"nodeId":{"type":"string","description":"Search only this node and its descendants"},"text":{"type":"string","description":"Case-insensitive substring of the node's own text"},"name":{"type":"string","description":"Case-insensitive substring of the layer name (or icon name, such as lucide:arrow)"},"tag":{"type":"string","description":"Exact tag, such as button or svg; icon finds icon layers"},"styles":{"type":"object","description":"Property (CSS or camelCase, or * for any) to a test: a value for an exact match, true/false for present/absent, or {\"contains\": text}"}});
    let query = json!({"type":"object","properties":query_props,"additionalProperties":false,"description":"Target every node find_nodes would match, instead of listing ids"});
    let mut tools = vec![];
    let mut add = |name: &str, description: &str, props: Value, required: Vec<&str>, read: bool| {
        let mut props = props.as_object().unwrap().clone();
        props.insert("fileId".into(), s.clone());
        if !read {
            props.insert(
                "expectedRevision".into(),
                json!({"type":"integer","minimum":0}),
            );
        }
        tools.push(json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":props,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read,"destructiveHint":!read,"openWorldHint":false}}));
    };
    add(
        "get_guide",
        "Read Cardboard's design workflow before creating a design.",
        json!({"topic":s}),
        vec!["topic"],
        true,
    );
    add(
        "list_resources",
        "List files currently open in tabs.",
        json!({}),
        vec![],
        true,
    );
    add(
        "create_file",
        "Create a new .cardboard file in the library, open it in a tab and make it active.",
        json!({"name":s}),
        vec![],
        false,
    );
    add(
        "open_file",
        "Open a .cardboard file by path (see list_files), or activate an open file by fileId.",
        json!({"path":s}),
        vec![],
        false,
    );
    add(
        "get_basic_info",
        "Inspect pages, artboards, dimensions and tokens before editing. Also reports `connection` (app, serve or headless, with an editor URL to share) and `agents` (layers other agents are working on).",
        json!({}),
        vec![],
        true,
    );
    add(
        "get_selection",
        "Read the user's selected nodes.",
        json!({}),
        vec![],
        true,
    );
    add(
        "get_node_info",
        "Inspect one design node. Styles are authored, not computed.",
        json!({"nodeId":s}),
        vec!["nodeId", "fileId"],
        true,
    );
    add(
        "get_children",
        "Read immediate children with stable identifiers.",
        json!({"nodeId":s}),
        vec!["nodeId", "fileId"],
        true,
    );
    add(
        "get_tree_summary",
        "Read a compact outline (tag, name, id per line) of one node, or of every artboard on a page (default: the active page); cheaper than full code export.",
        json!({"nodeId":s,"pageId":s,"depth":{"type":"integer","minimum":0,"maximum":10},"maxChars":max}),
        vec!["fileId"],
        true,
    );
    add(
        "get_styles",
        "Read authored CSS values and token references. This does not measure browser computed styles.",
        json!({"nodeIds":ids}),
        vec!["nodeIds", "fileId"],
        true,
    );
    add(
        "create_page",
        "Create a page without switching the user's current page.",
        json!({"name":s}),
        vec!["fileId"],
        false,
    );
    add(
        "rename_pages",
        "Rename pages in one atomic operation.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"pageId":s,"name":s},"required":["pageId","name"],"additionalProperties":false}}}),
        vec!["updates", "fileId"],
        false,
    );
    add(
        "create_artboard",
        "Create a top-level flex frame. Supply CSS width and height; use 1440x900 or 390x844 unless the brief says otherwise.",
        json!({"name":s,"styles":obj,"pageId":s,"x":{"type":"number"},"y":{"type":"number"}}),
        vec!["name", "styles", "fileId"],
        false,
    );
    add(
        "write_html",
        "Insert one visual group as static HTML with inline CSS, parsed like a browser would (named entities, unclosed <p>/<li>, <img> without a slash are fine). Pass html inline, or path (absolute, .html/.htm/.svg, up to 8 MiB) to import a large file without sending it through the chat. Unsupported tags/attributes are mapped or dropped and reported in warnings; scripts, event handlers and javascript: URLs are rejected. index counts every child, text included. Preserve existing nodes through move/duplicate instead of rewriting them.",
        json!({"html":s,"path":s,"targetNodeId":s,"mode":{"type":"string","enum":["insert-children","insert-before","insert-after","replace"]},"index":{"type":"integer","minimum":0}}),
        vec!["targetNodeId", "mode", "fileId"],
        false,
    );
    add(
        "update_styles",
        "Apply targeted CSS changes atomically. Each update targets nodeIds or a query (the same filters as find_nodes) so one call can restyle every match. Accepts CSS names or camelCase names; use var(--token) when a token exists. Reports matched and changed node counts.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeIds":ids,"query":query,"styles":obj},"required":["styles"],"additionalProperties":false}}}),
        vec!["updates", "fileId"],
        false,
    );
    add(
        "set_text_content",
        "Update leaf text nodes without changing their identifiers.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"textContent":s},"required":["nodeId","textContent"],"additionalProperties":false}}}),
        vec!["updates", "fileId"],
        false,
    );
    add(
        "rename_nodes",
        "Rename layers without replacing their contents.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"name":s},"required":["nodeId","name"],"additionalProperties":false}}}),
        vec!["updates", "fileId"],
        false,
    );
    add(
        "delete_nodes",
        "Delete nodeIds, or every node a query matches. Deleting an ancestor also removes its descendants.",
        json!({"nodeIds":ids,"query":query}),
        vec!["fileId"],
        false,
    );
    add(
        "duplicate_nodes",
        "Clone nodes with new stable identifiers; optionally supply parentId, pageId (copy to another page; parentId defaults to root there), index, x/y (artboards) and style overrides. Copies of a main component are plain layers; copies of instances stay linked.",
        json!({"nodes":{"type":"array","items":{"type":"object","properties":{"id":s,"parentId":s,"pageId":s,"index":{"type":"integer","minimum":0},"x":{"type":"number"},"y":{"type":"number"},"styles":{"type":"object","additionalProperties":{"type":"string"}}},"required":["id"],"additionalProperties":false}}}),
        vec!["nodes", "fileId"],
        false,
    );
    add(
        "move_nodes",
        "Reparent nodes while retaining identifiers. parentId root moves a node to the top level of its current page, or of pageId (moves whole artboards between pages); index controls sibling order.",
        json!({"moves":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"parentId":s,"pageId":s,"index":{"type":"integer","minimum":0},"x":{"type":"number"},"y":{"type":"number"}},"required":["nodeId","parentId"],"additionalProperties":false}}}),
        vec!["moves", "fileId"],
        false,
    );
    add(
        "get_tokens",
        "Read the token set as JSON, CSS or a Tailwind theme block.",
        json!({"format":{"type":"string","enum":["json","css","tailwind"]}}),
        vec!["fileId"],
        true,
    );
    add(
        "create_tokens",
        "Create CSS variables with names beginning --. Use named roles for colors, spacing and typography.",
        json!({"tokens":{"type":"array","items":{"type":"object","properties":{"type":s,"name":s,"value":{},"description":s},"required":["name","value"],"additionalProperties":false}}}),
        vec!["tokens", "fileId"],
        false,
    );
    add(
        "set_tokens",
        "Update, rename or delete CSS variables. Renames update exact var(--name) references.",
        json!({"tokens":{"type":"array","items":{"type":"object","properties":{"name":s,"newName":s,"value":{},"delete":{"type":"boolean"}},"required":["name"],"additionalProperties":false}}}),
        vec!["tokens", "fileId"],
        false,
    );
    add(
        "get_jsx",
        "Export one subtree as JSX with inline styles. Preserve values when adapting it to the project's framework. Long output is truncated with a marker.",
        json!({"nodeId":s,"format":{"type":"string","enum":["inline-styles"]},"maxChars":max}),
        vec!["nodeId", "fileId"],
        true,
    );
    add(
        "export_html",
        "Export ordinary HTML/CSS without editor attributes: one artboard or node (nodeId), a page (pageId), or by default the active page. Images and fonts from the asset store are inlined as data URIs (assets: inline, the default) or linked as file:// URLs on this machine (assets: files, much smaller). Long output is truncated with a marker.",
        json!({"nodeId":s,"pageId":s,"maxChars":max,"assets":{"type":"string","enum":["inline","files"]}}),
        vec!["fileId"],
        true,
    );
    add(
        "start_working_on_nodes",
        "Claim layers before a multi-step change. The editor marks them as being edited by you, and other agents get an error if they edit them, their ancestors' structure or anything inside them. Editing a layer claims it too. Claims last 2 minutes after your last edit in the file.",
        json!({"nodeIds":ids}),
        vec!["fileId", "nodeIds"],
        false,
    );
    add(
        "finish_working_on_nodes",
        "Release your claims on these layers (and layers inside them), or on all your layers in the file when nodeIds is omitted. Call it when you're done.",
        json!({"nodeIds":ids}),
        vec!["fileId"],
        false,
    );
    add(
        "undo",
        "Undo your most recent edit of a file; revisions stay monotonic. Fails if the latest edit was made by the person or another agent.",
        json!({}),
        vec![],
        false,
    );
    add(
        "redo",
        "Redo the most recently undone edit of a file.",
        json!({}),
        vec![],
        false,
    );
    add(
        "list_files",
        "List .cardboard files in the local library, most recent first, with their artboards. previews adds per-artboard HTML (large).",
        json!({"archived":{"type":"boolean"},"previews":{"type":"boolean"}}),
        vec![],
        true,
    );
    add(
        "close_file",
        "Close a file's tab after saving it.",
        json!({}),
        vec!["fileId"],
        false,
    );
    add(
        "show_dashboard",
        "Show the file dashboard instead of a file.",
        json!({}),
        vec![],
        false,
    );
    add(
        "rename_file",
        "Rename a file (open by fileId, or in the library by path); renames it on disk too.",
        json!({"path":s,"name":s}),
        vec!["name"],
        false,
    );
    add(
        "duplicate_file",
        "Copy a library file as \"Name copy\".",
        json!({"path":s}),
        vec![],
        false,
    );
    add(
        "archive_file",
        "Move a library file into the archive, closing it.",
        json!({"path":s}),
        vec![],
        false,
    );
    add(
        "restore_file",
        "Move an archived file back into the library.",
        json!({"path":s}),
        vec!["path"],
        false,
    );
    add(
        "delete_file",
        "Permanently delete an archived file.",
        json!({"path":s}),
        vec!["path"],
        false,
    );
    add(
        "get_document",
        "Read the document model as JSON: one node (nodeId), one page (pageId) or the whole file. depth cuts children below that level. Results over maxChars are refused with a hint; prefer get_tree_summary or find_nodes to locate things.",
        json!({"nodeId":s,"pageId":s,"depth":{"type":"integer","minimum":0},"maxChars":max}),
        vec![],
        true,
    );
    add(
        "select_nodes",
        "Set the user's selection. Not part of undo history and does not change the revision.",
        json!({"nodeIds":ids}),
        vec!["nodeIds"],
        false,
    );
    add(
        "set_active_page",
        "Switch the visible page. Not part of undo history and does not change the revision.",
        json!({"pageId":s}),
        vec!["pageId"],
        false,
    );
    add(
        "update_pages",
        "Rename pages or set their canvas background color (\"\" restores the default).",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"pageId":s,"name":s,"background":s},"required":["pageId"],"additionalProperties":false}}}),
        vec!["updates"],
        false,
    );
    add(
        "delete_pages",
        "Delete pages; a file keeps at least one.",
        json!({"pageIds":ids}),
        vec!["pageIds"],
        false,
    );
    add(
        "duplicate_page",
        "Copy a page with fresh node identifiers.",
        json!({"pageId":s}),
        vec!["pageId"],
        false,
    );
    add(
        "set_attributes",
        "Set or remove (\"\") HTML attributes such as src, alt or href on a nodeId or on every node a query matches.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"query":query,"attributes":obj},"required":["attributes"],"additionalProperties":false}}}),
        vec!["updates"],
        false,
    );
    add(
        "set_node_state",
        "Show, hide, lock or unlock layers.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"visible":{"type":"boolean"},"locked":{"type":"boolean"}},"required":["nodeId"],"additionalProperties":false}}}),
        vec!["updates"],
        false,
    );
    add(
        "set_positions",
        "Move top-level artboards on the canvas.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"x":{"type":"number"},"y":{"type":"number"}},"required":["nodeId","x","y"],"additionalProperties":false}}}),
        vec!["updates"],
        false,
    );
    add(
        "import_image",
        "Store an image in the library's asset store from exactly one of url, path (local file), dataUri, or stockId (from search_images), and return its src (\"/asset/<sha256>.<ext>\") for write_html, set_attributes or url() in update_styles. With parentId, also inserts an <img> sized from the image (width overrides the display width). Prefer this over base64 data URIs; those are moved into the store automatically anyway.",
        json!({"url":s,"path":s,"dataUri":s,"stockId":s,"parentId":s,"index":{"type":"integer","minimum":0},"alt":s,"name":s,"width":{"type":"number","exclusiveMinimum":0}}),
        vec![],
        false,
    );
    add(
        "search_images",
        "Search stock photos. Uses Unsplash when UNSPLASH_ACCESS_KEY is configured, otherwise Openverse (openly licensed, no key). Results include stockId, a preview src, size, alt text, photographer and the attribution to keep. Place one with import_image {stockId}.",
        json!({"query":s,"count":{"type":"integer","minimum":1,"maximum":30},"orientation":{"type":"string","enum":["landscape","portrait","squarish"]},"provider":{"type":"string","enum":["unsplash","openverse"]}}),
        vec!["query"],
        true,
    );
    add(
        "create_component",
        "Turn an existing layer into a main component. Then place copies with insert_instance: edits to the main reach every instance, and each instance keeps its own text, style and attribute overrides.",
        json!({"nodeId":s,"name":s}),
        vec!["nodeId", "fileId"],
        false,
    );
    add(
        "insert_instance",
        "Insert an instance of a component into parentId (\"root\" for a top-level frame on pageId) at index. Instance layers are real nodes with ids <instanceId>:<mainLayerId>; edit them with update_styles, set_text_content, set_attributes or set_node_state to override. Layers cannot be added, removed or moved inside an instance.",
        json!({"componentId":s,"parentId":s,"pageId":s,"index":{"type":"integer","minimum":0},"x":{"type":"number"},"y":{"type":"number"}}),
        vec!["componentId", "parentId", "fileId"],
        false,
    );
    add(
        "detach_instance",
        "Turn instances into plain layers that look the same and no longer follow the main.",
        json!({"nodeIds":ids}),
        vec!["nodeIds", "fileId"],
        false,
    );
    add(
        "reset_overrides",
        "Clear every override of the given instances so they match their main component again.",
        json!({"nodeIds":ids}),
        vec!["nodeIds", "fileId"],
        false,
    );
    add(
        "batch",
        "Run several editing tools on one file as a single atomic, undoable step. Read tools (get_*, find_nodes, export_html) may be mixed in and see the edits before them.",
        json!({"operations":{"type":"array","items":{"type":"object","properties":{"tool":s,"args":obj},"required":["tool"],"additionalProperties":false}}}),
        vec!["operations"],
        false,
    );
    add(
        "find_nodes",
        "Search a file, a page (pageId) or a subtree (nodeId) by text, layer name, tag and authored styles. All given filters must match. Returns ids with short info, up to limit (default 50).",
        {
            let mut props = query_props.clone();
            props["limit"] = json!({"type":"integer","minimum":1,"maximum":500});
            props
        },
        vec!["fileId"],
        true,
    );
    add(
        "screenshot",
        "Render an artboard or any node from the live document (unsaved edits included) in headless Chromium and return a PNG you can look at. scale defaults to 1 for artboards and 2 for nodes up to 800px; large images are scaled down, so screenshot a section for detail. path (absolute) also writes the PNG to disk. Needs Chromium, Chrome or Edge installed (or CARDBOARD_BROWSER).",
        json!({"nodeId":s,"scale":{"type":"number","minimum":0.1,"maximum":4},"path":s}),
        vec!["nodeId", "fileId"],
        true,
    );
    add(
        "get_bounds",
        "Measure rendered layout boxes (x, y, width, height in px relative to the artboard) for nodes and their descendants down to depth (default 1). Flags overflowX/overflowY when content is larger than its box, outsideParent when a box leaves its parent, and hidden. styles lists CSS properties to report as computed values. All nodeIds must share one artboard. Uses the same renderer as screenshot.",
        json!({"nodeIds":ids,"depth":{"type":"integer","minimum":0,"maximum":10},"styles":ids}),
        vec!["nodeIds", "fileId"],
        true,
    );
    add(
        "search_icons",
        "Search real icon sets (Lucide, Phosphor, Tabler, Heroicons, Material Symbols and more, via Iconify) instead of drawing SVG by hand. Returns names like lucide:arrow-right for insert_icon or <icon name=\"...\" /> in write_html. Narrow with sets, e.g. [\"lucide\"]; stick to one set per design.",
        json!({"query":s,"sets":ids,"limit":{"type":"integer","minimum":1,"maximum":200}}),
        vec!["query"],
        true,
    );
    add(
        "insert_icon",
        "Insert an icon from a real icon set as ONE layer (placement like write_html, default mode insert-children). size is px or a CSS length (default 24); strokeWidth works for outline sets (lucide, tabler); color defaults to currentColor so the icon follows the text color. Recolor later with update_styles color, resize with width/height. Inside write_html you can also write <icon name=\"lucide:check\" size=\"16\" stroke-width=\"1.5\" color=\"var(--accent)\" style=\"...\" />.",
        json!({"icon":s,"targetNodeId":s,"mode":{"type":"string","enum":["insert-children","insert-before","insert-after","replace"]},"index":{"type":"integer","minimum":0},"size":{"type":["number","string"]},"strokeWidth":{"type":["number","string"]},"color":s,"name":s}),
        vec!["icon", "targetNodeId", "fileId"],
        false,
    );
    add(
        "add_font",
        "Add a font family to the file so the canvas and exports render it on any machine. By default downloads it from Google Fonts by family name (weights default [\"400\",\"700\"]; italic adds italics; subsets default [\"latin\",\"latin-ext\"], or [\"all\"]). With path or dataUri, adds one local WOFF2/WOFF/TTF/OTF file as the given weight and style. Then use font-family: \"Family\", fallback.",
        json!({"family":s,"weights":{"type":"array","items":{"type":["string","integer"]}},"italic":{"type":"boolean"},"subsets":ids,"path":s,"dataUri":s,"weight":{"type":["string","integer"]},"style":{"type":"string","enum":["normal","italic"]}}),
        vec!["family", "fileId"],
        false,
    );
    add(
        "remove_font",
        "Remove a font family added with add_font from the file. Layers using it fall back to their next font.",
        json!({"family":s}),
        vec!["family", "fileId"],
        false,
    );
    add(
        "list_fonts",
        "List the file's own fonts (from add_font), generic CSS families, and the font families installed on this machine (optionally filtered by query). Installed fonts render only here; add_font makes a font travel with the file.",
        json!({"query":s}),
        vec![],
        true,
    );
    for tool in &mut tools {
        if matches!(
            tool["name"].as_str(),
            Some("import_image" | "search_images" | "add_font" | "search_icons" | "insert_icon")
        ) {
            tool["annotations"]["openWorldHint"] = json!(true);
        }
    }
    tools
}

// Assets: images (and fonts) imported into the library store.

/// What the network step of a tool needs from the store, copied out so the lock can be released.
pub struct Context {
    pub assets: Option<std::path::PathBuf>,
    pub unsplash_key: Option<String>,
}
pub fn context(store: &Store, args: &Value) -> Context {
    let file = match args.get("fileId") {
        Some(_) => store.file_id(args).ok(),
        None => store.active_file.clone(),
    };
    Context {
        assets: file
            .and_then(|id| store.assets_for(&id))
            .or_else(|| store.assets_dir()),
        unsplash_key: store.unsplash_key(),
    }
}
/// Whether a call must download something first. `Hub::call` does that without holding the store
/// lock (see `prepare`), so a slow network never stalls the editor.
pub fn needs_network(name: &str, args: &Value) -> bool {
    match name {
        "import_image" => args.get("url").is_some() || args.get("stockId").is_some(),
        // Without a local file or prepared faces, add_font downloads from Google Fonts.
        "add_font" => ["path", "dataUri", "faces"]
            .iter()
            .all(|k| args.get(*k).is_none()),
        _ => false,
    }
}
/// Do a tool's network work and return equivalent arguments that only refer to local assets.
pub fn prepare(name: &str, args: &Value, ctx: &Context) -> Result<Value> {
    let mut out = args.clone();
    let obj = out.as_object_mut().ok_or("Arguments must be an object")?;
    let store_dir = || {
        ctx.assets
            .clone()
            .ok_or("This session has no asset store; open a file saved in a library")
    };
    if name == "import_image" {
        if let Some(url) = obj.remove("url") {
            let bytes = assets::fetch(
                url.as_str().ok_or("url must be a string")?,
                &[],
                assets::MAX_BYTES,
            )?;
            obj.insert("asset".into(), json!(assets::put(&store_dir()?, &bytes)?));
        }
        if let Some(id) = obj.remove("stockId") {
            let chosen = stock::choose(
                id.as_str().ok_or("stockId must be a string")?,
                ctx.unsplash_key.as_deref(),
            )?;
            obj.insert("stock".into(), chosen.result);
            match (chosen.src, chosen.bytes) {
                (Some(src), _) => obj.insert("hotlink".into(), json!(src)),
                (None, Some(bytes)) => {
                    obj.insert("asset".into(), json!(assets::put(&store_dir()?, &bytes)?))
                }
                (None, None) => None,
            };
        }
    }
    if name == "add_font" {
        let family = string(args, "family")?;
        let list = |key: &str, default: &[&str]| -> Result<Vec<String>> {
            match args.get(key) {
                None => Ok(default.iter().map(|s| s.to_string()).collect()),
                Some(Value::Array(items)) => items.iter().map(css_value).collect(),
                Some(_) => Err(format!("{key} must be an array")),
            }
        };
        let faces = crate::fonts::google(
            &store_dir()?,
            family,
            &list("weights", &["400", "700"])?,
            bool_arg(args, "italic"),
            &list("subsets", &["latin", "latin-ext"])?,
        )?;
        for key in ["weights", "italic", "subsets"] {
            obj.remove(key);
        }
        obj.insert("faces".into(), json!(faces));
    }
    Ok(out)
}

fn add_font(store: &mut Store, args: &Value) -> Result<Value> {
    use crate::fonts::{self, FontFace};
    let id = store.file_id(args)?;
    let family = string(args, "family")?.trim().to_string();
    fonts::validate_family(&family)?;
    let dir = store
        .assets_for(&id)
        .ok_or("This session has no asset store; open a file saved in a library")?;
    let faces: Vec<FontFace> = match args.get("faces") {
        Some(faces) => serde_json::from_value(faces.clone()).map_err(|e| format!("faces: {e}"))?,
        None => {
            let name = assets::put(&dir, &assets::load(args)?)?;
            if !assets::is_font(name.rsplit('.').next().unwrap_or("")) {
                return Err("That file is an image, not a font (WOFF2, WOFF, TTF or OTF)".into());
            }
            vec![FontFace {
                family: family.clone(),
                src: assets::reference(&name),
                weight: args.get("weight").map_or(Ok("400".into()), css_value)?,
                style: args
                    .get("style")
                    .and_then(Value::as_str)
                    .unwrap_or("normal")
                    .into(),
                unicode_range: None,
                source: Some("file".into()),
            }]
        }
    };
    for face in &faces {
        let stored = assets::ref_name(&face.src).and_then(|n| assets::path(&dir, n));
        if face.family != family || !stored.is_some_and(|p| p.exists()) {
            return Err("Each face must belong to the family and point at a stored font".into());
        }
    }
    let count = faces.len();
    store.transaction(&id, |doc| {
        fonts::merge(&mut doc.fonts, faces);
        Ok(())
    })?;
    let doc = store.file(&id)?;
    Ok(json!({
        "family": family, "faces": count,
        "fonts": fonts::summary(&doc.fonts),
        "usage": format!("font-family: \"{family}\", system-ui, sans-serif"),
        "fileId": id, "revision": doc.revision,
    }))
}

fn list_fonts(store: &Store, args: &Value) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let file = match args.get("fileId") {
        Some(_) => Some(store.file_id(args)?),
        None => store.active_file.clone(),
    };
    let document = file
        .and_then(|id| store.file(&id).ok())
        .map(|doc| crate::fonts::summary(&doc.fonts))
        .unwrap_or_default();
    let installed: Vec<&String> = crate::fonts::installed()
        .iter()
        .filter(|f| f.to_lowercase().contains(&query))
        .collect();
    Ok(json!({
        "document": document,
        "installed": installed,
        "generic": ["system-ui", "ui-sans-serif", "ui-serif", "ui-monospace", "ui-rounded", "sans-serif", "serif", "monospace"],
    }))
}

fn import_image(store: &mut Store, args: &Value) -> Result<Value> {
    let parent = args.get("parentId").and_then(Value::as_str);
    let file = match (parent, args.get("fileId")) {
        (Some(_), _) | (None, Some(_)) => Some(store.file_id(args)?),
        _ => store.active_file.clone(),
    };
    let stock = args.get("stock").cloned().unwrap_or(json!({}));
    let mut result = json!({});
    let (src, size) = match args.get("hotlink").and_then(Value::as_str) {
        Some(url) => (
            url.to_string(),
            stock["width"].as_u64().zip(stock["height"].as_u64()),
        ),
        None => {
            let dir = file
                .as_deref()
                .and_then(|id| store.assets_for(id))
                .or_else(|| store.assets_dir())
                .ok_or("This session has no asset store; open a file saved in a library")?;
            let (name, bytes) = match args.get("asset").and_then(Value::as_str) {
                Some(name) => (
                    name.to_string(),
                    assets::read(&dir, name).ok_or("That asset is not in the store")?,
                ),
                None => {
                    let bytes = assets::load(args)?;
                    (assets::put(&dir, &bytes)?, bytes)
                }
            };
            if assets::is_font(name.rsplit('.').next().unwrap_or("")) {
                return Err("This file is a font, not an image".into());
            }
            result["asset"] = json!(name);
            result["bytes"] = json!(bytes.len());
            let size = assets::image_size(&bytes).map(|(w, h)| (w as u64, h as u64));
            (assets::reference(&name), size)
        }
    };
    result["src"] = json!(src);
    if let Some((w, h)) = size {
        result["width"] = json!(w);
        result["height"] = json!(h);
    }
    for key in [
        "provider",
        "attribution",
        "license",
        "photographer",
        "sourceUrl",
    ] {
        if stock
            .get(key)
            .is_some_and(|v| v.as_str().is_some_and(|v| !v.is_empty()))
        {
            result[key] = stock[key].clone();
        }
    }
    let Some(parent) = parent else {
        return Ok(result);
    };
    let id = file.expect("parentId resolves a file");
    let alt = args
        .get("alt")
        .and_then(Value::as_str)
        .or(stock["alt"].as_str())
        .unwrap_or("");
    let width = args.get("width").and_then(Value::as_f64);
    let style = match size {
        Some((w, h)) if w > 0 => {
            let display = width.unwrap_or((w as f64).min(800.0)).round();
            format!(
                "width:{display}px;height:{}px;object-fit:cover;flex-shrink:0",
                (display * h as f64 / w as f64).round()
            )
        }
        _ => format!(
            "width:{}px;height:auto;flex-shrink:0",
            width.unwrap_or(400.0).round()
        ),
    };
    let mut img = format!(
        "<img layer-name=\"{}\" src=\"{}\" alt=\"{}\" style=\"{style}\"",
        html::escape(args.get("name").and_then(Value::as_str).unwrap_or("Image")),
        html::escape(&src),
        html::escape(alt),
    );
    for (attr, key) in [
        ("data-attribution", "attribution"),
        ("data-source-url", "sourceUrl"),
        ("data-license", "license"),
    ] {
        if let Some(v) = stock[key].as_str().filter(|v| !v.is_empty()) {
            img.push_str(&format!(" {attr}=\"{}\"", html::escape(v)));
        }
    }
    img.push_str(" />");
    let mut write = json!({"targetNodeId":parent,"mode":"insert-children","html":img});
    if let Some(index) = args.get("index") {
        write["index"] = index.clone();
    }
    // Placing the image is a write_html into parentId, and other agents' claims apply to it.
    let actor = store.actor.clone();
    if let Some(actor) = &actor {
        let file = store.open(&id)?;
        presence::check(&file.doc, &file.claims, actor, "write_html", &write)?;
    }
    let inserted = store.transaction(&id, |doc| apply(doc, "write_html", &write))?;
    if let Some(actor) = &actor {
        let file = store.files.get_mut(&id).ok_or("File is not open")?;
        presence::record(
            &file.doc,
            &mut file.claims,
            actor,
            "write_html",
            &write,
            &inserted,
            vec![],
        );
        store.presence_changed(&id);
    }
    result["nodeId"] = inserted["nodeIds"][0].clone();
    result["fileId"] = json!(id);
    result["revision"] = json!(store.file(&id)?.revision);
    Ok(result)
}
