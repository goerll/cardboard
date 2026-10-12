# MCP contract, version 0.1

MCP uses the official Rust SDK with newline-delimited stdio messages. Desktop clients launch a relay; standalone clients edit an explicitly named file. Mutations serialize through the same shared store used by the native UI.

## Tool surface

| Area | Tools |
| --- | --- |
| Guidance and files | `get_guide`, `list_resources`, `create_file`, `open_file`, `get_basic_info` |
| Read document | `get_selection`, `get_node_info`, `get_children`, `get_tree_summary`, `get_styles` |
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

`write_html` accepts `insert-children` and `replace`. Replacement produces new identifiers. Duplication recursively assigns new IDs; reparenting retains IDs. IDs are scoped to a file and must not be inferred from examples.

Example style update:

```json
{
  "fileId": "file-1",
  "expectedRevision": 2,
  "updates": [{"nodeIds": ["NODE_ID_FROM_RESPONSE"], "styles": {"padding": "40px", "border": ""}}]
}
```

Style names accept CSS or camelCase spelling and are normalized to CSS names. An empty value removes an authored property. `expectedRevision` is optional; supplying the last read revision rejects stale edits. Revision numbers in examples are placeholders.

## Guarantees and limits

- Explicit `fileId` is required by schemas for document tools. A relay does not implicitly pin to the currently visible tab. `open_file` changes the active tab; explicit targets remain independent.
- Batch commands are atomic: any invalid item rejects the whole batch. Successful edits validate the model and save before reporting success. Save errors roll back the model and history.
- Undo and redo apply to workspace history. Revisions and ID counters remain monotonic. History is limited to 100 in-memory snapshots.
- Imported designs cannot execute scripts. The importer uses an XML-style static subset and requires closed tags. HTML entities supported by the parser include `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, and numeric references; use numeric references for others.
- `get_styles` reports authored values. Width/height in basic info are CSS declarations, not measured geometry.
- `get_jsx` supports only `inline-styles`. `get_tokens` can return JSON, CSS, or a Tailwind theme block; this does not imply Tailwind JSX generation.
- `set_text_content` edits leaf text; it rejects nodes with children. Token renames rewrite exact `var(--name)` references, not every possible CSS fallback expression.
- `finish_working_on_nodes` acknowledges completion; presence markers are not implemented.
- Failures are MCP tool results with `isError`, preserving the connection so agents can inspect and retry. Protocol/schema errors remain protocol errors.

## Parity roadmap

Current tools cover the document core. Add screenshots, browser-computed styles and bounds, font discovery, raster export, and additional manipulation modes after the CEF integration is running. Do not advertise unsupported tools with fabricated responses.
