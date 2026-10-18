# MCP contract, version 0.1

MCP uses the official Rust SDK with newline-delimited stdio messages. Desktop clients launch a relay; standalone clients edit an explicitly named file. Mutations serialize through the same shared store used by the native UI.

## Connection and hosting

`cardboard mcp` connects to whoever owns the MCP socket. Exactly one process owns a library at a time, the one bound to the socket, so there is never a second writer:

- **App running:** the relay forwards to it (`connection.mode` is `app`, or `serve` for `cardboard serve`).
- **Nothing running:** the relay binds the socket and hosts the library in-process (`headless`), with an editor URL in `connection.editorUrl`. Other agents' relays connect to it and share the session.
- **App starts later:** the app's bind asks the headless host to hand over; the host saves, stops accepting commands and drops its agents. Every relay reconnects to the app and replays the MCP `initialize` handshake, so the agent's session continues. The app binds the socket before reading the library, so it never reads files mid-write.
- **App quits or restarts:** relays wait about a second for it, then one of them hosts headless again.

Requests in flight during a switch return a tool error asking the agent to re-read and retry, because they may or may not have applied. `--no-fallback` restores the old behavior (fail when no app is running). The relay names its session in `initialize` (`capabilities.experimental["cardboard/session"].id`) so claims and undo authorship survive reconnects; claims themselves live in memory and reset when the owner changes.

Auto-launching the desktop app was considered and rejected as the default: it fails in SSH, CI and headless sessions, surprises people with a window they didn't ask for, and is slow to start. Hosting headless works everywhere, and the app can still join at any moment.

Control lines (`{"cardboard":"status"}`, `{"cardboard":"handover"}`) sent as the first line on the socket are answered with one JSON line and never reach MCP. `cardboard doctor` uses `status` and checks the binary on PATH, the running owner, socket and library paths, Claude Code's MCP config and Chromium.

## Tool surface

| Area | Tools |
| --- | --- |
| Guidance and files | `get_guide`, `list_resources`, `list_files`, `create_file`, `open_file`, `get_basic_info` |
| Read document | `get_selection`, `get_node_info`, `get_children`, `get_tree_summary`, `get_styles`, `find_nodes`, `get_document` |
| Pages and artboards | `create_page`, `rename_pages`, `create_artboard` |
| Edit nodes | `write_html`, `update_styles`, `set_text_content`, `rename_nodes`, `delete_nodes`, `duplicate_nodes`, `move_nodes` |
| Components | `create_component`, `insert_instance`, `detach_instance`, `reset_overrides` (see `docs/components.md`) |
| Tokens | `get_tokens`, `create_tokens`, `set_tokens` |
| Icons | `search_icons`, `insert_icon` |
| Images | `import_image`, `search_images` |
| Fonts | `add_font`, `remove_font`, `list_fonts` |
| Export | `get_jsx`, `export_html` |
| See and measure | `screenshot`, `get_bounds` |
| Session and history | `start_working_on_nodes`, `finish_working_on_nodes`, `undo`, `redo` |

Inspect `tools/list` for authoritative JSON schemas. The implementation lives in `src/tools.rs`.

## Editing sequence

1. Call `get_guide` with `{"topic":"cardboard-design"}`.
2. Call `list_resources` and explicitly choose a `fileId`.
3. Read `get_basic_info`; inspect relevant children and styles before editing existing work.
4. Create an artboard or use an existing target.
5. Insert static HTML one visual group at a time; after each, call `screenshot` on it and look, and use `get_bounds` to check overflow and alignment. Claim only the visual checks you actually made.

Example arguments to `create_artboard`:

```json
{
  "fileId": "file-1",
  "name": "Desktop",
  "styles": {"width": "1440px", "height": "900px", "backgroundColor": "#f7f7f5"}
}
```

Use the returned `nodeId` in `write_html`:

```json
{
  "fileId": "file-1",
  "targetNodeId": "NODE_ID_FROM_RESPONSE",
  "mode": "insert-children",
  "html": "<div style=\"display:flex;flex-direction:column;gap:16px;padding:32px\"><h1 style=\"margin:0;font-size:40px\">Hello, Cardboard</h1><p style=\"margin:0;color:#555\">HTML is the design.</p></div>"
}
```

`write_html` accepts `insert-children`, `insert-before`, `insert-after` and `replace`. Replacement produces new identifiers. Duplication recursively assigns new IDs; reparenting retains IDs. IDs are scoped to a file and must not be inferred from examples.

Example style update:

```json
{
  "fileId": "file-1",
  "expectedRevision": 2,
  "updates": [{"nodeIds": ["NODE_ID_FROM_RESPONSE"], "styles": {"padding": "40px", "border": ""}}]
}
```

Style names accept CSS or camelCase spelling and are normalized to CSS names. An empty value removes an authored property. `expectedRevision` is optional; supplying the last read revision rejects stale edits, and the error names the current revision so you can re-read and retry. Revision numbers in examples are placeholders.

