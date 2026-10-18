//! The component specimen behind `create_kit`: shadcn-style components drawn as static HTML
//! whose every color, radius, font and shadow is a `var(--token)` reference, so editing a token
//! restyles the whole kit live. Layer names follow the component names.
use crate::{
    document::{Document, Node, Result},
    html,
};

pub const WIDTH: f64 = 1280.0;

const ICONS: &[(&str, &str)] = &[
    ("check", r#"<path d="M20 6 9 17l-5-5" />"#),
    ("chevron-down", r#"<path d="m6 9 6 6 6-6" />"#),
    ("chevron-right", r#"<path d="m9 18 6-6-6-6" />"#),
    ("chevron-left", r#"<path d="m15 18-6-6 6-6" />"#),
    ("plus", r#"<path d="M5 12h14" /><path d="M12 5v14" />"#),
    ("x", r#"<path d="M18 6 6 18" /><path d="m6 6 12 12" />"#),
    (
        "search",
        r#"<circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" />"#,
    ),
    (
        "arrow-right",
        r#"<path d="M5 12h14" /><path d="m12 5 7 7-7 7" />"#,
    ),
    (
        "trash",
        r#"<path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />"#,
    ),
    (
        "info",
        r#"<circle cx="12" cy="12" r="10" /><path d="M12 16v-4" /><path d="M12 8h.01" />"#,
    ),
    (
        "alert",
        r#"<circle cx="12" cy="12" r="10" /><path d="M12 8v4" /><path d="M12 16h.01" />"#,
    ),
    (
        "user",
        r#"<circle cx="12" cy="8" r="4" /><path d="M20 21a8 8 0 0 0-16 0" />"#,
    ),
    (
        "card",
        r#"<rect width="20" height="14" x="2" y="5" rx="2" /><path d="M2 10h20" />"#,
    ),
    (
        "copy",
        r#"<rect width="14" height="14" x="8" y="8" rx="2" /><path d="M4 16a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2" />"#,
    ),
    (
        "logout",
        r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" /><path d="m16 17 5-5-5-5" /><path d="M21 12H9" />"#,
    ),
    (
        "trend",
        r#"<path d="M22 7 13.5 15.5 8.5 10.5 2 17" /><path d="M16 7h6v6" />"#,
    ),
    (
        "bell",
        r#"<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />"#,
    ),
    (
        "upload",
        r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m17 8-5-5-5 5" /><path d="M12 3v12" />"#,
    ),
];

fn icon(name: &str, size: u32) -> String {
    let paths = ICONS.iter().find(|(n, _)| *n == name).map_or("", |i| i.1);
    // data-icon makes each one an icon layer, like insert_icon's, named after its Lucide icon.
    let lucide = match name {
        "alert" => "circle-alert",
        "card" => "credit-card",
        "logout" => "log-out",
        "trend" => "trending-up",
        other => other,
    };
    format!(
        r#"<svg layer-name="Icon {name}" data-icon="lucide:{lucide}" viewBox="0 0 24 24" width="{size}" height="{size}" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="flex-shrink:0">{paths}</svg>"#
    )
}

const DISPLAY: &str = "font-family:var(--font-display);font-weight:var(--display-weight);letter-spacing:var(--display-tracking)";
const FIGURES: &str = "font-family:var(--font-figures);font-variant-numeric:tabular-nums";
const MUTED: &str = "color:var(--muted-foreground)";
const CARD: &str = "display:flex;flex-direction:column;background:var(--card);color:var(--card-foreground);border-radius:calc(var(--radius) * 1.4);border:1px solid var(--card-ring);box-shadow:var(--card-shadow);overflow:hidden";
const FIELD: &str = "display:flex;align-items:center;gap:8px;height:36px;padding:0 12px;border-radius:var(--radius-field);border:1px solid var(--field-border);background:var(--field-bg);font-size:14px";
const ROW: &str = "display:flex;flex-wrap:wrap;align-items:center;gap:12px";

fn button(
    label: &str,
    variant: &str,
    size: &str,
    lead: Option<&str>,
    trail: Option<&str>,
) -> String {
    let (h, px, font, gap, ic) = match size {
        "xs" => (24, 8, 12, 4, 12),
        "sm" => (28, 10, 13, 6, 14),
        "lg" => (40, 18, 14, 8, 16),
        _ => (34, 14, 14, 6, 16),
    };
    let look = match variant {
        "secondary" => "background:var(--secondary);color:var(--secondary-foreground)",
        "outline" => {
            "background:var(--background);color:var(--foreground);box-shadow:inset 0 0 0 1px var(--border)"
        }
        "ghost" => "background:transparent;color:var(--foreground)",
        "destructive" => {
            "background:color-mix(in srgb, var(--destructive) 12%, transparent);color:var(--destructive)"
        }
        "link" => {
            "background:transparent;color:var(--primary);text-decoration:underline;text-underline-offset:4px"
        }
        _ => "background:var(--primary);color:var(--primary-foreground)",
    };
    let pad = if label.is_empty() {
        format!("width:{h}px;padding:0")
    } else {
        format!("padding:0 {px}px")
    };
    let mut inner = String::new();
    if let Some(i) = lead {
        inner.push_str(&icon(i, ic));
    }
    if !label.is_empty() {
        inner.push_str(&format!("<span>{label}</span>"));
    }
    if let Some(i) = trail {
        inner.push_str(&icon(i, ic));
    }
    let name = if label.is_empty() { "Icon" } else { label };
    format!(
        r#"<div layer-name="Button / {variant} / {name}" style="display:inline-flex;align-items:center;justify-content:center;gap:{gap}px;height:{h}px;{pad};border-radius:var(--radius-control);font-size:{font}px;font-weight:500;line-height:1;white-space:nowrap;flex-shrink:0;{look}">{inner}</div>"#
    )
}

fn badge(label: &str, variant: &str, dot: bool) -> String {
    let look = match variant {
        "secondary" => "background:var(--secondary);color:var(--secondary-foreground)",
        "outline" => "box-shadow:inset 0 0 0 1px var(--border);color:var(--foreground)",
        "destructive" => {
            "background:color-mix(in srgb, var(--destructive) 12%, transparent);color:var(--destructive)"
        }
        _ => "background:var(--primary);color:var(--primary-foreground)",
    };
    let dot = if dot {
        r#"<span layer-name="Signal" style="width:6px;height:6px;border-radius:999px;background:var(--signal)"></span>"#
    } else {
        ""
    };
    format!(
        r#"<div layer-name="Badge / {variant} / {label}" style="display:inline-flex;align-items:center;gap:6px;height:22px;padding:0 9px;border-radius:var(--radius-chip);font-size:12px;font-weight:500;line-height:1;white-space:nowrap;{look}">{dot}<span>{label}</span></div>"#
    )
}

fn section(title: &str, note: &str, body: &str) -> String {
    format!(
        r#"<section layer-name="{title}" style="display:flex;flex-direction:column;gap:24px;padding:48px 0;border-top:1px solid var(--border)"><div layer-name="Section header" style="display:flex;align-items:baseline;justify-content:space-between;gap:24px"><h2 style="margin:0;font-size:22px;line-height:28px;{DISPLAY}">{title}</h2><p style="margin:0;flex:1;text-align:right;font-size:14px;line-height:20px;{MUTED}">{note}</p></div>{body}</section>"#
    )
}

fn row(label: &str, body: &str) -> String {
    format!(
        r#"<div layer-name="Row / {label}" style="display:grid;grid-template-columns:120px 1fr;align-items:center;gap:24px"><span style="font-size:13px;{MUTED}">{label}</span><div style="{ROW}">{body}</div></div>"#
    )
}

fn card(name: &str, title: &str, description: &str, body: &str, footer: &str) -> String {
    let header = if title.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div layer-name="Card header" style="display:flex;flex-direction:column;gap:4px;padding:20px 20px 0"><h3 style="margin:0;font-size:16px;line-height:22px;font-weight:600">{title}</h3><p style="margin:0;font-size:14px;line-height:20px;{MUTED}">{description}</p></div>"#
        )
    };
    let footer = if footer.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div layer-name="Card footer" style="display:flex;justify-content:flex-end;gap:8px;padding:16px 20px;border-top:1px solid var(--border);margin-top:auto">{footer}</div>"#
        )
    };
    format!(
        r#"<div layer-name="{name}" style="{CARD}">{header}<div layer-name="Card content" style="display:flex;flex-direction:column;gap:18px;padding:20px">{body}</div>{footer}</div>"#
    )
}

fn field(label: &str, control: &str, hint: &str) -> String {
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!(r#"<span style="font-size:13px;line-height:18px;{MUTED}">{hint}</span>"#)
    };
    format!(
        r#"<div layer-name="Field / {label}" style="display:flex;flex-direction:column;gap:8px"><label style="font-size:14px;font-weight:500;line-height:1">{label}</label>{control}{hint}</div>"#
    )
}

fn input(value: &str, placeholder: bool, focused: bool, lead: Option<&str>) -> String {
    let color = if placeholder {
        "color:var(--muted-foreground)"
    } else {
        "color:var(--foreground)"
    };
    let focus = if focused {
        ";border-color:var(--ring);box-shadow:0 0 0 3px color-mix(in srgb, var(--ring) 35%, transparent)"
    } else {
        ""
    };
    let lead = lead
        .map(|i| {
            format!(
                r#"<span style="display:flex;{MUTED}">{}</span>"#,
                icon(i, 16)
            )
        })
        .unwrap_or_default();
    format!(
        r#"<div layer-name="Input" style="{FIELD}{focus}">{lead}<span style="{color}">{value}</span></div>"#
    )
}

fn checkbox(label: &str, on: bool) -> String {
    let boxed = if on {
        format!(
            r#"<div layer-name="Checkbox / on" style="display:flex;align-items:center;justify-content:center;width:16px;height:16px;border-radius:calc(var(--radius) * 0.4);background:var(--primary);color:var(--primary-foreground);flex-shrink:0">{}</div>"#,
            icon("check", 12)
        )
    } else {
        r#"<div layer-name="Checkbox / off" style="width:16px;height:16px;border-radius:calc(var(--radius) * 0.4);box-shadow:inset 0 0 0 1px var(--input);background:var(--field-bg);flex-shrink:0"></div>"#.into()
    };
    format!(
        r#"<div layer-name="Checkbox row / {label}" style="display:flex;align-items:center;gap:10px">{boxed}<label style="font-size:14px;line-height:1">{label}</label></div>"#
    )
}

fn radio(label: &str, on: bool) -> String {
    let dot = if on {
        r#"<div style="width:8px;height:8px;border-radius:999px;background:var(--primary)"></div>"#
    } else {
        ""
    };
    let ring = if on { "var(--primary)" } else { "var(--input)" };
    format!(
        r#"<div layer-name="Radio row / {label}" style="display:flex;align-items:center;gap:10px"><div layer-name="Radio" style="display:flex;align-items:center;justify-content:center;width:16px;height:16px;border-radius:999px;box-shadow:inset 0 0 0 1px {ring};flex-shrink:0">{dot}</div><label style="font-size:14px;line-height:1">{label}</label></div>"#
    )
}

fn switch(label: &str, hint: &str, on: bool) -> String {
    let (track, justify) = if on {
        ("var(--primary)", "flex-end")
    } else {
        ("var(--input)", "flex-start")
    };
    format!(
        r#"<div layer-name="Switch row / {label}" style="display:flex;align-items:center;justify-content:space-between;gap:16px"><div style="display:flex;flex-direction:column;gap:4px"><label style="font-size:14px;font-weight:500;line-height:1">{label}</label><span style="font-size:13px;line-height:18px;{MUTED}">{hint}</span></div><div layer-name="Switch / {}" style="display:flex;justify-content:{justify};align-items:center;width:36px;height:20px;padding:2px;border-radius:999px;background:{track};flex-shrink:0"><div style="width:16px;height:16px;border-radius:999px;background:var(--background);box-shadow:var(--shadow-sm)"></div></div></div>"#,
        if on { "on" } else { "off" }
    )
}

fn avatar(initials: &str, size: u32, ring: bool) -> String {
    let ring = if ring {
        ";box-shadow:0 0 0 2px var(--card);margin-left:-6px"
    } else {
        ""
    };
    format!(
        r#"<div layer-name="Avatar / {initials}" style="display:flex;align-items:center;justify-content:center;width:{size}px;height:{size}px;border-radius:999px;background:var(--secondary);color:var(--secondary-foreground);font-size:{}px;font-weight:500;flex-shrink:0{ring}">{initials}</div>"#,
        size * 9 / 25
    )
}

fn swatch(name: &str) -> String {
    format!(
        r#"<div layer-name="Swatch {name}" style="display:flex;flex-direction:column;gap:8px"><div style="height:56px;border-radius:var(--radius);background:var(--{name});box-shadow:inset 0 0 0 1px color-mix(in srgb, var(--foreground) 10%, transparent)"></div><span style="font-family:var(--font-mono);font-size:12px;line-height:16px">{name}</span></div>"#
    )
}

fn header(mode: &str) -> String {
    format!(
        r#"<header layer-name="Kit header" style="display:flex;align-items:flex-end;justify-content:space-between;gap:32px;padding-bottom:48px"><div style="display:flex;flex-direction:column;gap:12px"><span style="font-size:13px;font-weight:500;{MUTED}">Design system · {mode}</span><h1 style="margin:0;font-size:56px;line-height:60px;{DISPLAY}">Components</h1><p style="margin:0;max-width:560px;font-size:16px;line-height:24px;{MUTED}">Every color, radius, font and shadow here is a token. Edit a token and the whole kit follows, in every mode.</p></div><div style="display:flex;gap:8px">{}{}</div></header>"#,
        button("Export CSS", "outline", "default", Some("copy"), None),
        button("Publish", "default", "default", Some("upload"), None)
    )
}

fn tokens_section() -> String {
    let group = |label: &str, names: &[&str]| {
        format!(
            r#"<div layer-name="Swatches / {label}" style="display:flex;flex-direction:column;gap:12px"><span style="font-size:13px;font-weight:500">{label}</span><div style="display:grid;grid-template-columns:repeat(6, 1fr);gap:16px">{}</div></div>"#,
            names.iter().map(|n| swatch(n)).collect::<String>()
        )
    };
    let radii = [
        ("sm", "calc(var(--radius) * 0.6)"),
        ("md", "calc(var(--radius) * 0.8)"),
        ("lg", "var(--radius)"),
        ("xl", "calc(var(--radius) * 1.4)"),
        ("control", "var(--radius-control)"),
        ("field", "var(--radius-field)"),
        ("chip", "var(--radius-chip)"),
    ]
    .iter()
    .map(|(n, r)| format!(r#"<div layer-name="Radius {n}" style="display:flex;flex-direction:column;align-items:center;gap:8px"><div style="width:48px;height:48px;border-radius:{r};background:var(--muted);box-shadow:inset 0 0 0 1.5px var(--foreground)"></div><span style="font-family:var(--font-mono);font-size:12px;{MUTED}">{n}</span></div>"#))
    .collect::<String>();
    let shadows = [
        ("card", "var(--card-shadow);border:1px solid var(--card-ring)"),
        ("sm", "var(--shadow-sm)"),
        ("md", "var(--shadow-md)"),
        ("lg", "var(--shadow-lg)"),
    ]
    .iter()
    .map(|(n, s)| format!(r#"<div layer-name="Shadow {n}" style="display:flex;flex-direction:column;align-items:center;gap:8px"><div style="width:56px;height:48px;border-radius:var(--radius);background:var(--popover);box-shadow:{s}"></div><span style="font-family:var(--font-mono);font-size:12px;{MUTED}">{n}</span></div>"#))
    .collect::<String>();
    section(
        "Tokens",
        "The shadcn contract plus signal, card-ring, card-shadow and field tokens.",
        &format!(
            r#"{}{}{}{}<div style="display:grid;grid-template-columns:1fr 1fr;gap:32px;padding-top:8px"><div layer-name="Radius" style="display:flex;flex-direction:column;gap:12px"><span style="font-size:13px;font-weight:500">Radius</span><div style="display:flex;gap:16px">{radii}</div></div><div layer-name="Elevation" style="display:flex;flex-direction:column;gap:12px"><span style="font-size:13px;font-weight:500">Elevation</span><div style="display:flex;gap:24px">{shadows}</div></div></div>"#,
            group(
                "Surfaces",
                &[
                    "background",
                    "card",
                    "popover",
                    "secondary",
                    "muted",
                    "accent"
                ]
            ),
            group(
                "Text and action",
                &[
                    "foreground",
                    "muted-foreground",
                    "primary",
                    "primary-foreground",
                    "destructive",
                    "signal"
                ]
            ),
            group(
                "Lines and focus",
                &[
                    "border",
                    "input",
                    "ring",
                    "card-ring",
                    "field-bg",
                    "field-border"
                ]
            ),
            group(
                "Chart",
                &["chart-1", "chart-2", "chart-3", "chart-4", "chart-5"]
            ),
        ),
    )
}

fn type_section() -> String {
    let line = |label: &str, sample: String| {
        let rule = if label == "Display" {
            ""
        } else {
            ";border-top:1px solid var(--border)"
        };
        format!(
            r#"<div layer-name="Type / {label}" style="display:grid;grid-template-columns:120px 1fr;align-items:baseline;gap:24px;padding:16px 0{rule}"><span style="font-size:13px;{MUTED}">{label}</span>{sample}</div>"#
        )
    };
    section(
        "Type",
        "Display, figures and mono each have a token of their own.",
        &[
            line("Display", format!(r#"<span style="font-size:56px;line-height:60px;{DISPLAY}">Ship the work, <span style="{MUTED}">not the busywork.</span></span>"#)),
            line("Heading", format!(r#"<span style="font-size:32px;line-height:38px;{DISPLAY}">Plan, track and review in one place</span>"#)),
            line("Title", r#"<span style="font-size:20px;line-height:28px;font-weight:600">Quarterly planning</span>"#.into()),
            line("Body", format!(r#"<span style="max-width:640px;font-size:16px;line-height:26px;{MUTED}">Invite your team, connect a repository and set a budget. Everything else updates as work moves, so the status page never needs a meeting.</span>"#)),
            line("Small", r#"<span style="font-size:14px;line-height:20px">Last synced 4 minutes ago by Maya Chen</span>"#.into()),
            line("Figures", format!(r#"<span style="font-size:40px;line-height:44px;letter-spacing:-0.02em;{FIGURES}">$48,210.75 · 2,318</span>"#)),
            line("Code", r#"<span><code style="padding:4px 8px;border-radius:calc(var(--radius) * 0.6);background:var(--muted);font-family:var(--font-mono);font-size:13px">npm install @acme/ui</code></span>"#.into()),
        ]
        .concat(),
    )
}

fn actions_section() -> String {
    section(
        "Actions",
        "Buttons use --radius-control; badges use --radius-chip.",
        &[
            row(
                "Button",
                &[
                    button("Create project", "default", "default", None, None),
                    button("Invite", "secondary", "default", None, None),
                    button("Share", "outline", "default", None, None),
                    button("Cancel", "ghost", "default", None, None),
                    button("Delete", "destructive", "default", Some("trash"), None),
                    button("Learn more", "link", "default", None, Some("arrow-right")),
                ]
                .concat(),
            ),
            row(
                "Sizes",
                &[
                    button("Extra small", "default", "xs", None, None),
                    button("Small", "default", "sm", None, None),
                    button("Default", "default", "default", None, None),
                    button("Large", "default", "lg", None, None),
                    button("", "outline", "default", Some("plus"), None),
                    button("", "secondary", "sm", Some("search"), None),
                    format!(r#"<div layer-name="Disabled" style="display:flex;opacity:0.5">{}</div>"#, button("Disabled", "default", "default", None, None)),
                ]
                .concat(),
            ),
            row(
                "Badge",
                &[
                    badge("New", "default", false),
                    badge("Design", "secondary", false),
                    badge("12 tasks", "outline", false),
                    badge("Overdue", "destructive", false),
                    badge("Live", "secondary", true),
                ]
                .concat(),
            ),
            row(
                "Toggle group",
                &format!(
                    r#"<div layer-name="Toggle group" style="display:flex;padding:3px;gap:2px;border-radius:var(--radius-control);box-shadow:inset 0 0 0 1px var(--border)">{}{}{}</div><span style="display:flex;align-items:center;gap:6px;font-size:13px;{MUTED}">Search <span layer-name="Kbd" style="display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 5px;border-radius:calc(var(--radius) * 0.6);background:var(--muted);color:var(--foreground);font-family:var(--font-sans);font-size:12px">⌘</span><span layer-name="Kbd" style="display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 5px;border-radius:calc(var(--radius) * 0.6);background:var(--muted);color:var(--foreground);font-size:12px">K</span></span>"#,
                    toggle("Week", false),
                    toggle("Month", true),
                    toggle("Quarter", false)
                ),
            ),
        ]
        .concat(),
    )
}

fn toggle(label: &str, on: bool) -> String {
    let look = if on {
        "background:var(--accent);color:var(--accent-foreground)"
    } else {
        "color:var(--muted-foreground)"
    };
    format!(
        r#"<div layer-name="Toggle / {label}" style="display:flex;align-items:center;height:28px;padding:0 12px;border-radius:var(--radius-control);font-size:13px;font-weight:500;{look}">{label}</div>"#
    )
}

fn forms_section() -> String {
    let select = format!(
        r#"<div layer-name="Select" style="{FIELD};justify-content:space-between"><span>Product design</span><span style="display:flex;{MUTED}">{}</span></div>"#,
        icon("chevron-down", 16)
    );
    let textarea = format!(
        r#"<div layer-name="Textarea" style="{FIELD};align-items:flex-start;height:84px;padding:10px 12px;{MUTED}">A short note for the team about this project</div>"#
    );
    let slider = r#"<div layer-name="Slider" style="display:flex;align-items:center;height:16px;position:relative"><div layer-name="Track" style="flex:1;height:6px;border-radius:999px;background:var(--muted);overflow:hidden"><div layer-name="Range" style="width:62%;height:100%;background:var(--primary)"></div></div><div layer-name="Thumb" style="position:absolute;left:62%;width:16px;height:16px;margin-left:-8px;border-radius:999px;background:var(--background);border:1.5px solid var(--primary);box-shadow:var(--shadow-sm)"></div></div>"#;
    let left = card(
        "Card / Project settings",
        "Project settings",
        "Visible to everyone in the workspace.",
        &[
            field("Name", &input("Website relaunch", false, true, None), ""),
            field(
                "Owner",
                &input("maya@acme.com", true, false, Some("user")),
                "",
            ),
            field("Team", &select, ""),
            field("Monthly budget", slider, "$6,200 of $10,000"),
            field("Description", &textarea, ""),
        ]
        .concat(),
        &[
            button("Cancel", "ghost", "default", None, None),
            button("Save changes", "default", "default", None, None),
        ]
        .concat(),
    );
    let sep = r#"<div layer-name="Separator" style="height:1px;background:var(--border)"></div>"#;
    let right = card(
        "Card / Notifications",
        "Notifications",
        "Choose what reaches your inbox.",
        &[
            switch("Weekly digest", "A summary every Monday morning.", true),
            switch("Mentions only", "Skip updates you are not tagged in.", false),
            sep.into(),
            format!(
                r#"<div layer-name="Checkbox group" style="display:flex;flex-direction:column;gap:14px">{}{}{}</div>"#,
                checkbox("New comments", true),
                checkbox("Status changes", true),
                checkbox("Pull request reviews", false)
            ),
            sep.into(),
            format!(
                r#"<div layer-name="Radio group" style="display:flex;flex-direction:column;gap:14px">{}{}{}</div>"#,
                radio("Every update", false),
                radio("Daily summary", true),
                radio("Never", false)
            ),
            sep.into(),
            field("Search", &input("Search members", true, false, Some("search")), ""),
        ]
        .concat(),
        "",
    );
    section(
        "Forms",
        "Fields use --field-bg, --field-border and --radius-field.",
        &format!(
            r#"<div style="display:grid;grid-template-columns:1fr 1fr;gap:24px;align-items:start">{left}{right}</div>"#
        ),
    )
}

fn data_section() -> String {
    let bars = [38, 52, 46, 64, 58, 72, 68, 84, 79, 92, 88, 100]
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let fill = if i == 11 { "var(--chart-1)" } else { "var(--chart-4)" };
            format!(r#"<div layer-name="Bar" style="flex:1;height:{h}%;border-radius:4px 4px 2px 2px;background:{fill}"></div>"#)
        })
        .collect::<String>();
    let months = ["J", "F", "M", "A", "M", "J", "J", "A", "S", "O", "N", "D"]
        .iter()
        .map(|m| format!(r#"<span style="flex:1;text-align:center">{m}</span>"#))
        .collect::<String>();
    let chart = format!(
        r#"<div layer-name="Chart" style="display:flex;flex-direction:column;gap:8px"><div layer-name="Bars" style="display:flex;align-items:flex-end;gap:6px;height:112px">{bars}</div><div layer-name="Axis" style="display:flex;gap:6px;font-size:11px;{MUTED}">{months}</div></div>"#
    );
    let stat = |name: &str, label: &str, value: &str, delta: &str, body: &str| {
        format!(
            r#"<div layer-name="Stat / {name}" style="{CARD}"><div style="display:flex;flex-direction:column;gap:18px;padding:20px"><div style="display:flex;justify-content:space-between;align-items:center"><span style="font-size:14px;{MUTED}">{label}</span>{delta}</div><span style="font-size:36px;line-height:40px;letter-spacing:-0.02em;{FIGURES}">{value}</span>{body}</div></div>"#
        )
    };
    let progress = |label: &str, pct: u32| {
        format!(
            r#"<div layer-name="Progress / {label}" style="display:flex;flex-direction:column;gap:8px"><div style="display:flex;justify-content:space-between;font-size:13px"><span>{label}</span><span style="{FIGURES};{MUTED}">{pct}%</span></div><div style="height:6px;border-radius:999px;background:var(--muted);overflow:hidden"><div style="width:{pct}%;height:100%;border-radius:999px;background:var(--primary)"></div></div></div>"#
        )
    };
    let up = format!(
        r#"<div layer-name="Delta" style="display:inline-flex;align-items:center;gap:4px;height:22px;padding:0 8px;border-radius:var(--radius-chip);background:var(--secondary);font-size:12px;font-weight:500">{}<span>12.4%</span></div>"#,
        icon("trend", 12)
    );
    let stats = [
        stat("Revenue", "Revenue this year", "$48,210", &up, &chart),
        stat(
            "Goals",
            "Active projects",
            "24",
            &badge("On track", "secondary", true),
            &[
                progress("Website relaunch", 82),
                progress("Mobile onboarding", 64),
                progress("Billing migration", 37),
            ]
            .concat(),
        ),
        stat(
            "Team",
            "Team members",
            "18",
            &badge("3 invited", "outline", false),
            &format!(
                r#"<div style="display:flex;flex-direction:column;gap:14px"><div layer-name="Avatar group" style="display:flex;padding-left:8px">{}{}{}{}{}</div><p style="margin:0;font-size:14px;line-height:20px;{MUTED}">Maya, Leo, Sam and 15 others worked on 9 projects this week.</p><div style="display:flex">{}</div></div>"#,
                avatar("MC", 36, true),
                avatar("LP", 36, true),
                avatar("SK", 36, true),
                avatar("AR", 36, true),
                avatar("+14", 36, true),
                button("Invite people", "outline", "sm", Some("plus"), None)
            ),
        ),
    ]
    .concat();
    let rows = [
        ("INV-2041", "Northwind Traders", "Paid", "secondary", "$4,250.00"),
        ("INV-2040", "Globex Corporation", "Pending", "outline", "$1,820.50"),
        ("INV-2039", "Initech", "Paid", "secondary", "$12,400.00"),
        ("INV-2038", "Umbrella Health", "Overdue", "destructive", "$960.00"),
    ]
    .iter()
    .map(|(id, client, status, variant, amount)| {
        format!(
            r#"<tr layer-name="Row {id}"><td style="padding:14px 20px;border-top:1px solid var(--border);font-weight:500;{FIGURES}">{id}</td><td style="padding:14px 12px;border-top:1px solid var(--border)">{client}</td><td style="padding:14px 12px;border-top:1px solid var(--border)">{}</td><td style="padding:14px 20px;border-top:1px solid var(--border);text-align:right;{FIGURES}">{amount}</td></tr>"#,
            badge(status, variant, false)
        )
    })
    .collect::<String>();
    let th = format!("padding:12px;text-align:left;font-size:13px;font-weight:500;{MUTED}");
    let table = format!(
        r#"<div layer-name="Table" style="{CARD}"><table style="width:100%;border-collapse:collapse;font-size:14px"><thead><tr><th style="{th};padding-left:20px">Invoice</th><th style="{th}">Client</th><th style="{th}">Status</th><th style="{th};padding-right:20px;text-align:right">Amount</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    section(
        "Data",
        "Figures use --font-figures; the chart uses --chart-1 and --chart-4.",
        &format!(
            r#"<div style="display:grid;grid-template-columns:repeat(3, 1fr);gap:24px">{stats}</div>{table}"#
        ),
    )
}

fn navigation_section() -> String {
    let tab = |label: &str, on: bool| {
        let look = if on {
            "background:var(--background);color:var(--foreground);box-shadow:var(--shadow-sm)"
        } else {
            "color:var(--muted-foreground)"
        };
        format!(
            r#"<div layer-name="Tab / {label}" style="display:flex;align-items:center;height:28px;padding:0 12px;border-radius:var(--radius-control);font-size:14px;font-weight:500;{look}">{label}</div>"#
        )
    };
    let line_tab = |label: &str, on: bool| {
        let look = if on {
            "color:var(--foreground);box-shadow:inset 0 -2px 0 var(--primary)"
        } else {
            "color:var(--muted-foreground)"
        };
        format!(
            r#"<div layer-name="Tab / {label}" style="display:flex;align-items:center;height:36px;padding:0 2px;font-size:14px;font-weight:500;{look}">{label}</div>"#
        )
    };
    let tabs = format!(
        r#"<div layer-name="Tabs" style="display:flex;flex-direction:column;gap:12px"><div layer-name="Tabs list" style="display:flex;align-self:flex-start;gap:2px;padding:3px;border-radius:var(--radius-control);background:var(--muted)">{}{}{}</div><p style="margin:0;font-size:14px;line-height:20px;{MUTED}">Nine projects across three teams, two due this week.</p></div>"#,
        tab("Overview", true),
        tab("Projects", false),
        tab("Activity", false)
    );
    let line_tabs = format!(
        r#"<div layer-name="Tabs / line" style="display:flex;gap:24px;box-shadow:inset 0 -1px 0 var(--border)">{}{}{}{}</div>"#,
        line_tab("Overview", false),
        line_tab("Projects", true),
        line_tab("Activity", false),
        line_tab("Settings", false)
    );
    let crumb = format!(
        r#"<div layer-name="Breadcrumb" style="display:flex;align-items:center;gap:8px;font-size:14px;{MUTED}"><span>Acme</span><span style="display:flex">{chev}</span><span>Projects</span><span style="display:flex">{chev}</span><span style="color:var(--foreground);font-weight:500">Website relaunch</span></div>"#,
        chev = icon("chevron-right", 14)
    );
    let page = |label: &str, on: bool| {
        let look = if on {
            "box-shadow:inset 0 0 0 1px var(--border);background:var(--background)"
        } else {
            ""
        };
        format!(
            r#"<div layer-name="Page {label}" style="display:flex;align-items:center;justify-content:center;min-width:32px;height:32px;border-radius:var(--radius-control);font-size:14px;{look}">{label}</div>"#
        )
    };
    let pagination = format!(
        r#"<div layer-name="Pagination" style="display:flex;align-items:center;gap:4px">{}{}{}{}{}</div>"#,
        button("Previous", "ghost", "sm", Some("chevron-left"), None),
        page("1", false),
        page("2", true),
        page("3", false),
        button("Next", "ghost", "sm", None, Some("chevron-right"))
    );
    let item = |q: &str, a: &str, open: bool| {
        let body = if open {
            format!(
                r#"<p style="margin:0;padding:0 0 16px;font-size:14px;line-height:22px;{MUTED}">{a}</p>"#
            )
        } else {
            String::new()
        };
        let rotate = if open {
            ";transform:rotate(180deg)"
        } else {
            ""
        };
        format!(
            r#"<div layer-name="Accordion item" style="display:flex;flex-direction:column;border-bottom:1px solid var(--border)"><div layer-name="Trigger" style="display:flex;align-items:center;justify-content:space-between;padding:16px 0;font-size:14px;font-weight:500"><span>{q}</span><span style="display:flex;{MUTED}{rotate}">{}</span></div>{body}</div>"#,
            icon("chevron-down", 16)
        )
    };
    let accordion = format!(
        r#"<div layer-name="Accordion" style="display:flex;flex-direction:column">{}{}{}</div>"#,
        item(
            "Who can see this project?",
            "Everyone in the workspace can view it. Only members of the project team can edit tasks or change the budget.",
            true
        ),
        item("Can I move tasks between projects?", "", false),
        item("How is the budget calculated?", "", false)
    );
    section(
        "Navigation",
        "Tabs, breadcrumb, pagination and accordion.",
        &format!(
            r#"<div style="display:grid;grid-template-columns:1fr 1fr;gap:48px"><div style="display:flex;flex-direction:column;gap:28px">{tabs}{line_tabs}{crumb}{pagination}</div>{accordion}</div>"#
        ),
    )
}

fn feedback_section() -> String {
    let alert = |name: &str, ic: &str, title: &str, text: &str, destructive: bool| {
        let (color, desc) = if destructive {
            (
                "color:var(--destructive)",
                "color:color-mix(in srgb, var(--destructive) 85%, var(--foreground))",
            )
        } else {
            ("color:var(--card-foreground)", MUTED)
        };
        format!(
            r#"<div layer-name="Alert / {name}" style="display:flex;gap:12px;padding:14px 16px;border-radius:var(--radius);background:var(--card);box-shadow:inset 0 0 0 1px var(--border);{color}"><span style="display:flex;padding-top:2px">{}</span><div style="display:flex;flex-direction:column;gap:4px"><span style="font-size:14px;font-weight:500;line-height:20px">{title}</span><span style="font-size:14px;line-height:20px;{desc}">{text}</span></div></div>"#,
            icon(ic, 16)
        )
    };
    let toast = format!(
        r#"<div layer-name="Toast" style="display:flex;align-items:center;gap:16px;padding:16px;border-radius:var(--radius);background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);box-shadow:var(--shadow-lg)"><div style="display:flex;flex-direction:column;gap:2px;flex:1"><span style="font-size:14px;font-weight:600">Project created</span><span style="font-size:13px;line-height:18px;{MUTED}">Website relaunch is ready for your team.</span></div>{}</div>"#,
        button("Undo", "outline", "sm", None, None)
    );
    let tooltip = format!(
        r#"<div layer-name="Tooltip example" style="display:flex;flex-direction:column;align-items:center;gap:8px"><div layer-name="Tooltip" style="display:flex;flex-direction:column;align-items:center"><div style="padding:6px 10px;border-radius:calc(var(--radius) * 0.6);background:var(--primary);color:var(--primary-foreground);font-size:12px;line-height:16px;white-space:nowrap">Copied to clipboard</div><div style="width:8px;height:8px;margin-top:-4px;transform:rotate(45deg);background:var(--primary)"></div></div>{}</div>"#,
        button("", "outline", "default", Some("copy"), None)
    );
    let bar = |w: &str, h: u32| {
        format!(
            r#"<div layer-name="Skeleton" style="width:{w};height:{h}px;border-radius:calc(var(--radius) * 0.6);background:var(--muted)"></div>"#
        )
    };
    let skeleton = format!(
        r#"<div layer-name="Skeleton card" style="{CARD}"><div style="display:flex;gap:14px;align-items:center;padding:20px"><div layer-name="Skeleton" style="width:44px;height:44px;border-radius:999px;background:var(--muted);flex-shrink:0"></div><div style="display:flex;flex-direction:column;gap:8px;flex:1">{}{}</div></div><div style="display:flex;flex-direction:column;gap:8px;padding:0 20px 20px">{}{}{}</div></div>"#,
        bar("55%", 14),
        bar("35%", 12),
        bar("100%", 12),
        bar("92%", 12),
        bar("70%", 12)
    );
    let avatars = format!(
        r#"<div layer-name="Avatars" style="display:flex;align-items:center;gap:12px">{}{}{}<div style="display:flex;flex-direction:column;gap:2px"><span style="font-size:14px;font-weight:500">Maya Chen</span><span style="font-size:13px;{MUTED}">maya@acme.com</span></div></div>"#,
        avatar("MC", 40, false),
        avatar("LP", 32, false),
        avatar("SK", 24, false)
    );
    section(
        "Feedback",
        "Alerts, toast, tooltip, avatars and loading skeleton.",
        &format!(
            r#"<div style="display:grid;grid-template-columns:1fr 1fr;gap:24px"><div style="display:flex;flex-direction:column;gap:16px">{}{}{toast}</div><div style="display:flex;flex-direction:column;gap:24px">{skeleton}<div style="display:flex;align-items:flex-end;justify-content:space-between;gap:24px">{avatars}{tooltip}</div></div></div>"#,
            alert(
                "default",
                "info",
                "Heads up",
                "You can add teammates from the project settings at any time.",
                false
            ),
            alert(
                "destructive",
                "alert",
                "Payment failed",
                "Your card was declined. Update billing to keep the workspace active.",
                true
            ),
        ),
    )
}

fn overlays_section() -> String {
    let dialog = format!(
        r#"<div layer-name="Dialog" style="display:flex;flex-direction:column;gap:20px;width:420px;padding:24px;border-radius:calc(var(--radius) * 1.4);background:var(--background);color:var(--foreground);border:1px solid var(--border);box-shadow:var(--shadow-lg)"><div style="display:flex;justify-content:space-between;gap:16px"><div style="display:flex;flex-direction:column;gap:6px"><h3 style="margin:0;font-size:18px;line-height:24px;{DISPLAY}">Invite teammates</h3><p style="margin:0;font-size:14px;line-height:20px;{MUTED}">They get access to every project in this workspace.</p></div><span style="display:flex;{MUTED}">{}</span></div>{}<div style="display:flex;justify-content:flex-end;gap:8px">{}{}</div></div>"#,
        icon("x", 16),
        field(
            "Email",
            &input("leo@acme.com, sam@acme.com", false, false, None),
            ""
        ),
        button("Cancel", "outline", "default", None, None),
        button("Send invites", "default", "default", None, None)
    );
    let menu_item = |ic: &str, label: &str, shortcut: &str, destructive: bool| {
        let color = if destructive {
            "color:var(--destructive)"
        } else {
            ""
        };
        format!(
            r#"<div layer-name="Menu item / {label}" style="display:flex;align-items:center;gap:8px;height:32px;padding:0 8px;border-radius:calc(var(--radius) * 0.6);font-size:14px;{color}"><span style="display:flex;{}">{}</span><span style="flex:1">{label}</span><span style="font-size:12px;letter-spacing:0.08em;{MUTED}">{shortcut}</span></div>"#,
            if destructive { "" } else { MUTED },
            icon(ic, 16)
        )
    };
    let sep = r#"<div layer-name="Separator" style="height:1px;margin:4px -4px;background:var(--border)"></div>"#;
    let highlighted = format!(
        r#"<div layer-name="Menu item / highlighted" style="display:flex;border-radius:calc(var(--radius) * 0.6);background:var(--accent);color:var(--accent-foreground)">{}</div>"#,
        menu_item("card", "Billing", "⌘B", false)
    )
    .replace("height:32px", "height:32px;flex:1");
    let menu = format!(
        r#"<div layer-name="Dropdown example" style="display:flex;flex-direction:column;align-items:flex-start;gap:6px">{}<div layer-name="Dropdown menu" style="display:flex;flex-direction:column;width:224px;padding:4px;border-radius:var(--radius);background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);box-shadow:var(--shadow-md)"><span style="padding:6px 8px;font-size:12px;font-weight:500;{MUTED}">maya@acme.com</span>{}{}{}{}{}{}</div></div>"#,
        button(
            "Account",
            "outline",
            "default",
            Some("user"),
            Some("chevron-down")
        ),
        menu_item("user", "Profile", "⌘P", false),
        highlighted,
        menu_item("bell", "Notifications", "", false),
        sep,
        menu_item("logout", "Sign out", "⇧⌘Q", false),
        menu_item("trash", "Delete account", "", true)
    );
    section(
        "Overlays",
        "Dialog and menu surfaces use --popover and the shadow scale.",
        &format!(
            r#"<div layer-name="Overlay stage" style="display:flex;align-items:center;justify-content:center;gap:56px;padding:48px;border-radius:calc(var(--radius) * 1.4);background:color-mix(in srgb, var(--muted) 70%, var(--background));background-image:radial-gradient(color-mix(in srgb, var(--foreground) 10%, transparent) 1px, transparent 1px);background-size:16px 16px">{dialog}{menu}</div>"#
        ),
    )
}

/// One kit artboard rendering in `mode`.
pub fn artboard(doc: &mut Document, mode: &str) -> Result<Node> {
    let body = [
        header(mode),
        tokens_section(),
        type_section(),
        actions_section(),
        forms_section(),
        data_section(),
        navigation_section(),
        feedback_section(),
        overlays_section(),
    ]
    .concat();
    let markup = format!(
        r#"<div layer-name="Design system · {mode}" data-mode="{mode}" style="display:flex;flex-direction:column;width:{WIDTH}px;height:fit-content;padding:72px 80px 40px;background:var(--background);color:var(--foreground);font-family:var(--font-sans);font-size:14px;line-height:1.5;-webkit-font-smoothing:antialiased">{body}</div>"#
    );
    let mut nodes = html::parse_fragment(doc, &markup)?;
    nodes.pop().ok_or_else(|| "Kit markup is empty".into())
}
