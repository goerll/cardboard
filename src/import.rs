//! Lenient HTML import: html5ever builds the same tree a browser would, then an allowlist turns
//! it into design nodes. Anything unsafe is an error; anything merely unsupported is mapped or
//! dropped with a warning so one odd tag doesn't fail a whole import.

use crate::document::{Document, Node, Result};
use crate::html::{is_void, parse_styles, split_declarations, validate_attribute, validate_tag};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, EndTag, StartTag, Tag, TagToken, Token, TokenSink, TokenSinkResult, Tokenizer,
    TokenizerOpts,
};
use html5ever::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink,
};
use html5ever::{Attribute, LocalName, QualName, TokenizerResult, local_name, ns};
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

/// Icon names (`<icon name="...">`) in an HTML fragment, read the way the importer reads them, so
/// they can be downloaded before the import runs.
pub fn icon_names(html: &str) -> Vec<String> {
    struct Names(RefCell<Vec<String>>);
    impl TokenSink for Names {
        type Handle = ();
        fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
            if let TagToken(tag) = token
                && tag.kind == StartTag
                && &*tag.name == "icon"
                && let Some(name) = tag.attrs.iter().find(|a| &*a.name.local == "name")
            {
                self.0.borrow_mut().push(name.value.to_string());
            }
            TokenSinkResult::Continue
        }
    }
    let tokenizer = Tokenizer::new(Names(RefCell::new(vec![])), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    while !matches!(tokenizer.feed(&input), TokenizerResult::Done) {}
    tokenizer.end();
    tokenizer.sink.0.take()
}

/// Inline imports go through the chat; file imports (`path`) may be larger.
pub const INLINE_LIMIT: usize = 1024 * 1024;
pub const FILE_LIMIT: usize = 8 * 1024 * 1024;

pub struct Import {
    pub nodes: Vec<Node>,
    pub warnings: Vec<String>,
}

