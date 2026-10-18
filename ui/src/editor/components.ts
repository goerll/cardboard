// Components: a main is a node with `component: true`; an instance has `instance.componentId` and
// its layers carry ids `<instanceId>:<mainLayerId>` (see docs/components.md).
import { call } from '../api';
import type { CardboardDocument, DesignNode } from '../types';
import { edit, engineRef, select } from './store';

export type ComponentRole = 'main' | 'instance' | 'inner' | null;

export function componentRole(node: DesignNode): ComponentRole {
  if (node.component) return 'main';
  if (node.instance && !node.id.includes(':')) return 'instance';
  if (node.id.includes(':')) return 'inner';
  return null;
}

/** The layer in a main component that an instance layer copies (the deepest main for nested ones). */
export function sourceLayer(id: string) {
  return id.slice(id.lastIndexOf(':') + 1);
}

export function findNode(doc: CardboardDocument, id: string): { node: DesignNode; pageId: string } | null {
  const walk = (n: DesignNode): DesignNode | null => (n.id === id ? n : n.children.reduce<DesignNode | null>((f, c) => f ?? walk(c), null));
  for (const p of doc.pages)
    for (const a of p.artboards) {
      const node = walk(a);
      if (node) return { node, pageId: p.id };
    }
  return null;
}

export function instanceCount(doc: CardboardDocument, componentId: string) {
  let count = 0;
  const walk = (n: DesignNode) => {
    if (n.instance?.componentId === componentId && !n.id.includes(':')) count++;
    n.children.forEach(walk);
  };
  doc.pages.forEach((p) => p.artboards.forEach(walk));
  return count;
}

/** Select a layer anywhere in the file, switching pages if needed. */
export async function reveal(doc: CardboardDocument, id: string) {
  const hit = findNode(doc, id);
  if (!hit) return;
  if (hit.pageId !== doc.activePage) await call('set_active_page', { fileId: doc.id, pageId: hit.pageId });
  select([id]);
  requestAnimationFrame(() => engineRef.current?.zoomToFit(true));
}

export const createComponent = (ids: string[]) => edit(ids.map((nodeId) => ({ tool: 'create_component', args: { nodeId } })));
export const detachInstances = (ids: string[]) => edit([{ tool: 'detach_instance', args: { nodeIds: ids } }]);
export const resetOverrides = (ids: string[]) => edit([{ tool: 'reset_overrides', args: { nodeIds: ids } }]);
