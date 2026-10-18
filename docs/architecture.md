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

## Assets

Images live once per library in `<library>/.assets/<sha256>.<ext>` (type checked by content, 32 MB
max each) and documents reference them as the root-relative URL `/asset/<sha256>.<ext>`. The
server answers `GET /asset/<name>` from the library store (and the stores of open files from other
folders) with an immutable cache header and a `sandbox` CSP. The route is not token-gated: names
are content hashes, so only someone who already has the bytes can form one, and the dashboard's
opaque-origin thumbnail iframes cannot send the session cookie. Because the canvas, thumbnails and
inspector all resolve that URL as-is, nothing in the UI rewrites references.

Portability: saving a `.cardboard` file also writes a top-level `"assets": {"<name>": "<base64>"}`
map with every asset the document references, so a file copied to another machine or library is
self-contained. Reading a file restores missing entries into that library's store after checking
each hash. In memory, documents and undo snapshots only carry the short reference, which keeps
WebSocket pushes small. Saves still rewrite the embedded copies (the same disk cost as the old
inline base64); if that becomes a problem the next step is a sidecar folder per file.

Migration: any base64 `data:image/...` value (pasted, written by an agent, or in an older file) is
moved into the store: during the edit that wrote it, or when an older file opens (saved, not an
undo step). Memory-only sessions have no store and keep data URLs.

`export_html` inlines stored assets as `data:` URIs (default) or links them with `file://` URLs
(`assets: "files"`); both are allowed by the export's CSP (`img-src`/`font-src` include `data:`
and `file:`).

Fonts: `Document.fonts` (serde default, skipped when empty) is a list of
`{family, src, weight, style, unicodeRange?, source?}` where `src` is an asset reference. The
canvas iframe and the editor page get matching `@font-face` rules (so the font menu previews
them), dashboard thumbnails too (font assets are served with `Access-Control-Allow-Origin: *`
because those iframes have an opaque origin), and `export_html` emits them with the files inlined.
Installed families come from parsing the `name` table of TrueType/OpenType files in the system
font folders, once per process.

Network: `import_image {url | stockId}`, `add_font` (Google Fonts) and `search_images` download in `Hub::call` before taking
the store lock (`tools::prepare`), then run against local assets. MCP tool calls run on a blocking
thread. Rendering never fetches anything except hotlinked Unsplash photos, which their API terms
require.

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
{ "type": "document", "fileId": "f-..", "doc": { ...Document... }, "canUndo": true, "canRedo": false,
  "agents": { "node-12": "claude-code" } }   // layers agents are working on (see docs/mcp.md)
{ "type": "closed", "fileId": "f-.." }
// library contents changed (create/rename/archive/delete/save of a closed file)
{ "type": "library" }
// the desktop app's updater status (src/update.rs); not sent by `cardboard serve`
{ "type": "update", "update": { "state": "ready", "current": "0.2.0", "version": "0.2.1",
  "notesUrl": "https://github.com/...", "managed": true, "autoUpdate": true } }
