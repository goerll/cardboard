# Cardboard

<img src="assets/cardboard.svg" width="96" alt="Cardboard icon" />

An open source, local-first HTML/CSS design editor with an MCP interface so agents can edit the same files you do. MIT licensed. No account, no cloud: your designs are `.cardboard` files in a folder.

## Run it

```sh
./scripts/run-desktop.sh                    # desktop app, library at ~/Documents/Cardboard
./scripts/run-desktop.sh --library ~/designs
cargo run --release -- serve --open         # same editor in your normal browser
```

The first desktop build downloads the CEF browser runtime and needs CMake, Ninja and a C/C++ compiler. The UI needs Node 20+ (`run-desktop.sh` builds it when sources change; otherwise run `npm --prefix ui install && npm --prefix ui run build` before `cargo build`, because the server embeds `ui/dist`).

On first run, a v1 workspace at `~/.local/share/cardboard/workspace.json` is imported into the library as `.cardboard` files. The original is left untouched.

## How it fits together

```
┌──────────── desktop (CEF window) or any browser ────────────┐
│  React UI: dashboard, tabs, layers, inspector, canvas        │
│  canvas = same-origin iframe (your design, real Blink        │
│  layout) + transparent overlay for selection and gestures    │
└──────────────── WebSocket (127.0.0.1, random token) ─────────┘
                              │
┌──────────── Rust core (`cardboard serve` / desktop) ─────────┐
│  Hub: library of .cardboard files, per-file undo,            │
│  debounced atomic saves, one command engine (tools::call)    │
└──────────────── Unix socket ── `cardboard mcp` ── agents ────┘
```

- Every edit, from you or an agent, is a tool call on the same engine, so both share undo and show up live.
- Gestures (drag, resize, color scrubbing) preview straight in the DOM and commit once on release.
- Because the whole editor is one web page, popovers and the canvas live in one DOM: clicks on a popover can never reach the design behind it.

Details: [architecture](docs/architecture.md), [MCP contract](docs/mcp.md).

## Using it

- Dashboard: Recents, Files, Archive, Settings, search (Ctrl F), grid/list, new file (Ctrl N).
- Canvas: click selects, click again drills into children, Ctrl click selects the deepest layer, double click edits text. Drag reorders inside auto layout; Ctrl while dragging nests into a sibling; Alt drag duplicates. Space or middle mouse pans, Ctrl scroll zooms, Shift 1 fits.
- Tools: `V` select, `H` hand, `F` frame, `R` rectangle, `T` text. `Ctrl+/` lists all shortcuts.
- Inspector: layout (fixed/fit/fill), flex, padding, typography, radius, fill (solid, gradient, image), border, shadows, filters, raw CSS, HTML/JSX export.

## Agents (MCP)

```json
{ "mcpServers": { "cardboard": { "command": "/path/to/cardboard", "args": ["mcp"] } } }
```

`cardboard mcp` relays to the running app over `$XDG_RUNTIME_DIR/cardboard.sock`. For standalone editing of one file: `cardboard mcp --document design.cardboard`.

## Development

```sh
cargo test && cargo clippy --all-targets -- -D warnings
npm --prefix ui run dev       # Vite on :5173, proxies /ws to `cardboard serve --port 4610`
```

## Known gaps

- Undo history lives in memory per session.
- Each edit sends the full document to the UI. Fine on localhost up to a few thousand layers; diffs would be the next step.
- No snapping guides, components, or raster export yet.
- Linux is the tested platform.

## Trademarks

Cardboard is an independent project. It is not affiliated with, endorsed by, or sponsored by Paper or any other design tool vendor. All code, text and artwork in this repository are original.
