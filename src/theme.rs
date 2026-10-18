//! Token modes, the stock shadcn seed and shadcn/tweakcn CSS import and export.
//!
//! `Document.tokens` holds the base mode, called "light". Every other mode stores only the
//! values it overrides in `Document.modes`. Renderers declare the fully resolved set for each
//! mode, because a token that references another (`--sidebar: var(--card)`) is resolved where
//! it is declared and would otherwise keep the base value inside a dark artboard.
use crate::{
    document::{Document, Result},
    html,
};
use std::collections::BTreeMap;

pub type Tokens = BTreeMap<String, String>;
pub const BASE_MODE: &str = "light";

pub fn validate_mode(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 32
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Mode {name:?} must be lowercase letters, digits and dashes, starting with a letter"
        ))
    }
}

/// "light" first, then the named modes.
pub fn mode_names(doc: &Document) -> Vec<String> {
    std::iter::once(BASE_MODE.to_string())
        .chain(doc.modes.keys().cloned())
        .collect()
}

/// Every token's value in `mode`: the base set with the mode's overrides on top.
pub fn resolved(doc: &Document, mode: &str) -> Tokens {
    let mut tokens = doc.tokens.clone();
    if let Some(overrides) = doc.modes.get(mode) {
        tokens.extend(overrides.clone());
    }
    tokens
}

fn block(selector: &str, tokens: &Tokens, pretty: bool) -> String {
    if pretty {
        let body = order(tokens)
            .into_iter()
            .map(|(k, v)| format!("  {k}: {v};\n"))
            .collect::<String>();
        format!("{selector} {{\n{body}}}\n")
    } else {
        let body = tokens
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect::<Vec<_>>()
            .join(";");
        format!("{selector}{{{body}}}")
    }
}

/// The stylesheet the canvas and export_html use: `:root` holds the document's default mode,
/// and `[data-mode="…"]` re-declares the whole set so any artboard or layer can switch modes.
/// Mirrored in `ui/src/lib/tokens.ts`.
pub fn tokens_css(doc: &Document, pretty: bool) -> String {
    let default = doc.default_mode.as_deref().unwrap_or(BASE_MODE);
    let mut css = block(":root", &resolved(doc, default), pretty);
    if !doc.modes.is_empty() {
        for mode in mode_names(doc) {
            css.push_str(&block(
                &format!("[data-mode=\"{mode}\"]"),
                &resolved(doc, &mode),
                pretty,
            ));
        }
    }
    css
}

/// shadcn's variable contract in its documented order; anything else follows alphabetically.
const SHADCN_ORDER: &[&str] = &[
    "--radius",
    "--background",
    "--foreground",
    "--card",
    "--card-foreground",
    "--popover",
    "--popover-foreground",
    "--primary",
    "--primary-foreground",
    "--secondary",
    "--secondary-foreground",
    "--muted",
    "--muted-foreground",
    "--accent",
    "--accent-foreground",
    "--destructive",
    "--destructive-foreground",
    "--border",
    "--input",
    "--ring",
    "--chart-1",
    "--chart-2",
    "--chart-3",
    "--chart-4",
    "--chart-5",
    "--sidebar",
    "--sidebar-foreground",
    "--sidebar-primary",
    "--sidebar-primary-foreground",
    "--sidebar-accent",
    "--sidebar-accent-foreground",
    "--sidebar-border",
    "--sidebar-ring",
    "--font-sans",
    "--font-serif",
    "--font-mono",
    "--shadow-2xs",
    "--shadow-xs",
    "--shadow-sm",
    "--shadow",
    "--shadow-md",
    "--shadow-lg",
    "--shadow-xl",
    "--shadow-2xl",
    "--tracking-normal",
    "--spacing",
];

fn order(tokens: &Tokens) -> Vec<(&String, &String)> {
    let rank = |k: &str| {
        SHADCN_ORDER
            .iter()
            .position(|o| *o == k)
            .unwrap_or(SHADCN_ORDER.len())
    };
    let mut items: Vec<_> = tokens.iter().collect();
    items.sort_by_key(|(k, _)| (rank(k), (*k).clone()));
    items
}

pub fn is_color(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    v.starts_with('#')
        || v == "transparent"
        || v == "currentcolor"
        || [
            "rgb(",
            "rgba(",
            "hsl(",
            "hsla(",
            "hwb(",
            "oklch(",
            "oklab(",
            "lab(",
            "lch(",
            "color(",
            "color-mix(",
        ]
        .iter()
        .any(|p| v.starts_with(p))
}

