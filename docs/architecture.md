# Cardboard architecture (v2)

## Why v2

v1 drew the editor UI with egui and rendered designs in an offscreen Chromium (CEF), copying
pixels from Chromium into an egui texture on the CPU. Two input systems (egui and Chromium) both
saw every click, which caused click-through bugs (dragging the color picker also dragged the layer
behind it), and the CPU pixel path capped performance.

v2 keeps Rust for everything that owns data, and lets Chromium do what it is good at:

```
 ┌──────────────── cardboard-desktop (Rust) ─────────────────┐
 │  CEF Views window (GPU composited, no pixel copies)        │
 │    └── loads http://127.0.0.1:<port>/?t=<token>            │
 │                                                            │
 │  cardboard core (Rust lib)                                 │
 │    Store: open documents, per-file undo, debounced saves   │
 │    Library: ~/Documents/Cardboard/*.cardboard              │
 │    tools::call  ← single command engine                    │
 │    HTTP + WebSocket server (axum) → editor UI              │
 │    MCP over Unix socket            → agents                │
 └────────────────────────────────────────────────────────────┘
```

* The editor UI (`ui/`, React + TypeScript, built with Vite) is one web page: tab bar, dashboard,
  layers, inspector and canvas. Popovers, panels and the canvas live in one DOM, so z-order and
  event routing are handled by the browser. No click-through.
* Designs render inside a same-origin `<iframe sandbox="allow-same-origin">` (no scripts in user
  designs). The editor manipulates the iframe DOM directly (no postMessage) and draws selection
  overlays in the parent page above it.
* `cardboard serve` runs the same server headless, so the UI also works in any Chromium
  (`--open` launches `chromium --app=URL`). Useful for development and automated checks.

## File format: `.cardboard`

One JSON file per design:

```json
{ "format": "cardboard", "version": 2, "document": { ...Document... } }
```

`Document` is the existing model (`src/document.rs`) plus:

* `Node.position: Option<[f64; 2]>` (serde default, skipped when None): canvas position of a
  top-level artboard. Artboards without a position are laid out left to right with 80px gaps.
* `Document.id` is a random stable id (e.g. `f-1a2b3c4d5e6f7a8b`), independent of file path.

Library directory: `--library PATH`, else `$CARDBOARD_LIBRARY`, else `~/Documents/Cardboard`.
Archived files live in `<library>/.archive/`. Library metadata (recents with opened-at times,
open tabs, active tab, UI settings) lives in `<library>/.cardboard-library.json`.
On first run, a legacy `~/.local/share/cardboard/workspace.json` is imported (each file becomes a
`.cardboard`), and left in place.

Saves are atomic (temp file + rename) and debounced (~300ms after the last edit, plus on close
and shutdown).

## WebSocket protocol

Connect to `ws://127.0.0.1:<port>/ws?t=<token>`. All messages are JSON text frames.

Client → server:

```json
{ "id": 7, "tool": "update_styles", "args": { "fileId": "...", "updates": [...] } }
```

Server → client:

```jsonc
// on connect, and whenever tabs / active file change
{ "type": "workspace", "tabs": [{ "fileId": "f-..", "name": "Lively iris", "path": "/.../Lively iris.cardboard" }],
  "activeFile": "f-.." | null, "library": "/home/u/Documents/Cardboard" }
// on connect for every open file, and after any change to that file
// (edit, undo, selection, active page) from any source, UI or MCP
{ "type": "document", "fileId": "f-..", "doc": { ...Document... }, "canUndo": true, "canRedo": false }
{ "type": "closed", "fileId": "f-.." }
// library contents changed (create/rename/archive/delete/save of a closed file)
{ "type": "library" }
// reply to a request; the server sends any resulting document/workspace pushes BEFORE the result
{ "type": "result", "id": 7, "ok": true, "result": { ... } }
{ "type": "result", "id": 7, "ok": false, "error": "message" }
```

## Tools (shared by UI and MCP)

Existing tools keep their names and arguments. `fileId` defaults to the active file.
Additions:

| Tool | Args | Notes |
| --- | --- | --- |
| `list_files` | `{archived?: bool}` | `{files:[{path, name, fileId, open, archived, modified (unix ms), opened (unix ms or null), tokens, artboards:[{id, name, x, y, w, h, html}]}]}`, most recent first. `artboards` are the first page's; `html` is `node_html(editor=false)` (positioned by x/y; w/h are authored px, 0 if not px). Total html per file is capped at 200KB; artboards past the cap get `html: ""` |
| `open_file` | `{path}` or `{fileId}` | opens (if needed) and activates; adds a tab. Returns `get_basic_info` plus `path` |
| `close_file` | `{fileId}` | flushes save, removes tab |
| `show_dashboard` | `{}` | activeFile = null |
| `create_file` | `{name?}` | name defaults to a random two-word name ("Lively iris"); creates on disk with an empty "Page 1". Opens + activates. Returns `{fileId, path}` |
| `rename_file` | `{fileId? or path, name}` | renames doc and file on disk (not undoable). Returns `{path, name}` |
| `duplicate_file` | `{path}` or `{fileId}` | "Name copy". Returns `{path}` (not opened) |
| `archive_file` / `restore_file` | `{path}` (archive also takes `{fileId}`) | move into / out of `.archive/` (closes it if open). Returns new `{path}` |
| `delete_file` | `{path}` | only for archived files |
| `get_document` | `{fileId}` | `{document}` |
| `select_nodes` | `{nodeIds}` | sets doc.selection; not undoable; no revision bump |
| `set_active_page` | `{pageId}` | not undoable; no revision bump; clears selection |
| `update_pages` | `{updates:[{pageId, name?, background?}]}` | undoable. `background` is a CSS color for the canvas behind artboards (`""` clears; UI default `#282828`) |
| `delete_pages` | `{pageIds}` | keeps at least one page |
| `duplicate_page` | `{pageId}` | |
| `set_attributes` | `{updates:[{nodeId, attributes:{name: value or "" to remove}}]}` | validated |
| `set_node_state` | `{updates:[{nodeId, visible?, locked?}]}` | |
| `set_positions` | `{updates:[{nodeId, x, y}]}` | top-level artboards only |
| `batch` | `{fileId, operations:[{tool, args}]}` | runs mutating tools on one file as ONE transaction / undo step; returns `{results:[...]}` |
| `undo` / `redo` | `{fileId?}` | per-file history (200 steps). Undo keeps the current page/selection where they still exist |

Changed existing tools:

* `write_html`: `mode` also accepts `insert-before` / `insert-after` (sibling of target);
  `insert-children` takes an optional `index` (clamped). `layer-name="..."` sets `Node.name`;
  `hidden` hides; `id` attributes are kept (SVG gradients/clip paths reference them).
* `create_artboard`: optional `x`, `y` set `position`.
* `move_nodes`: moving to `root` accepts optional `x`, `y`; moving into a node clears `position`.
* `duplicate_nodes`: without `parentId`, the copy goes right after its source; duplicated
  artboards are placed 80px right of the rightmost artboard.
  Optional per-node `index`, `x`/`y` (artboards) and `styles` overrides (empty string removes)
  let the UI do alt-drag duplicate as one undo step.
* `create_file`/`open_file` and every other library tool also change tabs, so `workspace` is pushed.

`Page.background: Option<String>` (serde default, skipped when None) holds the canvas color.

## Rust API for hosts (desktop)

```rust
let hub = cardboard::hub::Hub::new(cardboard::store::Store::library(&dir, Some(&Store::legacy_workspace()))?);
hub.spawn_autosave();                                   // inside a Tokio runtime
let server = cardboard::server::run_server(hub.clone(), 0).await?; // Server { addr, token, url, task }
// MCP for agents on the same hub:
let listener = cardboard::mcp::bind_socket(&cardboard::mcp::default_socket())?;
tokio::spawn(cardboard::mcp::serve_socket(listener, hub.clone()));
// open server.url in the window; on exit: hub.flush()
```

`Store::default_library()` honours `CARDBOARD_LIBRARY`, else `~/Documents/Cardboard`.

Selection/page changes still push a `document` message (the "doc" includes `selection` and
`activePage`) so agents and UI stay in sync.