## Finding and bulk-editing nodes

`find_nodes` searches the whole file, one page (`pageId`) or one subtree (`nodeId`, which includes that node). Every filter given must match:

| Filter | Matches |
| --- | --- |
| `text` | case-insensitive substring of the node's own text |
| `name` | case-insensitive substring of the layer name, or of an icon layer's icon name (`lucide:arrow`) |
| `tag` | exact tag, e.g. `button`, `svg`; `icon` matches icon layers (results include their `icon` name) |
| `styles` | per property (CSS or camelCase, `*` for any property): a value for an exact match (case and whitespace insensitive), `true`/`false` for present/absent, or `{"contains": "..."}` |

```json
{"fileId": "file-1", "pageId": "page-1", "styles": {"background-color": "#000", "*": {"contains": "var(--color-accent)"}}, "limit": 20}
```

It returns `{"nodes": [{"id", "name", "tag", "pageId", "artboardId", "text"?, "styles"?}], "total", "truncated"}`; `styles` holds only the queried properties. `limit` defaults to 50 (max 500).

`update_styles` and `set_attributes` items, and `delete_nodes` itself, accept a `query` object (the same filters) in place of ids. The whole call is one undo step and reports `matched` and `changed` node counts (`count` for deletes). A query must include a filter or a `nodeId` subtree, so it can never silently match the whole file.

```json
{"fileId": "file-1", "updates": [{"query": {"tag": "button", "styles": {"border-radius": "4px"}}, "styles": {"borderRadius": "var(--radius-md)"}}]}
```

## Scoped, bounded reads

Reads stay small by default so they fit an agent's context. `maxChars` sets the budget (default 50000); `0` turns it off.

- `get_tree_summary`: one `nodeId`, or every artboard of `pageId` (default: the active page), to `depth` (default 3).
- `export_html`: one artboard or node (`nodeId`), one page (`pageId`), or the active page. `get_jsx` exports one `nodeId`. Both truncate over budget, append a `[truncated: ...]` marker and set `truncated` and `totalChars`.
- `get_document`: one `nodeId`, one `pageId`, or the whole file; `depth` replaces deeper children with an `omittedChildren` count. JSON is not truncated: over budget it returns an error saying how to narrow.
- `list_files` returns file and artboard metadata; per-artboard preview HTML only with `previews: true`.

Node ids may be given bare: `581` and `"581"` mean `node-581` (and `pageId: 2` means `page-2`) anywhere a tool takes an id.

## Icons

Icons come from [Iconify](https://iconify.design) (Lucide, Phosphor `ph`, Tabler, Heroicons,
Material Symbols and ~200 more sets), named `set:name`.

- `search_icons {query, sets?, limit?}` returns `{icons:["lucide:arrow-right", ...], total, source}`.
  Offline it lists matching icons from the local cache (`source: "cache"`) or fails with a clear
  error.
- `insert_icon {fileId, icon, targetNodeId, mode?, index?, size?, strokeWidth?, color?, name?}`
  places one icon like `write_html` (default `insert-children`) and returns `{nodeId}`.
- In `write_html`, `<icon name="lucide:arrow-right" size="16" stroke-width="1.5" color="var(--accent)" />`
  does the same inline; `style`, `layer-name`, `hidden` and ordinary attributes apply too.

An icon is ONE layer: an `<svg>` node with `viewBox`, `data-icon="set:name"`, `aria-hidden`,
`width`/`height`/`flex-shrink:0` styles, and its inner SVG stored in `Node.markup`. Color defaults
to `currentColor`, so `update_styles` `color` recolors it. `export_html` and `get_jsx` emit the
full SVG (JSX with React attribute names), and an exported `<svg data-icon>` pasted back into
`write_html` becomes one icon layer again.

Fetched icons are cached on disk (`$CARDBOARD_ICON_CACHE`, else the platform cache directory
under `cardboard/icons`), so repeat use and offline work need no network. Downloads happen
before the edit takes the document lock and never during rendering. Uncached icons fail with an
error instead of inserting anything. Icon markup is parsed into an allowlist of SVG shape
elements and attributes and re-serialized: scripts, `foreignObject`, animation, event handlers,
text, and any non-local `href` or `url()` are rejected. Documents whose stored markup is not in
that canonical form fail validation.

## Guarantees and limits