/// Tailwind v4 namespaces that already name a theme variable, so `@theme inline` maps them as is.
const THEME_NAMESPACES: &[&str] = &[
    "--color-",
    "--font-",
    "--text-",
    "--font-weight-",
    "--leading-",
    "--tracking-",
    "--spacing",
    "--radius-",
    "--shadow-",
    "--breakpoint-",
    "--container-",
];

/// What `npx shadcn` and tweakcn accept: `:root`, `.dark` (and `.<mode>` for other modes),
/// then `@theme inline` mapping the variables onto Tailwind v4 utilities.
pub fn shadcn_css(doc: &Document) -> String {
    let mut css = block(":root", &doc.tokens, true);
    for mode in doc.modes.keys() {
        css.push('\n');
        css.push_str(&block(&format!(".{mode}"), &resolved(doc, mode), true));
    }
    let all = mode_names(doc);
    let mut theme = vec![];
    for (name, _) in order(&doc.tokens) {
        if THEME_NAMESPACES.iter().any(|p| name.starts_with(p)) {
            theme.push(format!("  {name}: var({name});"));
        } else if name == "--radius" {
            for (size, calc) in [
                ("sm", "calc(var(--radius) * 0.6)"),
                ("md", "calc(var(--radius) * 0.8)"),
                ("lg", "var(--radius)"),
                ("xl", "calc(var(--radius) * 1.4)"),
            ] {
                theme.push(format!("  --radius-{size}: {calc};"));
            }
        } else if all
            .iter()
            .any(|m| resolved(doc, m).get(name).is_some_and(|v| is_color(v)))
        {
            theme.push(format!("  --color-{}: var({name});", &name[2..]));
        }
    }
    if !theme.is_empty() {
        css.push_str(&format!("\n@theme inline {{\n{}\n}}\n", theme.join("\n")));
    }
    css
}

/// Which mode a selector such as `:root`, `.dark`, `html.dark` or `[data-mode=dim]` declares.
fn selector_mode(selector: &str) -> Option<String> {
    for s in selector.split(',').map(str::trim) {
        let s = s
            .trim_start_matches(":root")
            .trim_start_matches("html")
            .trim_start_matches(":host");
        let mode = if s.is_empty() {
            BASE_MODE.to_string()
        } else if let Some(class) = s.strip_prefix('.') {
            class.to_string()
        } else if let Some(attr) = s.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
            let (key, value) = attr.split_once('=')?;
            if !matches!(key.trim(), "data-mode" | "data-theme" | "class") {
                continue;
            }
            value.trim().trim_matches(['"', '\'']).to_string()
        } else {
            continue;
        };
        if validate_mode(&mode).is_ok() {
            return Some(mode);
        }
    }
    None
}

/// Read custom properties from pasted shadcn/tweakcn CSS, grouped by mode. `:root` is light,
/// `.dark` is dark; `@theme` mappings and ordinary rules are ignored.
pub fn parse_css(text: &str) -> Result<BTreeMap<String, Tokens>> {
    if text.len() > 256 * 1024 {
        return Err("Theme CSS is limited to 256 KB".into());
    }
    let mut result: BTreeMap<String, Tokens> = BTreeMap::new();
    let mut stack: Vec<String> = vec![];
    let mut buf = String::new();
    let (mut quote, mut parens) = (None, 0i32);
    let mut chars = text.chars().peekable();
    let mut take = |stack: &[String], decl: &str| -> Result<()> {
        let Some((name, value)) = decl.split_once(':') else {
            return Ok(());
        };
        let (name, value) = (name.trim(), value.trim());
        if !name.starts_with("--") || stack.iter().any(|s| s.starts_with("@theme")) {
            return Ok(());
        }
        let mode = match stack.last().map(String::as_str) {
            Some(s) if s.starts_with("@media") && s.contains("dark") => Some("dark".into()),
            Some(s) => selector_mode(s).map(|m| {
                // `@media (prefers-color-scheme: dark) { :root { … } }`
                if m == BASE_MODE
                    && stack
                        .iter()
                        .any(|s| s.starts_with("@media") && s.contains("dark"))
                {
                    "dark".into()
                } else {
                    m
                }
            }),
            None => None,
        };
        if let Some(mode) = mode {
            html::validate_token(name, value).map_err(|e| format!("{name}: {e}"))?;
            result
                .entry(mode)
                .or_default()
                .insert(name.into(), value.into());
        }
        Ok(())
    };
    while let Some(c) = chars.next() {
        if quote.is_none() && c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = ' ';
            for c in chars.by_ref() {
                if prev == '*' && c == '/' {
                    break;
                }
                prev = c;
            }
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            buf.push(c);
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                buf.push(c);
            }
            '(' => {
                parens += 1;
                buf.push(c);
            }
            ')' => {
                parens -= 1;
                buf.push(c);
            }
            '{' if parens == 0 => {
                stack.push(buf.trim().to_string());
                buf.clear();
            }
            ';' if parens == 0 => {
                take(&stack, &buf)?;
                buf.clear();
            }
            '}' if parens == 0 => {
                take(&stack, &buf)?;
                buf.clear();
                stack.pop().ok_or("Unbalanced braces in theme CSS")?;
            }
            _ => buf.push(c),
        }
    }
    if !stack.is_empty() || quote.is_some() || parens != 0 {
        return Err("Unbalanced theme CSS".into());
    }
    if result.is_empty() {
        return Err("No custom properties found in :root, .dark or [data-mode] blocks".into());
    }
    Ok(result)
}