// reply to a request; the server sends any resulting document/workspace pushes BEFORE the result
{ "type": "result", "id": 7, "ok": true, "result": { ... } }
{ "type": "result", "id": 7, "ok": false, "error": "message" }
```

`set_auto_update {enabled}`, `check_for_updates` and `restart_to_update` are UI-only requests the
server hands to the updater; they are not tools and agents can't reach them over MCP.

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
  `insert-children` takes an optional `index` (clamped, counts text: a target's own text is
  hoisted into a leading `Text` span first). `html` or an absolute `path` (.html/.htm/.svg,
  8 MiB) supplies the markup; the result carries `warnings` for mapped/dropped markup
  (`src/import.rs`). `layer-name="..."` sets `Node.name`;
  `hidden` hides; `id` attributes are kept (SVG gradients/clip paths reference them).
* `create_artboard`: optional `x`, `y` set `position`.
* `move_nodes`: moving to `root` accepts optional `x`, `y` and `pageId` (moves whole artboards
  between pages, placed right of that page's artboards unless `x`/`y` are given); moving into a
  node clears `position`. A `parentId` node may be on any page.
* `duplicate_nodes`: without `parentId`, the copy goes right after its source; duplicated
  artboards are placed 80px right of the rightmost artboard.
  Optional per-node `pageId` (copy to another page; parentId defaults to `root` there),
  `index`, `x`/`y` (artboards) and `styles` overrides (empty string removes)
  let the UI do alt-drag duplicate as one undo step.
* `create_file`/`open_file` and every other library tool also change tabs, so `workspace` is pushed.

`Page.background: Option<String>` (serde default, skipped when None) holds the canvas color.

`Node.markup: Option<String>` (serde default, skipped when None) holds the sanitized inner SVG of
an icon layer (`src/icons.rs`): the node is an `<svg>` with no children or text, and renderers
(`html::node_html`, `get_jsx`, the canvas engine) emit the markup as its content. See the Icons
section of `docs/mcp.md`.

Token modes: `Document.tokens` is the base ("light") set; `Document.modes: {mode: {token: value}}`
holds per-mode overrides and `Document.defaultMode` the mode `:root` renders in (both serde
default, skipped when empty). Layers opt into a mode with the `data-mode` attribute. `theme.rs`
builds the stylesheet (`tokens_css`, mirrored by `ui/src/lib/tokens.ts`), parses and emits
shadcn/tweakcn CSS and holds the stock shadcn seed plus extension-token defaults; `kit.rs` generates the `create_kit` specimen.

## Rust API for hosts (desktop)

```rust
// Bind first: a headless agent host saves and hands the library over before we read it.
let listener = cardboard::mcp::bind_socket(&cardboard::mcp::default_socket())?;
let hub = cardboard::hub::Hub::new(cardboard::store::Store::library(&dir, Some(&Store::legacy_workspace()))?);
hub.spawn_autosave();                                   // inside a Tokio runtime
let server = cardboard::server::run_server(hub.clone(), 0).await?; // Server { addr, token, url, task }
// MCP for agents on the same hub:
tokio::spawn(cardboard::mcp::serve_socket(listener, hub.clone()));
// open server.url in the window; on exit: hub.flush()
```

`Store::default_library()` honours `CARDBOARD_LIBRARY`, else `~/Documents/Cardboard`.

Selection/page changes still push a `document` message (the "doc" includes `selection` and
`activePage`) so agents and UI stay in sync.

## Updates

`src/update.rs`, shared by the CLI and the desktop app. Only installs in the folders the install
scripts use update themselves (`~/.local/share/cardboard/app`, `Cardboard.app` in
`/Applications` or `~/Applications`, `%LOCALAPPDATA%\Programs\Cardboard`); a dev build says why
and does nothing.

1. **Check.** The desktop starts an `Updater` thread: at launch unless it checked in the last 4
   hours, then every 24 hours. It reads `releases/latest` from the GitHub API and picks
   `cardboard-<os>-<arch>.<ext>` and its `.minisig`. A release without a signature yet counts as
   not available. Offline, rate limits and errors stay silent and retry in an hour.
2. **Stage.** Download, verify the signature with the key compiled in from
   `packaging/update.pub`, and require its trusted comment to be
   `cardboard file:<asset> version:<version>` for this exact asset and release. Extract with the
   system `tar` (`ditto` on macOS), check the layout and that the new `cardboard --version`
   matches, then move it next to the install as `.<name>.new` (same filesystem, so applying is two
   renames). State lives in `state.json` in a per-user folder (`~/.local/share/cardboard/update`,
   `~/Library/Application Support/Cardboard/update`, `%LOCALAPPDATA%\Cardboard\update`).
3. **Apply.** With the app closed: rename the install to `.<name>.old`, the staged copy into
   place, roll back if the second rename fails, delete the old copy. The desktop copies its CLI
   to the state folder and starts `cardboard update --apply --root <install> --wait-pid <pid>
   [--relaunch -- <args>]` detached, then exits; the helper waits, swaps and relaunches. This runs
   when the user clicks **Restart** in the pill (the server calls the desktop's restart hook,
   which closes the window like the close button), on quit, or at the next launch before any
   window opens. On Windows a running `cardboard mcp` relay locks the folder: the helper retries
   for 10 seconds, then leaves the update staged and the launch fast path waits an hour before
   trying again, so it never restart-loops. `cardboard update` on macOS and Linux swaps in place
   (renaming a running program's folder is fine there); with the app open it only stages.

The editor sees the status as an `update` push and shows the pill when it is `ready`.
`CARDBOARD_NO_UPDATE=1` or the Settings toggle (stored in `state.json`, per machine) turn it
off. Debug builds also read `CARDBOARD_UPDATE_API`, `CARDBOARD_UPDATE_TEST_KEY` and
`CARDBOARD_UPDATE_FAKE_READY` for `scripts/e2e_update.sh` and UI checks; release builds trust
only the embedded key. Releasing and the signing key: [releasing.md](releasing.md).
