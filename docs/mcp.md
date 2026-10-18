# MCP contract, version 0.1

MCP uses the official Rust SDK with newline-delimited stdio messages. Desktop clients launch a relay; standalone clients edit an explicitly named file. Mutations serialize through the same shared store used by the native UI.

## Tool surface

| Area | Tools |
| --- | --- |
| Guidance and files | `get_guide`, `list_resources`, `list_files`, `create_file`, `open_file`, `get_basic_info` |
| Read document | `get_selection`, `get_node_info`, `get_children`, `get_tree_summary`, `get_styles`, `find_nodes`, `get_document` |
| Pages and artboards | `create_page`, `rename_pages`, `create_artboard` |
| Edit nodes | `write_html`, `update_styles`, `set_text_content`, `rename_nodes`, `delete_nodes`, `duplicate_nodes`, `move_nodes` |
| Tokens | `get_tokens`, `create_tokens`, `set_tokens` |
| Export | `get_jsx`, `export_html` |
| Session and history | `finish_working_on_nodes`, `undo`, `redo` |

Inspect `tools/list` for authoritative JSON schemas. The implementation lives in `src/tools.rs`.

## Editing sequence

1. Call `get_guide` with `{"topic":"cardboard-design"}`.
2. Call `list_resources` and explicitly choose a `fileId`.
3. Read `get_basic_info`; inspect relevant children and styles before editing existing work.
4. Create an artboard or use an existing target.
5. Insert static HTML, refine styles, and export. Do not claim visual verification until an actual screenshot or UI inspection is available.

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
| `name` | case-insensitive substring of the layer name |
| `tag` | exact tag, e.g. `button`, `svg` |
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

## Guarantees and limits

- Explicit `fileId` is required by schemas for document tools. A relay does not implicitly pin to the currently visible tab. `open_file` changes the active tab; explicit targets remain independent.
- Batch commands are atomic: any invalid item rejects the whole batch. Read tools (`get_*`, `find_nodes`, `export_html`) may be mixed into a batch and see the edits before them; file tools and `undo`/`redo` must be called on their own. Successful edits validate the model and save before reporting success. Save errors roll back the model and history.
- Undo and redo apply to workspace history. Revisions and ID counters remain monotonic. History is limited to 100 in-memory snapshots.
- Imported designs cannot execute scripts. `write_html` parses with html5ever (the WHATWG HTML algorithm browsers use), so named entities, unquoted attributes, void elements without `/>` and optional end tags (`p`, `li`, `dt`, `dd`, table rows/cells) are accepted; `<div />` is still read as an empty element. An allowlist then maps unknown tags to `div`/`span`, drops unsupported attributes, SVG elements, `<style>`/`<link>`/`<meta>` and invalid CSS declarations, and reports each in `warnings`. Scripts, active embeds, event handlers, `javascript:` URLs, unclosed block elements at the end and truncated tags are errors.
- `write_html` takes `html` (up to 1 MiB) or an absolute `path` to a local `.html`/`.htm`/`.svg` file (up to 8 MiB), so large imports don't pass through the chat. `index` counts all children: a target's own text becomes a leading `Text` layer first, so index 0 inserts before it. `move_nodes` and `duplicate_nodes` follow the same rule.
- `get_styles` reports authored values. Width/height in basic info are CSS declarations, not measured geometry.
- `get_jsx` supports only `inline-styles`. `get_tokens` can return JSON, CSS, or a Tailwind theme block; this does not imply Tailwind JSX generation.
- `set_text_content` edits leaf text; it rejects nodes with children. Token renames rewrite exact `var(--name)` references, not every possible CSS fallback expression.
- `finish_working_on_nodes` acknowledges completion; presence markers are not implemented.
- Failures are MCP tool results with `isError`, preserving the connection so agents can inspect and retry. Protocol/schema errors remain protocol errors.

## Parity roadmap

Current tools cover the document core. Add screenshots, browser-computed styles and bounds, font discovery, raster export, and additional manipulation modes after the CEF integration is running. Do not advertise unsupported tools with fabricated responses.