pub fn parse(doc: &mut Document, html: &str) -> Result<Import> {
    let sink = Sink::new();
    let context = sink.create_element(
        QualName::new(None, ns!(html), local_name!("body")),
        vec![],
        ElementFlags::default(),
    );
    let builder = TreeBuilder::new_for_fragment(
        sink,
        context,
        None,
        TreeBuilderOpts {
            exact_errors: true,
            scripting_enabled: false,
            ..Default::default()
        },
    );
    let tokenizer = Tokenizer::new(
        SelfClosing(builder),
        TokenizerOpts {
            exact_errors: true,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    // feed pauses on scripts and <meta charset>; neither changes how we read a &str.
    while !matches!(tokenizer.feed(&input), TokenizerResult::Done) {}
    tokenizer.end();
    let sink = tokenizer.sink.0.sink;
    if let Some(error) = sink.fatal.into_inner() {
        return Err(error);
    }
    let tree = sink.tree.into_inner();
    // A fragment parse puts everything under a synthetic <html> root.
    let root = tree[0].children.first().copied().ok_or("Empty HTML tree")?;
    let mut import = Import {
        nodes: vec![],
        warnings: vec![],
    };
    let mut out = vec![];
    let state = &mut Convert {
        doc,
        tree: &tree,
        warnings: &mut import.warnings,
    };
    for &child in &tree[root].children {
        state.child(child, None, 0, &mut out)?;
    }
    import.nodes = nodes(out);
    Ok(import)
}

/// Spec-ignored `<div />` on a non-void HTML element is honoured as an empty element, which is
/// what agents mean by it and what this importer accepted before.
struct SelfClosing(TreeBuilder<Handle, Sink>);

impl TokenSink for SelfClosing {
    type Handle = Handle;
    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<Handle> {
        match token {
            TagToken(mut tag)
                if tag.kind == StartTag
                    && tag.self_closing
                    && !is_void(&tag.name)
                    && !matches!(&*tag.name, "svg" | "math")
                    && !self
                        .0
                        .adjusted_current_node_present_but_not_in_html_namespace() =>
            {
                tag.self_closing = false;
                let end = Tag {
                    kind: EndTag,
                    name: tag.name.clone(),
                    self_closing: false,
                    attrs: vec![],
                    had_duplicate_attributes: false,
                };
                let result = self.0.process_token(TagToken(tag), line);
                // Raw-text elements (<textarea/>, <title/>) switch the tokenizer; leave them be.
                if matches!(result, TokenSinkResult::Continue) {
                    let _ = self.0.process_token(TagToken(end), line);
                }
                result
            }
            token => self.0.process_token(token, line),
        }
    }
    fn end(&self) {
        self.0.end()
    }
    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.0
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// Element handles carry their immutable name so `elem_name` can borrow it; the tree shape
/// lives in an index arena.
#[derive(Clone)]
struct Handle(Rc<(usize, QualName)>);

enum Data {
    Root,
    Element(QualName, Vec<Attribute>),
    Text(String),
    Comment,
}
struct Slot {
    data: Data,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Sink {
    tree: RefCell<Vec<Slot>>,
    fatal: RefCell<Option<String>>,
}

impl Sink {
    fn new() -> Self {
        let sink = Sink {
            tree: RefCell::new(vec![]),
            fatal: RefCell::new(None),
        };
        sink.add(Data::Root);
        sink
    }
    fn add(&self, data: Data) -> usize {
        let mut tree = self.tree.borrow_mut();
        tree.push(Slot {
            data,
            parent: None,
            children: vec![],
        });
        tree.len() - 1
    }
    /// Handle for a non-element; the tree builder never asks for its name.
    fn handle(&self, id: usize) -> Handle {
        Handle(Rc::new((
            id,
            QualName::new(None, ns!(), LocalName::from("")),
        )))
    }
    fn detach(&self, id: usize) {
        let mut tree = self.tree.borrow_mut();
        if let Some(parent) = tree[id].parent.take() {
            tree[parent].children.retain(|&c| c != id);
        }
    }
    /// Insert under `parent` before `sibling` (or last), merging adjacent text like the DOM.
    fn place(&self, parent: usize, sibling: Option<usize>, child: NodeOrText<Handle>) {
        if let NodeOrText::AppendNode(h) = &child {
            self.detach(h.0.0);
        }
        let index = {
            let siblings = &self.tree.borrow()[parent].children;
            sibling
                .and_then(|s| siblings.iter().position(|&c| c == s))
                .unwrap_or(siblings.len())
        };
        let id = match child {
            NodeOrText::AppendNode(h) => h.0.0,
            NodeOrText::AppendText(text) => {
                let mut tree = self.tree.borrow_mut();
                if let Some(prev) = index.checked_sub(1).map(|i| tree[parent].children[i])
                    && let Data::Text(existing) = &mut tree[prev].data
                {
                    existing.push_str(&text);
                    return;
                }
                drop(tree);
                self.add(Data::Text(text.to_string()))
            }
        };
        let mut tree = self.tree.borrow_mut();
        tree[id].parent = Some(parent);
        tree[parent].children.insert(index, id);
    }
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a> = &'a QualName;

    fn finish(self) -> Self {
        self
    }
    fn parse_error(&self, msg: Cow<'static, str>) {
        // Browsers repair almost everything silently; what they can't tell us is whether a
        // fragment was cut off, so an element left open at the end is still an error.
        let fatal = if let Some(rest) = msg.strip_prefix("Unexpected open tag ") {
            let name = rest.trim_end_matches(" at end of body").rsplit(':').next();
            format!(
                "Unclosed <{}> at the end of the HTML. Close it explicitly; only p, li, dt, dd, option and table rows/cells may be left open.",
                name.unwrap_or("element")
            )
        } else if msg.contains("EOF") {
            "The HTML ends inside a tag, attribute or comment; it looks cut off.".into()
        } else {
            return;
        };
        self.fatal.borrow_mut().get_or_insert(fatal);
    }
    fn get_document(&self) -> Handle {
        self.handle(0)
    }
    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        &target.0.1
    }
    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, _: ElementFlags) -> Handle {
        Handle(Rc::new((
            self.add(Data::Element(name.clone(), attrs)),
            name,
        )))
    }
    fn create_comment(&self, _: StrTendril) -> Handle {
        self.handle(self.add(Data::Comment))
    }
    fn create_pi(&self, _: StrTendril, _: StrTendril) -> Handle {
        self.handle(self.add(Data::Comment))
    }
    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        self.place(parent.0.0, None, child);
    }
    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        if self.tree.borrow()[element.0.0].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }
    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}
    fn get_template_contents(&self, target: &Handle) -> Handle {
        target.clone()
    }
    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        x.0.0 == y.0.0
    }
    fn set_quirks_mode(&self, _: QuirksMode) {}
    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        let parent = self.tree.borrow()[sibling.0.0].parent;
        if let Some(parent) = parent {
            self.place(parent, Some(sibling.0.0), child);
        }
    }
    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        if let Data::Element(_, existing) = &mut self.tree.borrow_mut()[target.0.0].data {
            for attr in attrs {
                if !existing.iter().any(|a| a.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }
    fn remove_from_parent(&self, target: &Handle) {
        self.detach(target.0.0);
    }
    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let mut tree = self.tree.borrow_mut();
        let children = std::mem::take(&mut tree[node.0.0].children);
        for &c in &children {
            tree[c].parent = Some(new_parent.0.0);
        }
        tree[new_parent.0.0].children.extend(children);
    }
}

