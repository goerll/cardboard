use crate::document::{Document, Node, Result, Styles};

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "img" | "br" | "hr" | "input" | "wbr" | "source" | "col"
    )
}
pub fn validate_tag(tag: &str) -> Result<()> {
    if matches!(
        tag,
        "div"
            | "span"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "button"
            | "a"
            | "img"
            | "br"
            | "hr"
            | "ul"
            | "ol"
            | "li"
            | "pre"
            | "code"
            | "strong"
            | "em"
            | "small"
            | "label"
            | "input"
            | "wbr"
            | "svg"
            | "path"
            | "circle"
            | "rect"
            | "line"
            | "polyline"
            | "polygon"
            | "g"
            | "ellipse"
            | "text"
            | "defs"
            | "linearGradient"
            | "stop"
            | "textarea"
            | "select"
            | "option"
            | "figure"
            | "figcaption"
            | "blockquote"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "td"
            | "th"
            | "b"
            | "i"
            | "u"
            | "s"
            | "sup"
            | "sub"
            | "mark"
            | "time"
            | "address"
            | "picture"
            | "source"
            | "radialGradient"
            | "clipPath"
            | "mask"
            | "tspan"
            | "dl"
            | "dt"
            | "dd"
            | "caption"
            | "tfoot"
            | "colgroup"
            | "col"
            | "hgroup"
            | "details"
            | "summary"
            | "abbr"
            | "cite"
            | "q"
            | "kbd"
            | "samp"
            | "var"
            | "del"
            | "ins"
            | "dfn"
            | "data"
            | "fieldset"
            | "legend"
            | "form"
            | "optgroup"
            | "meter"
            | "progress"
            | "search"
            | "menu"
    ) {
        Ok(())
    } else {
        Err(format!(
            "Unsupported element <{tag}>. Use static HTML or SVG design elements."
        ))
    }
}
pub fn validate_attribute(name: &str, value: &str) -> Result<()> {
    if name.to_ascii_lowercase().starts_with("on")
        || matches!(name, "style" | "srcdoc" | "data-cardboard-id")
    {
        return Err(format!("Attribute {name} is not editable"));
    }
    if !(matches!(
        name,
        "src"
            | "href"
            | "alt"
            | "title"
            | "class"
            | "viewBox"
            | "xmlns"
            | "d"
            | "fill"
            | "stroke"
            | "stroke-width"
            | "width"
            | "height"
            | "x"
            | "y"
            | "x1"
            | "x2"
            | "y1"
            | "y2"
            | "cx"
            | "cy"
            | "r"
            | "rx"
            | "ry"
            | "points"
            | "transform"
            | "offset"
            | "stop-color"
            | "stop-opacity"
            | "type"
            | "value"
            | "placeholder"
            | "role"
            | "target"
            | "rel"
            | "loading"
            | "srcset"
            | "sizes"
            | "media"
            | "colspan"
            | "rowspan"
            | "datetime"
            | "fx"
            | "fy"
            | "gradientUnits"
            | "gradientTransform"
            | "clip-path"
            | "mask"
            | "fill-rule"
            | "clip-rule"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-dasharray"
            | "opacity"
            | "fill-opacity"
            | "stroke-opacity"
            | "preserveAspectRatio"
            | "id"
            // SVG text and paint attributes, so <text>/<tspan> can be positioned and styled
            | "text-anchor"
            | "dominant-baseline"
            | "alignment-baseline"
            | "font-family"
            | "font-size"
            | "font-weight"
            | "font-style"
            | "letter-spacing"
            | "dx"
            | "dy"
            | "rotate"
            | "textLength"
            | "lengthAdjust"
            | "stroke-dashoffset"
            | "stroke-miterlimit"
            | "vector-effect"
            | "paint-order"
            | "pathLength"
            | "spreadMethod"
            | "color"
            | "visibility"
            | "clipPathUnits"
            | "maskUnits"
            | "maskContentUnits"
            // Form and list state that real-world HTML carries
            | "disabled"
            | "checked"
            | "selected"
            | "readonly"
            | "required"
            | "multiple"
            | "for"
            | "name"
            | "lang"
            | "dir"
            | "tabindex"
            | "open"
            | "min"
            | "max"
            | "low"
            | "high"
            | "optimum"
            | "step"
            | "maxlength"
            | "rows"
            | "cols"
            | "span"
            | "start"
            | "reversed"
            | "cite"
            | "scope"
            | "label"
    ) || name.starts_with("aria-")
        || name.starts_with("data-"))
    {
        return Err(format!("Unsupported attribute {name}"));
    }
    if matches!(name, "href" | "src" | "srcset") {
        let lower: String = value
            .chars()
            .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
            .collect::<String>()
            .to_lowercase();
        if lower.starts_with("javascript:")
            || lower.starts_with("vbscript:")
            || (lower.starts_with("data:") && !lower.starts_with("data:image/"))
        {
            return Err("Only image data URLs and ordinary asset URLs are supported".into());
        }
    }
    Ok(())
}
pub fn css_name(name: &str) -> String {
    if name.starts_with("--") {
        return name.into();
    }
    let mut result = String::new();
    for ch in name.chars() {
        if ch.is_ascii_uppercase() {
            result.push('-');
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}
pub fn validate_style(name: &str, value: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid CSS property name".into());
    }
    let lower = value.to_ascii_lowercase();
    if value.contains(['<', '>', '{', '}'])
        || lower.contains("javascript:")
        || lower.contains("expression(")
    {
        return Err("Invalid CSS value".into());
    }
    // One property per entry. Semicolons inside quotes/functions are valid.
    split_declarations(value).and_then(|v| {
        if v.len() > 1 {
            Err("CSS values cannot introduce extra declarations".into())
        } else {
            Ok(())
        }
    })
}
pub fn validate_token(name: &str, value: &str) -> Result<()> {
    if !name.starts_with("--") || name.len() < 3 {
        return Err("Token names must begin with --".into());
    }
    validate_style(name, value)
}
pub(crate) fn split_declarations(text: &str) -> Result<Vec<&str>> {
    let mut parts = vec![];
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("Unbalanced CSS value".into());
                }
            }
            ';' if depth == 0 => {
                if !text[start..i].trim().is_empty() {
                    parts.push(&text[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 || escaped {
        return Err("Unbalanced CSS value".into());
    }
    if !text[start..].trim().is_empty() {
        parts.push(&text[start..]);
    }
    Ok(parts)
}
pub fn parse_styles(text: &str) -> Result<Styles> {
    let mut result = Styles::new();
    for entry in split_declarations(text)? {
        let (name, value) = entry
            .split_once(':')
            .ok_or("Expected CSS property: value")?;
        let name = css_name(name.trim());
        let value = value.trim();
        validate_style(&name, value)?;
        result.insert(name, value.into());
    }
    Ok(result)
}
/// Import an HTML fragment, discarding warnings. See [`crate::import::parse`].
pub fn parse_fragment(doc: &mut Document, html: &str) -> Result<Vec<Node>> {
    if html.len() > crate::import::INLINE_LIMIT {
        return Err("HTML import limit is 1 MiB per call".into());
    }
    crate::import::parse(doc, html).map(|i| i.nodes)
}
pub fn node_html(node: &Node, editor: bool) -> String {
    let mut out = format!("<{}", node.tag);
    if editor {
        out.push_str(&format!(" data-cardboard-id=\"{}\"", escape(&node.id)));
    }
    for (k, v) in &node.attributes {
        out.push_str(&format!(" {k}=\"{}\"", escape(v)));
    }
    let mut styles = node
        .styles
        .iter()
        .map(|(k, v)| format!("{}:{v}", css_name(k)))
        .collect::<Vec<_>>()
        .join(";");
    if !node.visible {
        styles.push_str(";display:none");
    }
    if !styles.is_empty() {
        out.push_str(&format!(" style=\"{}\"", escape(&styles)));
    }
    out.push('>');
    if is_void(&node.tag) {
        return out;
    }
    out.push_str(&escape(&node.text));
    for child in &node.children {
        out.push_str(&node_html(child, editor));
    }
    out.push_str(&format!("</{}>", node.tag));
    out
}
pub fn document_html(doc: &Document, editor: bool) -> String {
    let page = doc.pages.iter().find(|p| p.id == doc.active_page);
    let nodes = page
        .map(|p| p.artboards.iter().collect())
        .unwrap_or_default();
    nodes_html(doc, nodes, editor)
}
/// A standalone HTML document (with the file's tokens) containing `nodes`.
pub fn nodes_html(doc: &Document, nodes: Vec<&Node>, editor: bool) -> String {
    let tokens = doc
        .tokens
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";");
    let body = nodes
        .into_iter()
        .map(|n| node_html(n, editor))
        .collect::<String>();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src https: http: data: file:; style-src 'unsafe-inline'; font-src https: data: file:; script-src 'none'; base-uri 'none'; form-action 'none'\"><style>:root{{{tokens}}}*{{box-sizing:border-box}}body{{margin:0}}button,input{{font:inherit}}</style></head><body>{body}</body></html>"
    )
}
