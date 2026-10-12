#!/usr/bin/env python3
"""Turns the landing design's HTML export into site/index.html.

The landing page is a Cardboard design. To update the site, edit the design, save its
HTML (`cardboard export --document landing.cardboard > landing.html` or the export_html
tool), then run: scripts/build-site.py landing.html
This adds what a design export doesn't carry: page metadata, web fonts and links.
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
<meta property="og:title" content="Cardboard">
<meta property="og:description" content="Hand the pencil to your agent. A local-first HTML/CSS design editor with MCP built in.">
<meta property="og:image" content="https://cardboard.goerll.dev/cardboard-512.png">
<link rel="icon" href="favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:wght@400..700&family=Fira+Code:wght@400;500&display=swap">
<style>a{color:inherit;text-decoration:none}a:hover{opacity:.75}html{scroll-behavior:smooth;background:#faf5ec}body>div{margin:0 auto}</style>
"""

html = Path(sys.argv[1]).read_text()
html = re.sub(r'<meta http-equiv="Content-Security-Policy"[^>]*>', "", html)
# The design is a fixed 1440px artboard; let phones zoom out to fit it.
html = html.replace('content="width=device-width,initial-scale=1"', 'content="width=1440"')
html = html.replace("</head>", HEAD + "</head>", 1)

sections = iter(SECTION_IDS)
html = re.sub(r"<section\b", lambda m: f'<section id="{s}"' if (s := next(sections, "")) else m[0], html)


def link(m):
    href = LINKS.get(m[3])
    if not href:
        return m[0]
    external = ' target="_blank" rel="noopener"' if href.startswith("http") else ""
    # Every link sits in a flex row, so swapping the tag for <a> keeps the layout.
    return f'<a href="{href}"{external}{m[2]}>{m[3]}</a>'


html = re.sub(r"<(span|div)((?:\s[^>]*)?)>([^<>]+)</\1>", link, html)

out = Path(__file__).resolve().parent.parent / "site" / "index.html"
out.write_text(html)
print(f"wrote {out} ({len(html) // 1024} KB)")
