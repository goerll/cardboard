//! Token modes, theme presets and shadcn/tweakcn CSS import and export.
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

// ───────────────────────────── presets ─────────────────────────────

/// A theme is a small palette per mode plus shape and type knobs, expanded into the full shadcn
/// contract and the extension tokens the kit uses (ported from nexus-ds-lab `lib/themes.ts`).
struct Palette {
    ground: &'static str,
    surface: &'static str,
    tone: &'static str,
    line: &'static str,
    field: &'static str,
    text: &'static str,
    muted: &'static str,
    faint: &'static str,
    on_text: &'static str,
    destructive: &'static str,
}
struct Preset {
    sans: &'static str,
    mono: &'static str,
    figures: &'static str,
    display: (u32, &'static str),
    radius: [&'static str; 4], // base, control, field, chip
    card_shadow: &'static str,
    card_ring: bool,
    filled_fields: bool,
    shadow_rgb: &'static str,
    signal: Option<&'static str>,
    popover_white: bool,
    light: Palette,
    dark: Palette,
}

const INTER: &str = "Inter, ui-sans-serif, system-ui, sans-serif";
const GEIST: &str = "Geist, ui-sans-serif, system-ui, sans-serif";
const DM_SANS: &str = "\"DM Sans\", ui-sans-serif, system-ui, sans-serif";
const FIGTREE: &str = "Figtree, ui-sans-serif, system-ui, sans-serif";
const GEIST_MONO: &str = "\"Geist Mono\", ui-monospace, monospace";

const NEUTRAL_DARK: Palette = Palette {
    ground: "#0B0B0C",
    surface: "#141416",
    tone: "#1E1E21",
    line: "#2A2A2E",
    field: "#34343A",
    text: "#EDEDEF",
    muted: "#9A9AA1",
    faint: "#5C5C63",
    on_text: "#0B0B0C",
    destructive: "#F2715A",
};
const WARM_DARK: Palette = Palette {
    ground: "#121110",
    surface: "#1A1917",
    tone: "#24221F",
    line: "#2E2B28",
    field: "#3A3733",
    text: "#F2F0EC",
    muted: "#A39F98",
    faint: "#6B6761",
    on_text: "#121110",
    destructive: "#F2715A",
};
const COOL_DARK: Palette = Palette {
    ground: "#0F1113",
    surface: "#171A1D",
    tone: "#202428",
    line: "#2A2F34",
    field: "#363C42",
    text: "#ECEFF2",
    muted: "#9AA2AB",
    faint: "#5F666E",
    on_text: "#0F1113",
    destructive: "#F2715A",
};

fn preset_data(name: &str) -> Option<Preset> {
    Some(match name {
        "nexus" => Preset {
            sans: INTER,
            mono: GEIST_MONO,
            figures: INTER,
            display: (600, "-0.04em"),
            radius: ["0.75rem", "999px", "0.625rem", "999px"],
            card_shadow: "0 0 0 1px rgba(10,10,10,0.06), 0 1px 2px rgba(10,10,10,0.04), 0 6px 20px rgba(10,10,10,0.05)",
            card_ring: false,
            filled_fields: false,
            shadow_rgb: "10,10,10",
            signal: None,
            popover_white: true,
            light: Palette {
                ground: "#FFFFFF",
                surface: "#F6F6F6",
                tone: "#ECECEC",
                line: "#E6E6E6",
                field: "#E0E0E0",
                text: "#0A0A0A",
                muted: "#646464",
                faint: "#A6A6A6",
                on_text: "#FFFFFF",
                destructive: "#D93025",
            },
            dark: Palette {
                surface: "#161618",
                ..NEUTRAL_DARK
            },
        },
        "porcelain" => Preset {
            sans: FIGTREE,
            mono: GEIST_MONO,
            figures: FIGTREE,
            display: (600, "-0.035em"),
            radius: ["1rem", "999px", "0.75rem", "999px"],
            card_shadow: "0 1px 2px rgba(26,25,24,0.04), 0 8px 24px rgba(26,25,24,0.06)",
            card_ring: false,
            filled_fields: false,
            shadow_rgb: "26,25,24",
            signal: None,
            popover_white: false,
            light: Palette {
                ground: "#FAF9F7",
                surface: "#FFFFFF",
                tone: "#F1EFEB",
                line: "#E9E6E1",
                field: "#E2DED8",
                text: "#1A1918",
                muted: "#6E6B66",
                faint: "#B5B1AA",
                on_text: "#FFFFFF",
                destructive: "#C2412D",
            },
            dark: WARM_DARK,
        },
        "graphite" => Preset {
            sans: GEIST,
            mono: GEIST_MONO,
            figures: GEIST_MONO,
            display: (500, "-0.045em"),
            radius: ["0.5rem", "0.5rem", "0.5rem", "0.375rem"],
            card_shadow: "none",
            card_ring: true,
            filled_fields: false,
            shadow_rgb: "0,0,0",
            signal: None,
            popover_white: false,
            light: Palette {
                ground: "#FFFFFF",
                surface: "#FFFFFF",
                tone: "#F5F5F5",
                line: "#E6E6E6",
                field: "#E0E0E0",
                text: "#0A0A0A",
                muted: "#666666",
                faint: "#A8A8A8",
                on_text: "#FFFFFF",
                destructive: "#D93025",
            },
            dark: NEUTRAL_DARK,
        },
        "soft-machine" => Preset {
            sans: DM_SANS,
            mono: GEIST_MONO,
            figures: GEIST_MONO,
            display: (500, "-0.035em"),
            radius: ["1.25rem", "999px", "0.75rem", "999px"],
            card_shadow: "inset 0 1px 0 rgba(255,255,255,0.9), 0 1px 2px rgba(19,21,24,0.06), 0 6px 16px rgba(19,21,24,0.06)",
            card_ring: false,
            filled_fields: true,
            shadow_rgb: "19,21,24",
            signal: Some("#FF5B1F"),
            popover_white: false,
            light: Palette {
                ground: "#EDEFF1",
                surface: "#FFFFFF",
                tone: "#E4E7EA",
                line: "#DCE0E4",
                field: "#E4E7EA",
                text: "#131518",
                muted: "#666C74",
                faint: "#AEB3BA",
                on_text: "#FFFFFF",
                destructive: "#C2412D",
            },
            dark: COOL_DARK,
        },
        _ => return None,
    })
}

fn expand(t: &Preset, dark: bool) -> Tokens {
    let p = if dark { &t.dark } else { &t.light };
    let rgb = t.shadow_rgb;
    let mut v: Vec<(&str, String)> = vec![
        ("--background", p.ground.into()),
        ("--foreground", p.text.into()),
        ("--card", p.surface.into()),
        ("--card-foreground", p.text.into()),
        (
            "--popover",
            if t.popover_white && !dark {
                "#FFFFFF"
            } else {
                p.surface
            }
            .into(),
        ),
        ("--popover-foreground", p.text.into()),
        ("--primary", p.text.into()),
        ("--primary-foreground", p.on_text.into()),
        ("--secondary", p.tone.into()),
        ("--secondary-foreground", p.text.into()),
        ("--muted", p.tone.into()),
        ("--muted-foreground", p.muted.into()),
        ("--accent", p.tone.into()),
        ("--accent-foreground", p.text.into()),
        ("--destructive", p.destructive.into()),
        ("--border", p.line.into()),
        ("--input", p.field.into()),
        ("--ring", p.faint.into()),
        ("--chart-1", p.text.into()),
        ("--chart-2", p.muted.into()),
        ("--chart-3", p.faint.into()),
        ("--chart-4", p.line.into()),
        ("--chart-5", p.tone.into()),
        ("--radius", t.radius[0].into()),
        ("--sidebar", if dark { p.surface } else { p.ground }.into()),
        ("--sidebar-foreground", p.text.into()),
        ("--sidebar-primary", p.text.into()),
        ("--sidebar-primary-foreground", p.on_text.into()),
        ("--sidebar-accent", p.tone.into()),
        ("--sidebar-accent-foreground", p.text.into()),
        ("--sidebar-border", p.line.into()),
        ("--sidebar-ring", p.faint.into()),
        ("--font-sans", t.sans.into()),
        ("--font-mono", t.mono.into()),
        ("--shadow-sm", format!("0 1px 2px rgba({rgb},0.05)")),
        (
            "--shadow-md",
            format!("0 1px 2px rgba({rgb},0.04), 0 8px 24px rgba({rgb},0.08)"),
        ),
        (
            "--shadow-lg",
            format!("0 2px 4px rgba({rgb},0.04), 0 16px 48px rgba({rgb},0.12)"),
        ),
    ];
    v.extend(extensions(
        t.sans,
        t.figures,
        t.display,
        t.radius,
        if t.card_ring || dark {
            p.line
        } else {
            "transparent"
        },
        if dark { "none" } else { t.card_shadow },
        if t.filled_fields {
            p.field
        } else {
            "transparent"
        },
        if t.filled_fields {
            "transparent"
        } else {
            p.field
        },
        t.signal.unwrap_or(p.text),
    ));
    v.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

/// The knobs shadcn's contract lacks but real systems differ on.
#[allow(clippy::too_many_arguments)]
fn extensions(
    display_font: &str,
    figures: &str,
    display: (u32, &str),
    radius: [&str; 4],
    card_ring: &str,
    card_shadow: &str,
    field_bg: &str,
    field_border: &str,
    signal: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("--font-display", display_font.into()),
        ("--font-figures", figures.into()),
        ("--display-weight", display.0.to_string()),
        ("--display-tracking", display.1.into()),
        ("--radius-control", radius[1].into()),
        ("--radius-field", radius[2].into()),
        ("--radius-chip", radius[3].into()),
        ("--card-ring", card_ring.into()),
        ("--card-shadow", card_shadow.into()),
        ("--field-bg", field_bg.into()),
        ("--field-border", field_border.into()),
        ("--signal", signal.into()),
    ]
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
    let ext = extensions(
        "var(--font-sans)",
        "var(--font-sans)",
        (600, "-0.025em"),
        [
            "",
            "calc(var(--radius) - 2px)",
            "calc(var(--radius) - 2px)",
            "calc(var(--radius) - 4px)",
        ],
        "var(--border)",
        "var(--shadow-sm)",
        "transparent",
        "var(--input)",
        "var(--primary)",
    );
    let mut light: Tokens = pairs
        .iter()
        .map(|(k, l, _)| (k.to_string(), l.to_string()))
        .collect();
    let mut dark: Tokens = pairs
        .iter()
        .map(|(k, _, d)| (k.to_string(), d.to_string()))
        .collect();
    for (k, v) in ext {
        light.insert(k.into(), v.clone());
        dark.insert(k.into(), v);
    }
    BTreeMap::from([(BASE_MODE.into(), light), ("dark".into(), dark)])
}

pub const PRESETS: &[&str] = &["shadcn", "nexus", "porcelain", "graphite", "soft-machine"];

/// Light and dark token sets for a named preset.
pub fn preset(name: &str) -> Result<BTreeMap<String, Tokens>> {
    if name == "shadcn" {
        return Ok(shadcn_preset());
    }
    let t = preset_data(name)
        .ok_or_else(|| format!("Unknown preset {name}; use one of {}", PRESETS.join(", ")))?;
    Ok(BTreeMap::from([
        (BASE_MODE.into(), expand(&t, false)),
        ("dark".into(), expand(&t, true)),
    ]))
}
