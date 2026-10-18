use crate::{
    document::{Document, Node, Result, Store, Styles},
    html,
};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "Cardboard is a local HTML/CSS design editor; the person sees your edits live and shares your undo history. Before anything else, read get_guide with topic cardboard-design. Start with get_basic_info and get_selection, and pass fileId and pageId explicitly. Post a short design plan before starting something new, add one visual group per write_html call, reuse layers with duplicate_nodes and move_nodes, and use the file's tokens. Most tools report authored CSS; use screenshot to see your work and get_bounds to measure it after each visual group, and only claim visual checks you actually looked at. Call finish_working_on_nodes when done.";
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
    json!({"id":n.id,"name":n.name,"componentType":n.tag,"textContent":n.text,"children":n.children.iter().map(|n| &n.id).collect::<Vec<_>>(),"visible":n.visible,"locked":n.locked,"styles":n.styles})
}
fn tree(n: &Node, depth: usize, prefix: usize, result: &mut String) {
    result.push_str(&format!(
        "{}{} {} [{}]\n",
        "  ".repeat(prefix),
        n.tag,
        n.name,
        n.id
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
fn html_source(args: &Value) -> Result<String> {
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
                    "nodeId" | "nodeIds" | "targetNodeId" | "parentId" => fix(v, "node-"),
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
        self.text.as_ref().is_none_or(|t| fold(&n.text).contains(t))
            && self.name.as_ref().is_none_or(|t| fold(&n.name).contains(t))
            && self
                .tag
                .as_ref()
                .is_none_or(|t| n.tag.eq_ignore_ascii_case(t))
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
    "finish_working_on_nodes",
];

/// Tools that change one document and therefore can run inside `batch`.
const MUTATIONS: &[&str] = &[
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
];

pub fn call(store: &mut Store, name: &str, args: &Value) -> Result<Value> {
    let mut args = args.clone();
    normalize_ids(&mut args);
    let args = &args;
    match name {
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
    if let Some(mut value) = read(doc, name, args)? {
        value["fileId"] = json!(id);
        value["revision"] = json!(doc.revision);
        return Ok(value);
    }
    let mut result = match name {
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
                    read(doc, tool, &op_args).map(Option::unwrap)
                } else if MUTATIONS.contains(&tool) {
                    mutate(doc, tool, &op_args)
                } else {
                    return Err(format!(
                        "{tool} cannot run inside batch: batch takes one file's edits and reads \
                         (get_* except get_bounds, find_nodes, export_html). Call file, undo, redo, screenshot and \
                         get_bounds on their own"
                    ));
                };
                results.push(result.map_err(|e| format!("{tool}: {e}"))?);
            }
            Ok(json!({"results":results}))
        })?,
        _ if MUTATIONS.contains(&name) => store.transaction(&id, |doc| mutate(doc, name, args))?,
        _ => return Err(format!("Unknown tool: {name}")),
    };
    result["fileId"] = json!(id);
    result["revision"] = json!(store.file(&id)?.revision);
    Ok(result)
}

/// Read tools, or `None` when `name` is not one.
fn read(doc: &Document, name: &str, args: &Value) -> Result<Option<Value>> {
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
            limited(
                json!({"html":html}),
                "html",
                args,
                "export one artboard or node with nodeId",
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
        "finish_working_on_nodes" => json!({"finished":true}),
        _ => return Ok(None),
    }))
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
            fn rekey(doc: &mut Document, n: &mut Node) {
                n.id = doc.allocate();
                for c in &mut n.children {
                    rekey(doc, c);
                }
            }
            for n in &mut page.artboards {
                rekey(doc, n);
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
                let page = page_for(doc, source)?;
                let explicit = spec
                    .get("parentId")
                    .and_then(Value::as_str)
                    .map(String::from);
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
                    let page_ref = doc.pages.iter().find(|p| p.id == page).unwrap();
                    let frames = crate::store::artboard_frames(page_ref);
                    if let Some(i) = page_ref.artboards.iter().position(|n| n.id == source) {
                        let (x, y, w, _) = frames[i];
                        let right = frames.iter().map(|f| f.0 + f.2).fold(x + w, f64::max);
                        node.position = Some([right + 80.0, y]);
                    }
                    if let (Some(x), Some(y)) = (
                        spec.get("x").and_then(Value::as_f64),
                        spec.get("y").and_then(Value::as_f64),
                    ) {
                        node.position = Some([x, y]);
                    }
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
                let page = page_for(doc, source)?;
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
                } else if let (Some(x), Some(y)) = (
                    movement.get("x").and_then(Value::as_f64),
                    movement.get("y").and_then(Value::as_f64),
                ) {
                    node.position = Some([x, y]);
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
        _ => Err(format!("Unknown tool: {name}")),
    }
}

use std::collections::{BTreeMap, BTreeSet};
pub fn definitions() -> Vec<Value> {
    let s = json!({"type":"string"});
    let ids = json!({"type":"array","items":s});
    let obj = json!({"type":"object"});
    let max = json!({"type":"integer","minimum":0,"description":"Output budget in characters (default 50000); 0 means no limit"});
    let query_props = json!({"pageId":s,"nodeId":{"type":"string","description":"Search only this node and its descendants"},"text":{"type":"string","description":"Case-insensitive substring of the node's own text"},"name":{"type":"string","description":"Case-insensitive substring of the layer name"},"tag":{"type":"string","description":"Exact tag, such as button or svg"},"styles":{"type":"object","description":"Property (CSS or camelCase, or * for any) to a test: a value for an exact match, true/false for present/absent, or {\"contains\": text}"}});
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
        "Inspect pages, artboards, dimensions and tokens before editing.",
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
        "Clone nodes with new stable identifiers; optionally supply parentId, index, x/y (artboards) and style overrides.",
        json!({"nodes":{"type":"array","items":{"type":"object","properties":{"id":s,"parentId":s,"index":{"type":"integer","minimum":0},"x":{"type":"number"},"y":{"type":"number"},"styles":{"type":"object","additionalProperties":{"type":"string"}}},"required":["id"],"additionalProperties":false}}}),
        vec!["nodes", "fileId"],
        false,
    );
    add(
        "move_nodes",
        "Reparent nodes while retaining identifiers. parentId root moves a node to its current page; index controls sibling order.",
        json!({"moves":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"parentId":s,"index":{"type":"integer","minimum":0},"x":{"type":"number"},"y":{"type":"number"}},"required":["nodeId","parentId"],"additionalProperties":false}}}),
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
        "Export ordinary HTML/CSS without editor attributes: one artboard or node (nodeId), a page (pageId), or by default the active page. Long output is truncated with a marker.",
        json!({"nodeId":s,"pageId":s,"maxChars":max}),
        vec!["fileId"],
        true,
    );
    add(
        "finish_working_on_nodes",
        "Finish an editing session. This first version has no presence markers to release.",
        json!({"nodeIds":ids}),
        vec!["fileId"],
        true,
    );
    add(
        "undo",
        "Undo the most recent edit of a file; revisions stay monotonic.",
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
    tools
}
