use crate::{
    document::{Document, Node, Result, Store, Styles},
    html,
};
use serde_json::{Value, json};

pub const INSTRUCTIONS: &str = "Cardboard is a local HTML/CSS design editor; the person sees your edits live and shares your undo history. Before anything else, read get_guide with topic cardboard-design. Start with get_basic_info and get_selection, and pass fileId and pageId explicitly. Post a short design plan before starting something new, add one visual group per write_html call, reuse layers with duplicate_nodes and move_nodes, and use the file's tokens. Tools report authored CSS, not pixels, so never claim a visual check you did not do. Call finish_working_on_nodes when done.";
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
fn insert(
    doc: &mut Document,
    parent: &str,
    page: &str,
    node: Node,
    index: Option<usize>,
) -> Result<()> {
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
        "list_files" => return Ok(json!({"files":store.list(bool_arg(args, "archived"))})),
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
        _ => {}
    }
    let id = store.file_id(args)?;
    if let Some(expected) = args.get("expectedRevision")
        && expected.as_u64() != Some(store.file(&id)?.revision)
    {
        return Err("Document revision changed; read the file again before editing".into());
    }
    let doc = store.file(&id)?;
    let read = match name {
        "get_document" => Some(json!({"document":doc})),
        "get_basic_info" => Some(doc.info()),
        "get_selection" => Some(
            json!({"nodes":doc.selection.iter().filter_map(|id| doc.node(id).ok().map(node_info)).collect::<Vec<_>>()}),
        ),
        "get_node_info" => Some(node_info(doc.node(string(args, "nodeId")?)?)),
        "get_children" => Some(
            json!({"children":doc.node(string(args,"nodeId")?)?.children.iter().map(node_info).collect::<Vec<_>>() }),
        ),
        "get_tree_summary" => {
            let mut text = String::new();
            tree(
                doc.node(string(args, "nodeId")?)?,
                args.get("depth")
                    .and_then(Value::as_u64)
                    .unwrap_or(3)
                    .min(10) as usize,
                0,
                &mut text,
            );
            Some(json!({"tree":text}))
        }
        "get_styles" => {
            let mut map = serde_json::Map::new();
            for id in array(args, "nodeIds")? {
                let id = id.as_str().ok_or("nodeIds must contain strings")?;
                map.insert(id.into(), json!(doc.node(id)?.styles));
            }
            Some(json!({"styles":map,"kind":"authored"}))
        }
        "get_tokens" => {
            let format = args.get("format").and_then(Value::as_str).unwrap_or("json");
            Some(match format {
                "json" => {
                    json!({"tokens":doc.tokens.iter().map(|(name,value)| json!({"name":name,"value":value})).collect::<Vec<_>>() })
                }
                "css" | "tailwind" => {
                    json!({"css":format!("{} {{\n{}\n}}",if format=="css" {":root"} else {"@theme"},doc.tokens.iter().map(|(k,v)|format!("  {k}: {v};")).collect::<Vec<_>>().join("\n"))})
                }
                _ => return Err("format must be json, css or tailwind".into()),
            })
        }
        "get_jsx" => {
            if args
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|f| f != "inline-styles")
            {
                return Err("This version exports inline-styles JSX; Tailwind conversion is not implemented yet".into());
            }
            Some(json!({"jsx":jsx(doc.node(string(args,"nodeId")?)?),"format":"inline-styles"}))
        }
        "export_html" => Some(json!({"html":html::document_html(doc,false)})),
        "finish_working_on_nodes" => Some(json!({"finished":true})),
        _ => None,
    };
    if let Some(mut value) = read {
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
                if !MUTATIONS.contains(&tool) {
                    return Err(format!("{tool} cannot run inside batch"));
                }
                let op_args = op.get("args").cloned().unwrap_or(json!({}));
                if op_args.get("fileId").is_some_and(|f| f != &json!(id)) {
                    return Err("Batch operations must target the batch's file".into());
                }
                results.push(mutate(doc, tool, &op_args).map_err(|e| format!("{tool}: {e}"))?);
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
            let nodes = html::parse_fragment(doc, string(args, "html")?)?;
            let ids = nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
            match string(args, "mode")? {
                "insert-children" => {
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
            Ok(json!({"nodeIds":ids}))
        }
        "update_styles" => {
            for update in array(args, "updates")? {
                let patch = styles(update.get("styles").ok_or("styles is required")?)?;
                for id in array(update, "nodeIds")? {
                    let node = doc.node_mut(id.as_str().ok_or("nodeIds must contain strings")?)?;
                    for (name, value) in &patch {
                        if value.is_empty() {
                            node.styles.remove(name);
                        } else {
                            node.styles.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
            Ok(json!({"updated":true}))
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
            for update in array(args, "updates")? {
                let node = doc.node_mut(string(update, "nodeId")?)?;
                for (k, v) in update
                    .get("attributes")
                    .and_then(Value::as_object)
                    .ok_or("attributes must be an object")?
                {
                    let v = v.as_str().ok_or("Attribute values must be strings")?;
                    if v.is_empty() {
                        node.attributes.remove(k);
                    } else {
                        html::validate_attribute(k, v)?;
                        node.attributes.insert(k.clone(), v.into());
                    }
                }
            }
            Ok(json!({"updated":true}))
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
            let ids = string_ids(args, "nodeIds")?;
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
            Ok(json!({"deleted":ids}))
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

use std::collections::BTreeMap;
pub fn definitions() -> Vec<Value> {
    let s = json!({"type":"string"});
    let ids = json!({"type":"array","items":s});
    let obj = json!({"type":"object"});
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
        "Read a compact subtree; cheaper than full code export.",
        json!({"nodeId":s,"depth":{"type":"integer","minimum":0,"maximum":10}}),
        vec!["nodeId", "fileId"],
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
        "Insert one visual group as static, explicitly closed HTML with inline CSS. Preserve existing nodes through move/duplicate instead of rewriting them. Scripts and event handlers are unsupported.",
        json!({"html":s,"targetNodeId":s,"mode":{"type":"string","enum":["insert-children","insert-before","insert-after","replace"]},"index":{"type":"integer","minimum":0}}),
        vec!["html", "targetNodeId", "mode", "fileId"],
        false,
    );
    add(
        "update_styles",
        "Apply targeted CSS changes atomically. Accepts CSS names or camelCase names; use var(--token) when a token exists.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeIds":ids,"styles":obj},"required":["nodeIds","styles"],"additionalProperties":false}}}),
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
        "Delete a set of nodes. Deleting an ancestor also removes its descendants.",
        json!({"nodeIds":ids}),
        vec!["nodeIds", "fileId"],
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
        "Export one subtree as JSX with inline styles. Preserve values when adapting it to the project's framework.",
        json!({"nodeId":s,"format":{"type":"string","enum":["inline-styles"]}}),
        vec!["nodeId", "fileId"],
        true,
    );
    add(
        "export_html",
        "Export the active page as ordinary HTML/CSS without editor attributes.",
        json!({}),
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
        "List .cardboard files in the local library, most recent first, with per-artboard preview HTML.",
        json!({"archived":{"type":"boolean"}}),
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
        "Read the complete document model as JSON.",
        json!({}),
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
        "Set or remove (\"\") HTML attributes such as src, alt or href.",
        json!({"updates":{"type":"array","items":{"type":"object","properties":{"nodeId":s,"attributes":obj},"required":["nodeId","attributes"],"additionalProperties":false}}}),
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
        "Run several editing tools on one file as a single atomic, undoable step.",
        json!({"operations":{"type":"array","items":{"type":"object","properties":{"tool":s,"args":obj},"required":["tool"],"additionalProperties":false}}}),
        vec!["operations"],
        false,
    );
    tools
}
