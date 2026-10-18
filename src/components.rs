//! Reusable components (see docs/components.md).
//!
//! A main component is an ordinary node with `component: true`. An instance is a node with
//! `instance: {componentId, overrides}` whose subtree is a materialized copy of the main: the
//! root keeps its own id and every inner layer gets the id `<instanceId>:<mainLayerId>`. Because
//! the copy is made of real nodes, rendering, export, JSX and every read tool work unchanged.
//!
//! After each edit, `reconcile` folds changes made to instance layers (through any ordinary tool)
//! into overrides, then rebuilds every instance from its main, so main edits reach all instances.
use crate::document::{Document, Instance, Node, Override, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Inner layers of an instance carry `<instanceId>:<key>` ids; only they contain a colon.
pub fn is_inner(id: &str) -> bool {
    id.contains(':')
}

/// Agents name an instance root the way its inner layers are named, `<instanceId>:<mainRootId>`.
/// No layer carries that id (the root is the bare instance id), so it resolves unambiguously.
pub fn root_alias(doc: &Document, id: &str) -> Option<String> {
    let (prefix, key) = id.rsplit_once(':')?;
    let i = doc.node(prefix).ok()?.instance.as_ref()?;
    (i.component_id == key && doc.node(id).is_err()).then(|| prefix.into())
}

fn walk<'a>(n: &'a Node, f: &mut impl FnMut(&'a Node)) {
    f(n);
    for c in &n.children {
        walk(c, f);
    }
}
fn all_nodes<'a>(doc: &'a Document, mut f: impl FnMut(&'a Node)) {
    for n in doc.pages.iter().flat_map(|p| &p.artboards) {
        walk(n, &mut f);
    }
}

/// Main ids and (instance id, component id) pairs for instances that are not inside another one.
fn scan(doc: &Document) -> (BTreeSet<String>, Vec<(String, String)>) {
    let (mut mains, mut instances) = (BTreeSet::new(), vec![]);
    all_nodes(doc, |n| {
        if n.component {
            mains.insert(n.id.clone());
        }
        if let Some(i) = &n.instance
            && !is_inner(&n.id)
        {
            instances.push((n.id.clone(), i.component_id.clone()));
        }
    });
    (mains, instances)
}

/// The mains as they were before an edit, the instances that already existed, and the state
/// the edit sets explicitly on instance layers.
pub struct Snapshot {
    mains: BTreeMap<String, Node>,
    instances: BTreeSet<String>,
    requested: BTreeMap<String, Override>,
}

pub fn snapshot(doc: &Document, tool: &str, args: &Value) -> Option<Snapshot> {
    let (mains, instances) = scan(doc);
    if mains.is_empty() && instances.is_empty() {
        return None;
    }
    Some(Snapshot {
        mains: mains
            .into_iter()
            .filter_map(|id| doc.node(&id).ok().map(|n| (id, n.clone())))
            .collect(),
        instances: instances.into_iter().map(|(id, _)| id).collect(),
        requested: requested(tool, args),
    })
}

/// Name, text, visibility and lock values an edit gives instance layers by id. Folding alone
/// can't see them when they equal the main's old value and the same edit changes the main.
fn requested(tool: &str, args: &Value) -> BTreeMap<String, Override> {
    let mut out = BTreeMap::<String, Override>::new();
    let updates = args.get("updates").and_then(Value::as_array);
    for u in updates.into_iter().flatten() {
        let Some(id) = u
            .get("nodeId")
            .and_then(Value::as_str)
            .filter(|id| is_inner(id))
        else {
            continue;
        };
        let text = |k| u.get(k).and_then(Value::as_str).map(String::from);
        let flag = |k| u.get(k).and_then(Value::as_bool);
        let o = out.entry(id.into()).or_default();
        match tool {
            "set_node_state" => {
                o.visible = flag("visible").or(o.visible);
                o.locked = flag("locked").or(o.locked);
            }
            "set_text_content" => o.text = text("textContent").or(o.text.take()),
            "rename_nodes" => o.name = text("name").or(o.name.take()),
            _ => {}
        }
    }
    out.retain(|_, o| !o.is_empty());
    out
}

impl Override {
    fn is_empty(&self) -> bool {
        *self == Override::default()
    }
    fn merge(&mut self, o: &Override) {
        self.name = o.name.clone().or(self.name.take());
        self.text = o.text.clone().or(self.text.take());
        self.visible = o.visible.or(self.visible);
        self.locked = o.locked.or(self.locked);
    }
    fn apply(&self, n: &mut Node) {
        if let Some(v) = &self.name {
            n.name = v.clone();
        }
        if let Some(v) = &self.text {
            n.text = v.clone();
        }
        if let Some(v) = self.visible {
            n.visible = v;
        }
        if let Some(v) = self.locked {
            n.locked = v;
        }
        // Only icon layers take markup, and stored markup is re-checked by `Document::validate`.
        if let Some(v) = &self.markup
            && n.markup.is_some()
        {
            n.markup = Some(v.clone());
        }
        for (map, patch) in [
            (&mut n.styles, &self.styles),
            (&mut n.attributes, &self.attributes),
        ] {
            for (k, v) in patch {
                if v.is_empty() {
                    map.remove(k);
                } else {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
    }
    /// What `n` changes relative to `main`. The instance root owns its name, lock and visibility.
    fn diff(n: &Node, main: &Node, root: bool) -> Self {
        fn changed<T: PartialEq + Clone>(a: &T, b: &T) -> Option<T> {
            (a != b).then(|| a.clone())
        }
        fn map_diff(
            a: &BTreeMap<String, String>,
            b: &BTreeMap<String, String>,
        ) -> BTreeMap<String, String> {
            let mut out = BTreeMap::new();
            for (k, v) in a {
                if b.get(k) != Some(v) {
                    out.insert(k.clone(), v.clone());
                }
            }
            for k in b.keys().filter(|k| !a.contains_key(*k)) {
                out.insert(k.clone(), String::new());
            }
            out
        }
        Override {
            name: (!root).then(|| changed(&n.name, &main.name)).flatten(),
            text: changed(&n.text, &main.text),
            styles: map_diff(&n.styles, &main.styles),
            attributes: map_diff(&n.attributes, &main.attributes),
            visible: (!root)
                .then(|| changed(&n.visible, &main.visible))
                .flatten(),
            locked: (!root).then(|| changed(&n.locked, &main.locked)).flatten(),
            markup: changed(&n.markup, &main.markup).flatten(),
        }
    }
}

/// A fresh copy of `main` as instance `id`, with overrides applied. The caller restores the
/// root's own fields (name, lock, visibility, position, instance).
pub fn expand(main: &Node, id: &str, overrides: &BTreeMap<String, Override>) -> Node {
    fn relabel(n: &mut Node, prefix: &str, overrides: &BTreeMap<String, Override>) {
        if let Some(o) = overrides.get(&n.id) {
            o.apply(n);
        }
        n.component = false;
        n.id = format!("{prefix}:{}", n.id);
        for c in &mut n.children {
            relabel(c, prefix, overrides);
        }
    }
    let mut tree = main.clone();
    relabel(&mut tree, id, overrides);
    tree.id = id.into();
    tree
}

/// Overrides that turn `main` into the instance `n`; None when the layer structure differs.
fn fold(n: &Node, main: &Node, prefix: &str) -> Option<BTreeMap<String, Override>> {
    fn visit(
        n: &Node,
        main: &Node,
        prefix: &str,
        root: bool,
        out: &mut BTreeMap<String, Override>,
    ) -> bool {
        let expected = if root {
            prefix.to_string()
        } else {
            format!("{prefix}:{}", main.id)
        };
        if n.id != expected || n.children.len() != main.children.len() || n.tag != main.tag {
            return false;
        }
        let o = Override::diff(n, main, root);
        if !o.is_empty() {
            out.insert(main.id.clone(), o);
        }
        n.children
            .iter()
            .zip(&main.children)
            .all(|(a, b)| visit(a, b, prefix, false, out))
    }
    let mut out = BTreeMap::new();
    visit(n, main, prefix, true, &mut out).then_some(out)
}

/// Rebuild instance `id` from `main`, keeping the root's own fields.
fn materialize(doc: &mut Document, id: &str, main: &Node) -> Result<()> {
    let node = doc.node_mut(id)?;
    let instance = node.instance.take();
    let overrides = instance.as_ref().map(|i| &i.overrides);
    let mut fresh = expand(main, id, overrides.unwrap_or(&BTreeMap::new()));
    fresh.name = std::mem::take(&mut node.name);
    fresh.locked = node.locked;
    fresh.visible = node.visible;
    fresh.position = node.position;
    fresh.instance = instance;
    *node = fresh;
    Ok(())
}

/// Give a copied subtree fresh ids. Copies of mains become plain layers; copied instances stay
/// linked, with their inner layers re-prefixed so overrides still line up.
pub fn rekey(doc: &mut Document, n: &mut Node) {
    fn reprefix(n: &mut Node, old: &str, new: &str) {
        if let Some(key) = n.id.strip_prefix(old).and_then(|k| k.strip_prefix(':')) {
            n.id = format!("{new}:{key}");
        }
        for c in &mut n.children {
            reprefix(c, old, new);
        }
    }
    let old = std::mem::replace(&mut n.id, doc.allocate());
    n.component = false;
    if n.instance.is_some() {
        let new = n.id.clone();
        for c in &mut n.children {
            reprefix(c, &old, &new);
        }
    } else {
        for c in &mut n.children {
            rekey(doc, c);
        }
    }
}

/// Turn an instance into plain layers that look exactly the same. Nested instances stay linked.
pub fn detach(doc: &mut Document, id: &str) -> Result<()> {
    let mut node = doc.node(id)?.clone();
    if node.instance.take().is_none() || is_inner(id) {
        return Err(format!("{id} is not an instance"));
    }
    for c in &mut node.children {
        rekey(doc, c);
    }
    // Nested instances now stand alone: everything the outer instance changed in them becomes
    // their own overrides.
    fn refold(doc: &Document, n: &mut Node) {
        if let Some(i) = &n.instance
            && let Ok(main) = doc.node(&i.component_id)
            && let Some(overrides) = fold(n, main, &n.id)
        {
            n.instance.as_mut().unwrap().overrides = overrides;
            return;
        }
        for c in &mut n.children {
            refold(doc, c);
        }
    }
    for c in &mut node.children {
        refold(doc, c);
    }
    *doc.node_mut(id)? = node;
    Ok(())
}

/// Mains ordered so that every main comes after the mains its subtree has instances of.
fn order(doc: &Document, mains: &BTreeSet<String>) -> Result<Vec<String>> {
    fn visit(
        doc: &Document,
        id: &str,
        done: &mut Vec<String>,
        active: &mut Vec<String>,
    ) -> Result<()> {
        if done.iter().any(|d| d == id) {
            return Ok(());
        }
        if active.iter().any(|a| a == id) {
            return Err("A component cannot contain an instance of itself".into());
        }
        active.push(id.into());
        let mut deps = vec![];
        walk(doc.node(id)?, &mut |n| {
            if let Some(i) = &n.instance
                && !is_inner(&n.id)
            {
                deps.push(i.component_id.clone());
            }
        });
        for dep in deps {
            visit(doc, &dep, done, active)?;
        }
        active.pop();
        done.push(id.into());
        Ok(())
    }
    let mut done = vec![];
    for id in mains {
        visit(doc, id, &mut done, &mut vec![])?;
    }
    Ok(done)
}

/// Drop override fields that only repeat what the main now says, unless the instance already
/// overrode them before this edit (then they stay pinned).
fn prune(
    overrides: &mut BTreeMap<String, Override>,
    old: &BTreeMap<String, Override>,
    main: &Node,
) {
    fn same<T: PartialEq>(v: &Option<T>, main: &T, pinned: bool) -> bool {
        !pinned && v.as_ref() == Some(main)
    }
    let mut layers = BTreeMap::new();
    walk(main, &mut |n| {
        layers.insert(n.id.as_str(), n);
    });
    let empty = Override::default();
    for (layer, o) in overrides.iter_mut() {
        let Some(m) = layers.get(layer.as_str()) else {
            continue;
        };
        let was = old.get(layer).unwrap_or(&empty);
        if same(&o.name, &m.name, was.name.is_some()) {
            o.name = None;
        }
        if same(&o.text, &m.text, was.text.is_some()) {
            o.text = None;
        }
        if same(&o.visible, &m.visible, was.visible.is_some()) {
            o.visible = None;
        }
        if same(&o.locked, &m.locked, was.locked.is_some()) {
            o.locked = None;
        }
        if o.markup.is_some() && o.markup == m.markup && was.markup.is_none() {
            o.markup = None;
        }
        for (patch, main, was) in [
            (&mut o.styles, &m.styles, &was.styles),
            (&mut o.attributes, &m.attributes, &was.attributes),
        ] {
            patch.retain(|k, v| {
                was.contains_key(k)
                    || match main.get(k) {
                        Some(m) => m != v,
                        None => !v.is_empty(),
                    }
            });
        }
    }
    overrides.retain(|_, o| !o.is_empty());
}

/// Bring every instance in line with its main after an edit (`before` from `snapshot`).
pub fn reconcile(doc: &mut Document, before: Option<&Snapshot>) -> Result<()> {
    let (mains, instances) = scan(doc);
    if mains.is_empty() && instances.is_empty() {
        return Ok(());
    }
    // Edits made to instance layers through ordinary tools become overrides.
    if let Some(before) = before {
        for (id, of) in &instances {
            let Some(main) = before.mains.get(of) else {
                continue;
            };
            if !before.instances.contains(id) {
                continue;
            }
            let node = doc.node(id)?;
            let Some(mut overrides) = fold(node, main, id) else {
                return Err(format!(
                    "\"{}\" ({id}) is an instance of component {of}, so layers cannot be added, removed or moved inside it. Edit the main component, hide layers with set_node_state, or detach_instance first",
                    node.name
                ));
            };
            let prefix = format!("{id}:");
            for (layer, o) in &before.requested {
                if let Some(key) = layer.strip_prefix(&prefix) {
                    overrides.entry(key.into()).or_default().merge(o);
                }
            }
            // A bulk edit (a query, or the same ids listed together) that gave the main and
            // its instances the same new value is a main edit, not an override to pin.
            if let (Some(i), Ok(now)) = (&node.instance, doc.node(of)) {
                prune(&mut overrides, &i.overrides, now);
            }
            node_instance(doc, id)?.overrides = overrides;
        }
    }
    // Instances of a deleted main keep their look as plain layers.
    let orphans: Vec<_> = instances
        .iter()
        .filter(|(_, of)| !mains.contains(of))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &orphans {
        detach(doc, id)?;
    }
    let (mains, instances) = if orphans.is_empty() {
        (mains, instances)
    } else {
        scan(doc)
    };
    for main_id in order(doc, &mains)? {
        let main = doc.node(&main_id)?.clone();
        for (id, _) in instances.iter().filter(|(_, of)| *of == main_id) {
            materialize(doc, id, &main)?;
        }
    }
    let valid: Vec<_> = doc
        .selection
        .iter()
        .filter(|id| doc.node(id).is_ok())
        .cloned()
        .collect();
    doc.selection = valid;
    Ok(())
}

fn node_instance<'a>(doc: &'a mut Document, id: &str) -> Result<&'a mut Instance> {
    doc.node_mut(id)?
        .instance
        .as_mut()
        .ok_or_else(|| format!("{id} is not an instance"))
}

/// Create a new instance of `component_id` (not yet inserted into the tree).
pub fn instantiate(doc: &mut Document, component_id: &str) -> Result<Node> {
    let main = doc.node(component_id)?;
    if !main.component {
        return Err(format!(
            "{component_id} is not a component; call create_component first"
        ));
    }
    let main = main.clone();
    let id = doc.allocate();
    let mut node = expand(&main, &id, &BTreeMap::new());
    node.instance = Some(Instance {
        component_id: component_id.into(),
        overrides: BTreeMap::new(),
    });
    Ok(node)
}

/// Drop every override of an instance.
pub fn reset(doc: &mut Document, id: &str) -> Result<()> {
    let of = node_instance(doc, id)?.component_id.clone();
    if is_inner(id) {
        return Err(format!("{id} is inside another instance; reset that one"));
    }
    node_instance(doc, id)?.overrides.clear();
    let main = doc.node(&of)?.clone();
    materialize(doc, id, &main)
}

/// Structural rules that must hold in any saved document.
pub fn validate(doc: &Document) -> Result<()> {
    let (mains, _) = scan(doc);
    let mut result = Ok(());
    all_nodes(doc, |n| {
        if result.is_err() {
            return;
        }
        if n.component && (n.instance.is_some() || is_inner(&n.id)) {
            result = Err(format!("{} cannot be a component", n.id));
        } else if let Some(i) = &n.instance
            && !is_inner(&n.id)
            && !mains.contains(&i.component_id)
        {
            result = Err(format!("Instance {} has no main component", n.id));
        }
    });
    result
}

/// Components in the file with how many instances each has, for get_basic_info.
pub fn summary(doc: &Document) -> Vec<serde_json::Value> {
    let (_, instances) = scan(doc);
    let mut out = vec![];
    for page in &doc.pages {
        for ab in &page.artboards {
            walk(ab, &mut |n| {
                if n.component {
                    out.push(
                        serde_json::json!({"componentId":n.id,"name":n.name,"pageId":page.id,
                        "instances":instances.iter().filter(|(_, of)| *of == n.id).count()}),
                    );
                }
            });
        }
    }
    out
}
