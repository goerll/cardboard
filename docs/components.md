# Components

Reusable layers: build a button once, place it many times, and change every copy by editing the
original. The model stays plain HTML/CSS nodes so agents, export and the canvas need no special
cases.

## Model

* **Main component**: any node with `"component": true`. It lives in the tree like any other
  layer (usually on a "Components" page or next to the design). Its id is the component id.
* **Instance**: a node with `"instance": {"componentId": "node-4", "overrides": {...}}`. Its
  subtree is a *materialized* copy of the main: real nodes that are saved, rendered and exported
  like any other. The instance root keeps its own id; each inner layer gets the id
  `<instanceId>:<mainLayerId>`, e.g. `node-12:node-5`. Ids are stable across main edits, so
  selections and agent references survive. Since no layer carries `<instanceId>:<mainRootId>`, tools accept it as an
  alias for the instance root (`node-12:node-4` means `node-12`), and an unknown
  `<instanceId>:...` id fails with an error that spells out the naming.
* **Overrides** are keyed by the main layer's id and hold only what the instance changed:
  `name`, `text`, `styles`, `attributes` (an empty string means "removed here"), `visible`,
  `locked`, and `markup` for an icon layer whose icon was swapped. The instance root's name, lock, visibility and canvas position are its own.

```json
{ "id": "node-12", "tag": "button", "name": "Cancel button",
  "instance": { "componentId": "node-4",
                "overrides": { "node-4": { "styles": { "background": "#eee" } },
                               "node-5": { "text": "Cancel" } } },
  "children": [ { "id": "node-12:node-5", "tag": "span", "text": "Cancel", ... } ] }
```

## How edits flow

Every editing tool runs through `tools::apply`, which snapshots the mains, runs the tool, then
calls `components::reconcile`:

1. For each instance that existed before the edit, diff its layers against the main as it was
   *before* the edit. The diff is the new override set. This is how ordinary tools
   (`update_styles`, `set_text_content`, `set_attributes`, `set_node_state`, `rename_nodes`, and
   any future bulk-edit tool) create overrides without knowing about components. A bulk edit
   (a `find_nodes`-style `query`, or ids listed together) that gives the main and an instance the
   same new value is a main edit: fields that only repeat the main's new value are dropped from
   the overrides unless the instance had already overridden them. The diff alone misses an
   instance layer set to the main's *old* value by the same edit that changes the main (hide a
   layer in the main but keep it on one instance), so the name, text, visibility and lock that
   `rename_nodes`, `set_text_content` and `set_node_state` give an instance layer by id are
   merged into its overrides too.
2. Instances whose main no longer exists are detached (they keep their look).
3. Mains are ordered so nested components update first, then every instance is rebuilt from its
   main plus its overrides. Main edits therefore reach all instances immediately, and overridden
   properties keep the instance's value.

Icons are single layers, so swapping one is an edit, not a structure change:
`insert_icon {nodeId, icon}` on an instance's icon layer (e.g. `node-12:node-6`) replaces its
markup, `viewBox` and `data-icon`, keeps its size and color unless the call sets them, and folds
into a `markup` override plus attribute overrides. A feature-row component can then show a
different icon in each copy; swapping or restyling the main's icon still reaches copies that
didn't override it, and `reset_overrides` brings the main's icon back.

Structure belongs to the main: adding, removing, moving or replacing layers inside an instance is
rejected with an error that points to the alternatives (edit the main, hide the layer with
`set_node_state`, or `detach_instance`). A component that would contain an instance of itself is
rejected. The whole step is one transaction, so undo/redo and `batch` work as before: an agent's
undo of a main edit restores the instances too. `import_image` with `parentId` goes through the
same path. Claims follow the tree: a claim on an instance covers its inner layers, and
`insert_instance` claims the new instance (not its parent). Editing a main still updates instances
another agent has claimed, since those changes come from the component rather than the edit.

## Tools

| Tool | Args | Result |
| --- | --- | --- |
| `create_component` | `{nodeId, name?, moveToPage?}` | `{componentId}` (same as nodeId). Not allowed on instances or their layers. With `moveToPage` (page id, or a name like `"Components"`, matched case-insensitively and created if missing) the main moves there as a top-level frame and a new instance takes its place, in one undo step; adds `{pageId, instanceId}` |
| `insert_instance` | `{componentId, parentId, pageId?, index?, x?, y?}` | `{nodeId, pageId}`. `parentId: "root"` places a top-level frame on `pageId` (default active page) |
| `detach_instance` | `{nodeIds}` | plain layers with fresh ids that look identical; nested instances stay linked |
| `reset_overrides` | `{nodeIds}` | clears all overrides of the given instances |

Reads: `get_basic_info` lists `components: [{componentId, name, pageId, instances}]`;
`get_node_info` adds `isComponent` or `instanceOf` + `overrides`; `get_tree_summary` marks lines
with `(component)` / `(instance of node-4)`; `find_nodes` rows add `isComponent`, `instanceOf`, or
`inInstance` (the instance an inner layer belongs to, so editing it makes an override). `get_jsx` and `export_html` emit the expanded
layers, so exports are plain HTML with no component markup.

Copies: `duplicate_nodes`/`duplicate_page` of an instance stays linked (with its overrides); a
copy of a main is a plain layer, so there is exactly one main per component. `duplicate_nodes`
and `move_nodes` accept `pageId` to copy or move layers, including whole artboards, to another
page.

## Deleting a main

Instances of a deleted main are detached in the same step: they become plain layers that look
exactly as before. Undo restores the main and the links.

## UI

Layers show mains with the component icon and instances with a diamond, both in violet; layers
inside an instance get a violet icon. The Inspector has a Component section: "Create component"
on plain layers, instance count on mains, and Main / Reset / Detach on instances. The context
menu has the same actions plus "Move to <page>" for artboards. Shortcuts: Ctrl Alt K creates a
component, Ctrl Alt B detaches.

## Not done yet

* Variants (a component set with properties). The likely shape is a `variant` key on the
  instance mapping to a named override set stored on the main.
* Swapping an instance to another component, and "push overrides to main".
* Instances are stored expanded, so files grow with each instance (about the size of the main).
