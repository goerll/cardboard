use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub use crate::store::Store;

pub type Result<T> = std::result::Result<T, String>;
pub type Styles = BTreeMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub text: String,
    pub attributes: BTreeMap<String, String>,
    pub styles: Styles,
    pub children: Vec<Node>,
    pub locked: bool,
    pub visible: bool,
    /// Canvas position of a top-level artboard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
    /// Sanitized inner SVG of an icon layer (see `icons`), rendered as this `<svg>`'s content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    /// This node is a main component; instances copy its subtree.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub component: bool,
    /// This node is an instance; its subtree is a materialized copy of the main (see components.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<Instance>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub component_id: String,
    /// Per-layer changes, keyed by the id of the corresponding layer in the main component.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Override>,
}

/// Properties an instance layer changes relative to its main. Empty strings in `styles` and
/// `attributes` mean the instance removed that declaration.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Override {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub styles: Styles,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

impl Node {
    pub fn new(id: String, tag: &str, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            tag: tag.into(),
            text: String::new(),
            attributes: BTreeMap::new(),
            styles: Styles::new(),
            children: vec![],
            locked: false,
            visible: true,
            position: None,
            markup: None,
            component: false,
            instance: None,
        }
    }
    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|n| n.find(id))
        }
    }
    pub fn find_mut(&mut self, id: &str) -> Option<&mut Node> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter_mut().find_map(|n| n.find_mut(id))
        }
    }
    pub fn contains(&self, id: &str) -> bool {
        self.find(id).is_some()
    }
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub name: String,
    pub artboards: Vec<Node>,
    /// Canvas color behind the artboards; the editor default applies when None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub next_id: u64,
    pub active_page: String,
    pub pages: Vec<Page>,
    pub tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub selection: Vec<String>,
    /// `@font-face` declarations for fonts kept in the asset store.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<crate::fonts::FontFace>,
}

impl Document {
    pub fn new(id: String, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            revision: 0,
            next_id: 2,
            active_page: "page-1".into(),
            pages: vec![Page {
                id: "page-1".into(),
                name: "Page 1".into(),
                artboards: vec![],
                background: None,
            }],
            tokens: BTreeMap::new(),
            selection: vec![],
            fonts: vec![],
        }
    }
    pub fn allocate(&mut self) -> String {
        let id = format!("node-{}", self.next_id);
        self.next_id += 1;
        id
    }
    pub fn node(&self, id: &str) -> Result<&Node> {
        self.pages
            .iter()
            .flat_map(|p| &p.artboards)
            .find_map(|n| n.find(id))
            .ok_or_else(|| format!("Node {id} does not exist in this file"))
    }
    pub fn node_mut(&mut self, id: &str) -> Result<&mut Node> {
        self.pages
            .iter_mut()
            .flat_map(|p| &mut p.artboards)
            .find_map(|n| n.find_mut(id))
            .ok_or_else(|| format!("Node {id} does not exist in this file"))
    }
    pub fn page_mut(&mut self, id: &str) -> Result<&mut Page> {
        self.pages
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("Page {id} does not exist"))
    }
    pub fn detach(&mut self, id: &str) -> Result<Node> {
        fn remove(nodes: &mut Vec<Node>, id: &str) -> Option<Node> {
            if let Some(i) = nodes.iter().position(|n| n.id == id) {
                return Some(nodes.remove(i));
            }
            nodes.iter_mut().find_map(|n| remove(&mut n.children, id))
        }
        self.pages
            .iter_mut()
            .find_map(|p| remove(&mut p.artboards, id))
            .ok_or_else(|| format!("Node {id} does not exist"))
    }
    pub fn replace(&mut self, id: &str, replacements: Vec<Node>) -> Result<()> {
        fn replace_in(nodes: &mut Vec<Node>, id: &str, replacement: &[Node]) -> bool {
            if let Some(i) = nodes.iter().position(|n| n.id == id) {
                nodes.splice(i..=i, replacement.iter().cloned());
                return true;
            }
            nodes
                .iter_mut()
                .any(|n| replace_in(&mut n.children, id, replacement))
        }
        if self
            .pages
            .iter_mut()
            .any(|p| replace_in(&mut p.artboards, id, &replacements))
        {
            Ok(())
        } else {
            Err(format!("Node {id} does not exist"))
        }
    }
    pub fn clone_node(&mut self, id: &str) -> Result<Node> {
        let mut node = self.node(id)?.clone();
        crate::components::rekey(self, &mut node);
        Ok(node)
    }
    pub fn validate(&self) -> Result<()> {
        if !self.pages.iter().any(|p| p.id == self.active_page) {
            return Err("Active page is missing".into());
        }
        let mut ids = BTreeSet::new();
        fn visit(n: &Node, ids: &mut BTreeSet<String>) -> Result<()> {
            if !ids.insert(n.id.clone()) {
                return Err("Duplicate node id".into());
            }
            crate::html::validate_tag(&n.tag)?;
            for (k, v) in &n.attributes {
                crate::html::validate_attribute(k, v)?;
            }
            for (k, v) in &n.styles {
                crate::html::validate_style(k, v)?;
            }
            if crate::html::is_void(&n.tag) && (!n.children.is_empty() || !n.text.is_empty()) {
                return Err("Void elements cannot contain content".into());
            }
            if let Some(markup) = &n.markup
                && (n.tag != "svg"
                    || !n.children.is_empty()
                    || !n.text.is_empty()
                    || crate::icons::sanitize(markup).as_ref() != Ok(markup))
            {
                return Err(
                    "Icon layers hold sanitized SVG only and cannot contain other content".into(),
                );
            }
            for child in &n.children {
                visit(child, ids)?;
            }
            Ok(())
        }
        let mut page_ids = BTreeSet::new();
        for p in &self.pages {
            if !page_ids.insert(&p.id) {
                return Err("Duplicate page id".into());
            }
            if let Some(bg) = &p.background {
                crate::html::validate_style("background-color", bg)?;
            }
            for n in &p.artboards {
                visit(n, &mut ids)?;
            }
        }
        for id in ids
            .iter()
            .map(String::as_str)
            .chain(page_ids.iter().map(|id| id.as_str()))
        {
            if let Some(number) = id
                .strip_prefix("node-")
                .or_else(|| id.strip_prefix("page-"))
                .and_then(|n| n.parse::<u64>().ok())
                && number >= self.next_id
            {
                return Err("Identifier counter must exceed existing identifiers".into());
            }
        }
        for (name, value) in &self.tokens {
            crate::html::validate_token(name, value)?;
        }
        for face in &self.fonts {
            crate::fonts::validate(face)?;
        }
        crate::components::validate(self)
    }
    pub fn info(&self) -> Value {
        json!({"fileId": self.id, "fileName": self.name, "revision": self.revision,
            "pageId": self.active_page, "pages": self.pages.iter().map(|p| json!({"id":p.id,"name":p.name,"isActive":p.id==self.active_page})).collect::<Vec<_>>(),
            "nodeCount": self.pages.iter().flat_map(|p| &p.artboards).map(Node::count).sum::<usize>(),
            "artboards": self.pages.iter().flat_map(|p| p.artboards.iter().map(move |n| json!({"id":n.id,"name":n.name,"pageId":p.id,"width":n.styles.get("width"),"height":n.styles.get("height")}))).collect::<Vec<_>>(),
            "tokens": self.tokens, "fonts": crate::fonts::summary(&self.fonts),
            "components": crate::components::summary(self)})
    }
}