/// Text runs get a span layer (and id) as they are met, keeping ids in document order; a lone
/// text child collapses into its parent's text instead.
enum Child {
    Element(Node),
    Text(Node),
}

fn nodes(children: Vec<Child>) -> Vec<Node> {
    children
        .into_iter()
        .map(|(Child::Element(n) | Child::Text(n))| n)
        .collect()
}

enum Policy {
    Keep,
    Map(&'static str),
    Drop,
    Ignore,
}

/// Decide what an element becomes. Errors are reserved for active content.
fn policy(tag: &str, svg: bool) -> Result<Policy> {
    if validate_tag(tag).is_ok() {
        return Ok(Policy::Keep);
    }
    if svg {
        // Unknown SVG (filters, <use>, <animate>, <foreignObject>) has no safe generic stand-in.
        return Ok(Policy::Drop);
    }
    Ok(match tag {
        "script" | "iframe" | "frame" | "frameset" | "object" | "embed" | "applet" | "portal"
        | "base" => {
            return Err(format!(
                "<{tag}> is active content; Cardboard imports static HTML and SVG only"
            ));
        }
        // Document metadata from a pasted full page carries no design.
        "meta" | "title" | "head" => Policy::Ignore,
        "style" | "link" | "template" | "noscript" | "math" => Policy::Drop,
        "font" | "big" | "tt" | "nobr" | "bdi" | "bdo" | "ruby" | "rt" | "rp" | "acronym"
        | "strike" | "output" | "slot" => Policy::Map("span"),
        _ => Policy::Map("div"),
    })
}

struct Convert<'a> {
    doc: &'a mut Document,
    tree: &'a [Slot],
    warnings: &'a mut Vec<String>,
}