/// Merge mode-grouped tokens into the document. Light values become the base; other modes keep
/// only the values that differ from it. A token that only a non-light mode defines also seeds
/// the base so it exists everywhere.
pub fn apply(doc: &mut Document, sets: BTreeMap<String, Tokens>, replace: bool) -> Result<()> {
    if replace {
        doc.tokens.clear();
        doc.modes.clear();
        doc.default_mode = None;
    }
    for mode in sets.keys() {
        validate_mode(mode)?;
    }
    if let Some(light) = sets.get(BASE_MODE) {
        doc.tokens.extend(light.clone());
    }
    for (mode, tokens) in sets.iter().filter(|(m, _)| *m != BASE_MODE) {
        for (name, value) in tokens {
            let base = doc.tokens.entry(name.clone()).or_insert(value.clone());
            let overrides = doc.modes.entry(mode.clone()).or_default();
            if base == value {
                overrides.remove(name);
            } else {
                overrides.insert(name.clone(), value.clone());
            }
        }
    }
    doc.modes.retain(|_, o| !o.is_empty());
    Ok(())
}

// ───────────────────────────── seed ─────────────────────────────

const GEIST: &str = "Geist, ui-sans-serif, system-ui, sans-serif";
const GEIST_MONO: &str = "\"Geist Mono\", ui-monospace, monospace";

