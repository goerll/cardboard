#!/usr/bin/env python3
"""Turns the landing design's HTML export into site/index.html.

The landing page is a Cardboard design. To update the site, edit the design, save its
HTML (`cardboard export --document landing.cardboard > landing.html` or the export_html
tool), then run: scripts/build-site.py landing.html
This adds what a design export doesn't carry: page metadata, web fonts and links. The design
has a desktop artboard (1440px) and a mobile one (390px); screens under 900px get the
mobile one, and the desktop one scales down below 1440px.
"""
import re
import sys
from pathlib import Path

REPO = "https://github.com/goerll/cardboard"
BLOB = f"{REPO}/blob/main"
LINKS = {  # visible text -> href, applied to every element whose whole text matches
    "Editor": "#editor",
    "Agents": "#agents",
    "Your files": "#files",
    "Docs": f"{BLOB}/docs/architecture.md",
    "Star on GitHub": REPO,
    "Download": "#install",
    "Install Cardboard": "#install",
    "Read the source": REPO,
    "All downloads": f"{REPO}/releases/latest",
    "Changelog": f"{REPO}/releases",
    "Shortcuts": f"{REPO}#using-it",
    "MCP guide": f"{BLOB}/docs/mcp.md",
    "Tool reference": f"{BLOB}/docs/mcp.md",
    "Design guide": f"{BLOB}/guides/design.md",
    "GitHub": REPO,
    "Issues": f"{REPO}/issues",
    "License": f"{BLOB}/LICENSE",
}
SECTION_IDS = ["", "editor", "files", "agents", "install"]  # top-level <section>s in order

HEAD = """<title>Cardboard: a local-first design editor for you and your agent</title>
<meta name="description" content="Cardboard is a free, open source design editor where you and your AI agent work on the same canvas. Every layer is real HTML and CSS, and every file stays on your machine.">
<meta property="og:type" content="website">
<meta property="og:url" content="https://cardboard.goerll.dev/">
<meta property="og:site_name" content="Cardboard">
<meta property="og:title" content="Cardboard: hand the pencil to your agent">
<meta property="og:description" content="A free, local-first design editor. Real HTML and CSS, with your AI agent on the same canvas.">
<meta property="og:image" content="https://cardboard.goerll.dev/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="Cardboard: hand the pencil to your agent">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:image" content="https://cardboard.goerll.dev/og.png">
<meta name="theme-color" content="#faf5ec">
<link rel="icon" href="favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400..700&family=Fira+Code:wght@400;500&display=swap">
<style>a{color:inherit;text-decoration:none}a:hover{opacity:.75}html{scroll-behavior:smooth;background:#faf5ec;overflow-x:hidden}body>div{margin:0 auto}</style>
"""

RESPONSIVE = """<style>
.artboard-desktop{margin:0 auto}
.artboard-mobile{width:100%!important;max-width:600px;margin:0 auto}
@media (max-width:899px){.artboard-desktop{display:none!important}}
@media (min-width:900px){.artboard-mobile{display:none!important}}
</style>
<script>
// Below 1440px the desktop design scales down instead of scrolling sideways.
addEventListener("DOMContentLoaded",()=>{const d=document.querySelector(".artboard-desktop");
const fit=()=>{d.style.zoom=Math.min(1,document.documentElement.clientWidth/1440)};fit();addEventListener("resize",fit)});
</script>
"""


def top_level_divs(body):
    """Splits the body's markup into its top-level <div> elements (one per artboard)."""
    parts, depth, start = [], 0, 0
    for m in re.finditer(r"<div\b|</div>", body):
        if m[0] == "</div>":
            depth -= 1
            if depth == 0:
                parts.append(body[start:m.end()])
        else:
            if depth == 0:
                start = m.start()
            depth += 1
    return parts


def link(m, prefix):
    href = LINKS.get(m[3])
    if not href:
        return m[0]
    if href.startswith("#"):
        href = "#" + prefix + href[1:]
    external = ' target="_blank" rel="noopener"' if href.startswith("http") else ""
    # Every link sits in a flex row, so swapping the tag for <a> keeps the layout.
    return f'<a href="{href}"{external}{m[2]}>{m[3]}</a>'


def finish(board, kind):
    prefix = "" if kind == "desktop" else "m-"
    board = board.replace("<div ", f'<div class="artboard-{kind}" ', 1)
    if prefix:
        # Artboards are copies, so SVG gradient and clip ids repeat. url(#id) resolves to the
        # first match, which sits in the hidden artboard and doesn't paint; make them unique.
        board = re.sub(r'\bid="([^"]+)"', rf'id="{prefix}\1"', board)
        board = re.sub(r"url\(#([^)]+)\)", rf"url(#{prefix}\1)", board)
    sections = iter(SECTION_IDS)
    board = re.sub(r"<section\b", lambda m: f'<section id="{prefix}{s}"' if (s := next(sections, "")) else m[0], board)
    return re.sub(r"<(span|div)((?:\s[^>]*)?)>([^<>]+)</\1>", lambda m: link(m, prefix), board)


html = Path(sys.argv[1]).read_text()
html = re.sub(r'<meta http-equiv="Content-Security-Policy"[^>]*>', "", html)
html = html.replace("</head>", HEAD + RESPONSIVE + "</head>", 1)
head, body = html.split("<body>", 1)
boards = top_level_divs(body)
width = lambda b: int(re.search(r"width:(\d+)px", b[: b.index(">")])[1])
boards.sort(key=width, reverse=True)  # widest artboard is the desktop design
kinds = ["desktop", "mobile"]
html = head + "<body>" + "".join(finish(b, k) for b, k in zip(boards, kinds)) + "</body></html>"

out = Path(__file__).resolve().parent.parent / "site" / "index.html"
out.write_text(html)
print(f"wrote {out} ({len(html) // 1024} KB)")