impl Convert<'_> {
    fn warn(&mut self, message: String) {
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    fn child(
        &mut self,
        id: usize,
        parent: Option<&str>,
        depth: usize,
        out: &mut Vec<Child>,
    ) -> Result<()> {
        match &self.tree[id].data {
            Data::Text(text) => {
                let keep_space = parent.is_some_and(|p| matches!(p, "pre" | "code" | "textarea"));
                if !text.is_empty() && (keep_space || !text.trim().is_empty()) {
                    if let Some(Child::Text(prev)) = out.last_mut() {
                        prev.text.push_str(text);
                    } else {
                        let mut span = Node::new(self.doc.allocate(), "span", "Text");
                        span.text = text.clone();
                        out.push(Child::Text(span));
                    }
                }
            }
            Data::Element(..) => {
                if let Some(node) = self.element(id, depth)? {
                    out.push(Child::Element(node));
                }
            }
            Data::Root | Data::Comment => {}
        }
        Ok(())
    }

    fn element(&mut self, id: usize, depth: usize) -> Result<Option<Node>> {
        if depth > 128 {
            return Err("HTML nesting limit is 128".into());
        }
        let Data::Element(name, attrs) = &self.tree[id].data else {
            return Ok(None);
        };
        let svg = name.ns == ns!(svg);
        let original = name.local.to_string();
        if !svg && original == "icon" {
            return self.icon(id, attrs).map(Some);
        }
        let tag = match policy(&original, svg)? {
            Policy::Keep => original.clone(),
            Policy::Map(tag) => {
                self.warn(format!(
                    "<{original}> is not supported; imported as <{tag}>"
                ));
                tag.into()
            }
            Policy::Ignore => return Ok(None),
            Policy::Drop => {
                self.warn(match original.as_str() {
                    "style" | "link" => format!(
                        "<{original}> was ignored; Cardboard applies inline style attributes only"
                    ),
                    _ => format!("<{original}> is not supported and was left out"),
                });
                return Ok(None);
            }
        };
        let mut node = Node::new(self.doc.allocate(), &tag, &original);
        for attr in attrs {
            let key = match &attr.name.prefix {
                // Namespace declarations are implied by the tree; drop them quietly.
                Some(p) if &**p == "xmlns" || &**p == "xml" => continue,
                Some(p) if !p.is_empty() => format!("{p}:{}", attr.name.local),
                _ => attr.name.local.to_string(),
            };
            let value = attr.value.to_string();
            match key.as_str() {
                "style" => match parse_styles(&value) {
                    Ok(styles) => node.styles = styles,
                    Err(_) => node.styles = self.lenient_styles(&value),
                },
                "layer-name" => node.name = value,
                "hidden" => node.visible = false,
                "data-cardboard-locked" => node.locked = true,
                _ => match validate_attribute(&key, &value) {
                    Ok(()) => {
                        node.attributes.insert(key, value);
                    }
                    Err(e) if e.starts_with("Unsupported attribute") => {
                        self.warn(format!("Unsupported attribute {key} was dropped"));
                    }
                    Err(e) => return Err(e),
                },
            }
        }
        if svg && tag == "svg" && node.attributes.contains_key("data-icon") {
            // An exported or copied icon layer comes back as one layer.
            match crate::icons::sanitize(&self.markup(id)) {
                Ok(markup) => {
                    node.markup = Some(markup);
                    return Ok(Some(node));
                }
                Err(e) => self.warn(format!("Icon markup was imported as plain SVG layers: {e}")),
            }
        }
        let mut children = vec![];
        for &child in &self.tree[id].children {
            self.child(child, Some(&tag), depth + 1, &mut children)?;
        }
        if let [Child::Text(span)] = children.as_mut_slice() {
            node.text = std::mem::take(&mut span.text);
        } else {
            node.children = nodes(children);
        }
        Ok(Some(node))
    }

    /// `<icon name="lucide:arrow-right" size="20" stroke-width="1.5" color="..." />` becomes one
    /// icon layer; `style`, `layer-name`, `hidden` and ordinary attributes apply on top.
    fn icon(&mut self, id: usize, attrs: &[Attribute]) -> Result<Node> {
        let has_content = self.tree[id]
            .children
            .iter()
            .any(|&c| match &self.tree[c].data {
                Data::Text(t) => !t.trim().is_empty(),
                Data::Element(..) => true,
                _ => false,
            });
        if has_content {
            return Err(
                "Write icons as <icon name=\"set:name\" />; they cannot contain content".into(),
            );
        }
        let get = |key: &str| {
            attrs
                .iter()
                .find(|a| &*a.name.local == key)
                .map(|a| a.value.to_string())
        };
        let name = get("name").ok_or("<icon> needs name=\"set:name\", e.g. lucide:arrow-right")?;
        let (size, stroke_width, color) = (get("size"), get("stroke-width"), get("color"));
        let options = crate::icons::Options {
            size: size.as_deref(),
            stroke_width: stroke_width.as_deref(),
            color: color.as_deref(),
        };
        let mut node = crate::icons::node(self.doc, &name, options)?;
        for attr in attrs {
            let (key, value) = (attr.name.local.to_string(), attr.value.to_string());
            match key.as_str() {
                "name" | "size" | "stroke-width" | "color" => {}
                "style" => node.styles.extend(match parse_styles(&value) {
                    Ok(styles) => styles,
                    Err(_) => self.lenient_styles(&value),
                }),
                "layer-name" => node.name = value,
                "hidden" => node.visible = false,
                "data-cardboard-locked" => node.locked = true,
                _ => match validate_attribute(&key, &value) {
                    Ok(()) => {
                        node.attributes.insert(key, value);
                    }
                    Err(e) if e.starts_with("Unsupported attribute") => {
                        self.warn(format!("Unsupported attribute {key} was dropped"));
                    }
                    Err(e) => return Err(e),
                },
            }
        }
        Ok(node)
    }

    /// The inner markup of an element as XML, for the icon sanitizer to re-check.
    fn markup(&self, id: usize) -> String {
        let mut out = String::new();
        for &child in &self.tree[id].children {
            match &self.tree[child].data {
                Data::Text(text) => out.push_str(&crate::html::escape(text)),
                Data::Element(name, attrs) => {
                    out.push('<');
                    out.push_str(&name.local);
                    for attr in attrs {
                        match &attr.name.prefix {
                            Some(p) if &**p == "xmlns" => continue,
                            Some(p) if !p.is_empty() => {
                                out.push_str(&format!(" {p}:{}", attr.name.local))
                            }
                            _ if &*attr.name.local == "xmlns" => continue,
                            _ => out.push_str(&format!(" {}", attr.name.local)),
                        }
                        out.push_str(&format!("=\"{}\"", crate::html::escape(&attr.value)));
                    }
                    out.push('>');
                    out.push_str(&self.markup(child));
                    out.push_str(&format!("</{}>", name.local));
                }
                Data::Root | Data::Comment => {}
            }
        }
        out
    }

    /// Keep the valid declarations of a style attribute that doesn't fully parse.
    fn lenient_styles(&mut self, value: &str) -> crate::document::Styles {
        let mut styles = crate::document::Styles::new();
        let Ok(entries) = split_declarations(value) else {
            self.warn(format!("Dropped unparseable style attribute `{value}`"));
            return styles;
        };
        for entry in entries {
            match parse_styles(entry) {
                Ok(parsed) => styles.extend(parsed),
                Err(_) => self.warn(format!(
                    "Dropped invalid CSS declaration `{}`",
                    entry.trim()
                )),
            }
        }
        styles
    }
}
