import type { CardboardDocument, DesignNode, Page } from '../types';

export const ARTBOARD_GAP = 80;

export function activePage(doc: CardboardDocument): Page {
  return doc.pages.find((p) => p.id === doc.activePage) ?? doc.pages[0];
}

export interface NodeIndex {
  nodes: Map<string, DesignNode>;
  parents: Map<string, string | null>;
  /** Top-level artboard id for each node. */
  roots: Map<string, string>;
  depth: Map<string, number>;
}

const indexCache = new WeakMap<Page, NodeIndex>();
export function indexPage(page: Page): NodeIndex {
  const cached = indexCache.get(page);
  if (cached) return cached;
  const index: NodeIndex = { nodes: new Map(), parents: new Map(), roots: new Map(), depth: new Map() };
  const visit = (n: DesignNode, parent: string | null, root: string, depth: number) => {
    index.nodes.set(n.id, n);
    index.parents.set(n.id, parent);
    index.roots.set(n.id, root);
    index.depth.set(n.id, depth);
    for (const c of n.children) visit(c, n.id, root, depth + 1);
  };
  for (const a of page.artboards) visit(a, null, a.id, 0);
  indexCache.set(page, index);
  return index;
}

export function ancestors(index: NodeIndex, id: string): string[] {
  const out: string[] = [];
  for (let p = index.parents.get(id); p; p = index.parents.get(p)) out.push(p);
  return out;
}

export function isArtboard(index: NodeIndex, id: string) {
  return index.parents.get(id) === null;
}

export function px(value: string | undefined): number | null {
  if (!value) return null;
  const m = /^(-?\d*\.?\d+)px$/.exec(value.trim());
  return m ? parseFloat(m[1]) : null;
}

/** Artboard canvas positions; unpositioned artboards flow left to right after the previous one. */
export function artboardPositions(page: Page, measured?: Map<string, { w: number; h: number }>) {
  const out = new Map<string, [number, number]>();
  let cursor = 0;
  for (const a of page.artboards) {
    const w = measured?.get(a.id)?.w ?? px(a.styles.width) ?? 400;
    const pos = a.position ?? [cursor, 0];
    out.set(a.id, pos);
    cursor = Math.max(cursor, pos[0] + w + ARTBOARD_GAP);
  }
  return out;
}

export const TEXT_TAGS = new Set(['p', 'span', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'label', 'a', 'strong', 'em', 'small', 'code', 'pre', 'li', 'button', 'b', 'i', 'u', 's', 'mark', 'blockquote', 'figcaption', 'td', 'th', 'sup', 'sub', 'time', 'dt', 'dd', 'summary', 'legend', 'caption', 'abbr', 'cite', 'q', 'kbd', 'samp', 'var', 'del', 'ins', 'dfn']);
export const VOID_TAGS = new Set(['img', 'br', 'hr', 'input', 'wbr', 'source', 'col']);
export const SVG_TAGS = new Set(['svg', 'path', 'circle', 'rect', 'line', 'polyline', 'polygon', 'g', 'ellipse', 'text', 'defs', 'linearGradient', 'radialGradient', 'stop', 'clipPath', 'mask', 'tspan']);

export type LayerKind = 'frame' | 'flex-row' | 'flex-col' | 'grid' | 'text' | 'image' | 'svg' | 'shape' | 'rect';
export function layerKind(n: DesignNode, artboard = false): LayerKind {
  if (n.tag === 'img') return 'image';
  if (n.tag === 'svg') return 'svg';
  if (SVG_TAGS.has(n.tag)) return 'shape';
  const display = n.styles.display;
  if (display === 'flex' || display === 'inline-flex') return n.styles['flex-direction']?.startsWith('column') ? 'flex-col' : 'flex-row';
  if (display === 'grid') return 'grid';
  if (n.children.length === 0 && (n.text || TEXT_TAGS.has(n.tag))) return 'text';
  if (artboard) return 'frame';
  return n.children.length ? 'frame' : 'rect';
}

export function isTextLeaf(n: DesignNode) {
  return n.children.length === 0 && !VOID_TAGS.has(n.tag) && !SVG_TAGS.has(n.tag) && (n.text !== '' || TEXT_TAGS.has(n.tag));
}

/** Escape for HTML attribute/text contexts. */
export function escapeHtml(text: string) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

export function nodeToHtml(n: DesignNode): string {
  const attrs = Object.entries(n.attributes).map(([k, v]) => ` ${k}="${escapeHtml(v)}"`).join('');
  const style = Object.entries(n.styles).map(([k, v]) => `${k}:${v}`).join(';');
  const open = `<${n.tag} layer-name="${escapeHtml(n.name)}"${attrs}${style ? ` style="${escapeHtml(style)}"` : ''}`;
  if (VOID_TAGS.has(n.tag)) return `${open} />`;
  return `${open}>${escapeHtml(n.text)}${n.children.map(nodeToHtml).join('')}</${n.tag}>`;
}

export type SizeMode = 'fixed' | 'fit' | 'fill';
/** How a node sizes along an axis, given whether its parent is a flex row/column. */
export function sizeMode(styles: Record<string, string>, axis: 'width' | 'height', parentFlex: boolean, parentRow: boolean): SizeMode {
  const v = styles[axis];
  const main = axis === 'width' ? parentRow : parentFlex && !parentRow;
  if (v === '100%') return 'fill';
  if (main && styles.flex && /^1( |$)/.test(styles.flex)) return 'fill';
  if (!main && parentFlex && styles['align-self'] === 'stretch') return 'fill';
  if (v && v !== 'auto' && v !== 'fit-content') return 'fixed';
  return 'fit';
}

/** Name shown in the UI: explicit names win; unnamed text layers show their text. */
export function displayName(n: DesignNode): string {
  if (n.name && n.name !== n.tag) return n.name;
  const text = n.text.trim().replace(/\s+/g, ' ');
  if (text) return text.length > 48 ? `${text.slice(0, 48)}…` : text;
  return n.name || n.tag;
}