- Explicit `fileId` is required by schemas for document tools. A relay does not implicitly pin to the currently visible tab. `open_file` changes the active tab; explicit targets remain independent.
- Batch commands are atomic: any invalid item rejects the whole batch. Read tools (`get_*` other than `get_bounds`, `find_nodes`, `export_html`) may be mixed into a batch and see the edits before them; file tools and `undo`/`redo` must be called on their own. Successful edits validate the model and save before reporting success. Save errors roll back the model and history.
- Undo and redo apply to per-file history. Revisions and ID counters remain monotonic. History is limited to 200 in-memory snapshots. An agent can only undo or redo a step it made itself; if the latest step belongs to the person or another agent, `undo` fails and the agent should fix forward. The person can undo anything. Selective undo of an agent's older edits beneath someone else's is not implemented.
- Imported designs cannot execute scripts. `write_html` parses with html5ever (the WHATWG HTML algorithm browsers use), so named entities, unquoted attributes, void elements without `/>` and optional end tags (`p`, `li`, `dt`, `dd`, table rows/cells) are accepted; `<div />` is still read as an empty element. An allowlist then maps unknown tags to `div`/`span`, drops unsupported attributes, SVG elements, `<style>`/`<link>`/`<meta>` and invalid CSS declarations, and reports each in `warnings`. Scripts, active embeds, event handlers, `javascript:` URLs, unclosed block elements at the end and truncated tags are errors.
- `write_html` takes `html` (up to 1 MiB) or an absolute `path` to a local `.html`/`.htm`/`.svg` file (up to 8 MiB), so large imports don't pass through the chat. `index` counts all children: a target's own text becomes a leading `Text` layer first, so index 0 inserts before it. `move_nodes` and `duplicate_nodes` follow the same rule.
- `get_styles` reports authored values. Width/height in basic info are CSS declarations, not measured geometry; use `get_bounds` for measured boxes and computed styles.
- `screenshot` and `get_bounds` render the live in-memory document (not the saved file) in a headless Chromium-based browser, launched per call with a throwaway profile and driven over the DevTools protocol. The page holds just the node's artboard, positioned at 0,0 with the editor's frame CSS, the file's tokens and fonts, and stored images inlined; the viewport is the artboard's px size (1440x900 otherwise). The browser is `CARDBOARD_BROWSER` if set, else Chrome, Chromium, Edge or Brave from standard install locations and `PATH`; without one, both tools fail with an actionable error. Screenshots come back as MCP image content (PNG; JPEG above ~3.5 MB), capped at 4096 px per edge and 4 MP, and can also be written to an absolute `path`. A call takes about a second. Neither tool runs inside `batch`.
- `get_jsx` supports only `inline-styles`. `get_tokens` can return JSON, CSS, or a Tailwind theme block; this does not imply Tailwind JSX generation.
- `set_text_content` edits leaf text; it rejects nodes with children. Token renames rewrite exact `var(--name)` references, not every possible CSS fallback expression.
- Multi-agent claims: editing a layer claims it for that agent, as does `start_working_on_nodes`; `write_html` into a container claims only the new layers. While claimed, other agents get a tool error if they edit the layer or anything inside it, or delete, replace or move a layer containing it (including deleting its page). Edits targeted by `query` are checked against the layers the query matches, and claim them. Claims expire 2 minutes after the agent's last edit in the file and are released by `finish_working_on_nodes` (given layers and their descendants, or all of the agent's layers when `nodeIds` is omitted). The editor shows claimed layers with the agent's name. The person in the editor is never blocked.
- Images live in a content-addressed asset store and are referenced as `/asset/<sha256>.<ext>`. `import_image` stores one from `url`, `path`, `dataUri` or a `search_images` `stockId` and returns that `src`; with `parentId` it also inserts a sized `<img>`. Base64 `data:image/...` values written by any tool are moved into the store as part of the same edit. `export_html` inlines stored assets as data URIs by default (`assets: "files"` links them as `file://` URLs instead).
- `search_images` uses Unsplash when `UNSPLASH_ACCESS_KEY` (or the library setting `unsplashAccessKey`) is set and Openverse otherwise. Unsplash photos stay hotlinked and the download endpoint is called when one is placed, per Unsplash's API guidelines; Openverse photos are stored locally. Both keep attribution in `alt`/`data-attribution`.
- `add_font` downloads a Google Fonts family (WOFF2, latin and latin-ext subsets by default) or takes a local `path`/`dataUri` font file into the asset store and records `@font-face` rules in `Document.fonts`; the canvas, dashboard thumbnails and `export_html` all load them, so the font renders on any machine. `get_basic_info` lists document fonts; `list_fonts` adds generic families and the families installed on this machine. `remove_font` is undoable; `add_font` is too but cannot run inside `batch`.
- Network tools (`import_image` with `url` or `stockId`, `search_images`, `add_font` from Google Fonts) download before taking the editor lock and fail with a clear message offline. Nothing downloads while rendering.
- Failures are MCP tool results with `isError`, preserving the connection so agents can inspect and retry. Protocol/schema errors remain protocol errors.

## Parity roadmap

Current tools cover the document core plus screenshots, measured bounds, icons, images and fonts. Raster export and additional manipulation modes are still to come. Do not advertise unsupported tools with fabricated responses.