/// The knobs shadcn's contract lacks but real systems differ on, with neutral defaults derived
/// from shadcn's own tokens, so any theme (preset, pasted CSS or direct values) can carry them.
pub fn extension_defaults() -> Tokens {
    [
        ("--font-display", "var(--font-sans)"),
        ("--font-figures", "var(--font-sans)"),
        ("--display-weight", "600"),
        ("--display-tracking", "-0.025em"),
        ("--radius-control", "calc(var(--radius) - 2px)"),
        ("--radius-field", "calc(var(--radius) - 2px)"),
        ("--radius-chip", "calc(var(--radius) - 4px)"),
        ("--card-ring", "var(--border)"),
        ("--card-shadow", "var(--shadow-sm)"),
        ("--field-bg", "transparent"),
        ("--field-border", "var(--input)"),
        ("--signal", "var(--primary)"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Give a partial theme every token of the shadcn contract and the extensions, from the stock
/// shadcn preset, keeping every value the theme defines. A filled token also gets the preset's
/// dark value when the file has a dark mode, so pasting only a few colors never leaves the kit
/// with blank swatches or an empty chart.
pub fn fill_defaults(doc: &mut Document) {
    let preset = shadcn_preset();
    let dark = &preset["dark"];
    for (name, value) in &preset[BASE_MODE] {
        if doc.tokens.contains_key(name) {
            continue;
        }
        doc.tokens.insert(name.clone(), value.clone());
        if let Some(overrides) = doc.modes.get_mut("dark")
            && let Some(v) = dark.get(name).filter(|v| *v != value)
        {
            overrides.insert(name.clone(), v.clone());
        }
    }
}

/// shadcn/ui's stock "neutral" theme (Tailwind v4, oklch) plus the extension tokens.
fn shadcn_preset() -> BTreeMap<String, Tokens> {
    let pairs = [
        ("--background", "oklch(1 0 0)", "oklch(0.145 0 0)"),
        ("--foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
        ("--card", "oklch(1 0 0)", "oklch(0.205 0 0)"),
        ("--card-foreground", "oklch(0.145 0 0)", "oklch(0.985 0 0)"),
        ("--popover", "oklch(1 0 0)", "oklch(0.205 0 0)"),
        (
            "--popover-foreground",
            "oklch(0.145 0 0)",
            "oklch(0.985 0 0)",
        ),
        ("--primary", "oklch(0.205 0 0)", "oklch(0.922 0 0)"),
        (
            "--primary-foreground",
            "oklch(0.985 0 0)",
            "oklch(0.205 0 0)",
        ),
        ("--secondary", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        (
            "--secondary-foreground",
            "oklch(0.205 0 0)",
            "oklch(0.985 0 0)",
        ),
        ("--muted", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        ("--muted-foreground", "oklch(0.556 0 0)", "oklch(0.708 0 0)"),
        ("--accent", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        (
            "--accent-foreground",
            "oklch(0.205 0 0)",
            "oklch(0.985 0 0)",
        ),
        (
            "--destructive",
            "oklch(0.577 0.245 27.325)",
            "oklch(0.704 0.191 22.216)",
        ),
        ("--border", "oklch(0.922 0 0)", "oklch(1 0 0 / 10%)"),
        ("--input", "oklch(0.922 0 0)", "oklch(1 0 0 / 15%)"),
        ("--ring", "oklch(0.708 0 0)", "oklch(0.556 0 0)"),
        (
            "--chart-1",
            "oklch(0.646 0.222 41.116)",
            "oklch(0.488 0.243 264.376)",
        ),
        (
            "--chart-2",
            "oklch(0.6 0.118 184.704)",
            "oklch(0.696 0.17 162.48)",
        ),
        (
            "--chart-3",
            "oklch(0.398 0.07 227.392)",
            "oklch(0.769 0.188 70.08)",
        ),
        (
            "--chart-4",
            "oklch(0.828 0.189 84.429)",
            "oklch(0.627 0.265 303.9)",
        ),
        (
            "--chart-5",
            "oklch(0.769 0.188 70.08)",
            "oklch(0.645 0.246 16.439)",
        ),
        ("--radius", "0.625rem", "0.625rem"),
        ("--sidebar", "oklch(0.985 0 0)", "oklch(0.205 0 0)"),
        (
            "--sidebar-foreground",
            "oklch(0.145 0 0)",
            "oklch(0.985 0 0)",
        ),
        (
            "--sidebar-primary",
            "oklch(0.205 0 0)",
            "oklch(0.488 0.243 264.376)",
        ),
        (
            "--sidebar-primary-foreground",
            "oklch(0.985 0 0)",
            "oklch(0.985 0 0)",
        ),
        ("--sidebar-accent", "oklch(0.97 0 0)", "oklch(0.269 0 0)"),
        (
            "--sidebar-accent-foreground",
            "oklch(0.205 0 0)",
            "oklch(0.985 0 0)",
        ),
        ("--sidebar-border", "oklch(0.922 0 0)", "oklch(1 0 0 / 10%)"),
        ("--sidebar-ring", "oklch(0.708 0 0)", "oklch(0.556 0 0)"),
        ("--font-sans", GEIST, GEIST),
        ("--font-mono", GEIST_MONO, GEIST_MONO),
        (
            "--shadow-sm",
            "0 1px 2px rgba(0,0,0,0.05)",
            "0 1px 2px rgba(0,0,0,0.3)",
        ),
        (
            "--shadow-md",
            "0 2px 4px -1px rgba(0,0,0,0.06), 0 4px 12px -2px rgba(0,0,0,0.08)",
            "0 2px 4px -1px rgba(0,0,0,0.3), 0 4px 12px -2px rgba(0,0,0,0.4)",
        ),
        (
            "--shadow-lg",
            "0 4px 6px -2px rgba(0,0,0,0.05), 0 12px 24px -4px rgba(0,0,0,0.1)",
            "0 4px 6px -2px rgba(0,0,0,0.3), 0 12px 24px -4px rgba(0,0,0,0.5)",
        ),
    ];
    let ext = extension_defaults();
    let mut light: Tokens = pairs
        .iter()
        .map(|(k, l, _)| (k.to_string(), l.to_string()))
        .collect();
    let mut dark: Tokens = pairs
        .iter()
        .map(|(k, _, d)| (k.to_string(), d.to_string()))
        .collect();
    for (k, v) in ext {
        light.insert(k.clone(), v.clone());
        dark.insert(k, v);
    }
    BTreeMap::from([(BASE_MODE.into(), light), ("dark".into(), dark)])
}

/// Only the public shadcn/ui default ships as a seed; other systems come in as pasted CSS.
pub const PRESETS: &[&str] = &["shadcn"];

/// Light and dark token sets for a named preset.
pub fn preset(name: &str) -> Result<BTreeMap<String, Tokens>> {
    match name {
        "shadcn" => Ok(shadcn_preset()),
        _ => Err(format!(
            "Unknown preset {name}; use shadcn, or pass another theme as css or modes"
        )),
    }
}
