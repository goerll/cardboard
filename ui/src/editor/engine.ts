// The canvas engine. Imperative on purpose: pointer moves never go through React.
//
// Designs render in a same-origin iframe with scripts disabled. The iframe ignores pointer events;
// a transparent overlay above it receives all input, hit-tests through iframe.contentDocument,
// and draws selection chrome. Because inspector popovers sit above the overlay in the same DOM,
// input can never "fall through" to the canvas.
import type { Operation } from '../api';
import type { CardboardDocument, DesignNode, Page } from '../types';
import { activePage, ancestors, artboardPositions, indexPage, isTextLeaf, type NodeIndex, sizeMode, SVG_TAGS, VOID_TAGS } from '../lib/tree';

export type Tool = 'select' | 'hand' | 'frame' | 'rect' | 'text';
export interface View {
  x: number;
  y: number;
  zoom: number;
}
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
export interface EngineCallbacks {
  select(ids: string[]): void;
  commit(ops: Operation[]): Promise<unknown>;
  insertHtml(parentId: string | 'root', html: string, index: number | undefined, then?: (ids: string[]) => void): void;
  createArtboard(rect: Rect, name: string, styles: Record<string, string>): void;
  view(view: View): void;
  toolDone(): void;
  contextMenu(x: number, y: number, id: string | null): void;
  textEditing(id: string | null): void;
  toast(message: string): void;
}

const SVG_NS = 'http://www.w3.org/2000/svg';
const HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const;
type Handle = (typeof HANDLES)[number];
const MIN_ZOOM = 0.02;
const MAX_ZOOM = 64;

const FRAME_CSS = `
*,*::before,*::after{box-sizing:border-box}
html,body{margin:0;padding:0;height:100%;overflow:hidden;background:transparent}
body{font-family:system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;-webkit-font-smoothing:antialiased}
#stage{position:absolute;left:0;top:0;transform-origin:0 0}
.cb-ab{position:absolute}
[contenteditable]{outline:none;cursor:text}
img{display:block}
`;

type Gesture =
  | { kind: 'pan'; sx: number; sy: number; vx: number; vy: number }
  | { kind: 'marquee'; sx: number; sy: number; x: number; y: number; scope: string | null; clickTarget: string | null; additive: boolean; base: string[] }
  | { kind: 'move'; sx: number; sy: number; ids: string[]; moved: boolean; dx: number; dy: number; mode: 'artboard' | 'absolute' | 'flow'; origin: Map<string, [number, number]>; drop: Drop | null; duplicate: boolean; deeper: string | null }
  | { kind: 'resize'; sx: number; sy: number; id: string; handle: Handle; w: number; h: number; left: number; top: number; abs: boolean; artboard: boolean; moved: boolean; styles: Record<string, string>; pos: [number, number] | null }
  | { kind: 'draw'; sx: number; sy: number; x: number; y: number; tool: Tool };

interface Drop {
  parent: string;
  index: number;
  line: Rect;
}

export class CanvasEngine {
  readonly host: HTMLElement;
  readonly iframe: HTMLIFrameElement;
  readonly overlay: HTMLDivElement;
  private ready = false;
  private fdoc!: Document;
  private stage!: HTMLDivElement;
  private tokenStyle!: HTMLStyleElement;

  doc: CardboardDocument | null = null;
  page: Page | null = null;
  index: NodeIndex | null = null;
  selection: string[] = [];
  private pressSelection: string[] = [];
  tool: Tool = 'select';
  view: View = { x: 80, y: 80, zoom: 0.5 };
  private hover: string | null = null;
  private els = new Map<string, Element>();
  private wrappers = new Map<string, HTMLDivElement>();
  private prev = new Map<string, DesignNode>();
  private previewed = new Set<string>();
  private gesture: Gesture | null = null;
  private space = false;
  private raf = 0;
  private settleTimer = 0;
  private editing: { id: string; el: HTMLElement; original: string } | null = null;
  private loadedPageKey = '';
  private fitPending = true;
  private resizeObserver: ResizeObserver;

  // overlay parts
  private hoverBox: HTMLDivElement;
  private selLayer: HTMLDivElement;
  private labelLayer: HTMLDivElement;
  private marquee: HTMLDivElement;
  private dropLine: HTMLDivElement;
  private dropBox: HTMLDivElement;
  private drawBox: HTMLDivElement;
  private labels = new Map<string, HTMLDivElement>();

  constructor(host: HTMLElement, private cb: EngineCallbacks) {
    this.host = host;
    this.iframe = document.createElement('iframe');
    this.iframe.className = 'cb-frame';
    this.iframe.setAttribute('sandbox', 'allow-same-origin');
    this.iframe.setAttribute('tabindex', '-1');
    this.iframe.srcdoc = `<!doctype html><html><head><meta charset="utf-8"><style>${FRAME_CSS}</style><style id="tokens"></style></head><body><div id="stage"></div></body></html>`;
    this.overlay = document.createElement('div');
    this.overlay.className = 'cb-overlay';
    const part = (cls: string) => {
      const d = document.createElement('div');
      d.className = cls;
      this.overlay.append(d);
      return d;
    };
    this.labelLayer = part('cb-labels');
    this.hoverBox = part('cb-hover');
    this.dropBox = part('cb-dropbox');
    this.selLayer = part('cb-sel');
    this.marquee = part('cb-marquee');
    this.dropLine = part('cb-dropline');
    this.drawBox = part('cb-draw');
    host.append(this.iframe, this.overlay);
    this.resizeObserver = new ResizeObserver(() => this.requestDraw());
    this.resizeObserver.observe(host);
    this.iframe.addEventListener('load', () => this.onFrameLoad());

    const o = this.overlay;
    o.addEventListener('pointerdown', (e) => this.onPointerDown(e));
    o.addEventListener('pointermove', (e) => this.onPointerMove(e));
    o.addEventListener('pointerup', (e) => this.onPointerUp(e));
    o.addEventListener('pointercancel', () => this.cancelGesture());
    o.addEventListener('lostpointercapture', () => {
      if (this.gesture) this.cancelGesture();
    });
    o.addEventListener('pointerleave', () => this.setHover(null));
    o.addEventListener('dblclick', (e) => this.onDoubleClick(e));
    o.addEventListener('contextmenu', (e) => this.onContextMenu(e));
    host.addEventListener('wheel', (e) => this.onWheel(e), { passive: false });
    window.addEventListener('keydown', this.onKey, true);
    window.addEventListener('keyup', this.onKey, true);
    window.addEventListener('blur', () => {
      this.space = false;
      this.updateCursor();
    });
  }

  destroy() {
    window.removeEventListener('keydown', this.onKey, true);
    window.removeEventListener('keyup', this.onKey, true);
    this.resizeObserver.disconnect();
    cancelAnimationFrame(this.raf);
    this.host.replaceChildren();
  }

  private onFrameLoad() {
    const d = this.iframe.contentDocument;
    if (!d) return;
    this.fdoc = d;
    this.stage = d.getElementById('stage') as HTMLDivElement;
    this.tokenStyle = d.getElementById('tokens') as HTMLStyleElement;
    // Images and fonts change layout after the fact.
    d.addEventListener('load', () => this.requestDraw(), true);
    d.fonts?.addEventListener?.('loadingdone', () => this.requestDraw());
    this.ready = true;
    this.applyView();
    if (this.doc) this.setDocument(this.doc, true);
  }

  // ───────────────────────────── document sync ─────────────────────────────

  setDocument(doc: CardboardDocument, force = false) {
    const pageChanged = !this.doc || this.doc.id !== doc.id || this.doc.activePage !== doc.activePage;
    this.doc = doc;
    this.page = activePage(doc);
    this.index = indexPage(this.page);
    this.selection = doc.selection.filter((id) => this.index!.nodes.has(id));
    if (!this.ready) return;
    const key = `${doc.id}:${this.page.id}`;
    if (pageChanged || force || key !== this.loadedPageKey) {
      this.loadedPageKey = key;
      this.stage.replaceChildren();
      this.els.clear();
      this.wrappers.clear();
      this.prev.clear();
      for (const l of this.labels.values()) l.remove();
      this.labels.clear();
      if (pageChanged) this.fitPending = true;
    }
    if (this.editing && !this.index.nodes.has(this.editing.id)) this.stopEditing(false);
    this.tokenStyle.textContent = `:root{${Object.entries(doc.tokens).map(([k, v]) => `${k}:${v}`).join(';')}}`;
    this.host.style.setProperty('--canvas-bg', this.page.background || '#282828');
    this.syncArtboards();
    this.previewed.clear();
    if (this.gesture?.kind === 'move' || this.gesture?.kind === 'resize') this.reapplyGesture();
    if (this.fitPending && this.page.artboards.length) {
      this.fitPending = false;
      requestAnimationFrame(() => this.zoomToFit(false));
    } else if (this.fitPending) {
      this.fitPending = false;
      this.setView({ x: this.host.clientWidth / 2 - 200, y: this.host.clientHeight / 2 - 200, zoom: 1 });
    }
    this.requestDraw();
  }

  private syncArtboards() {
    const page = this.page!;
    const positions = artboardPositions(page, this.measuredWidths());
    const seen = new Set<string>();
    let i = 0;
    for (const ab of page.artboards) {
      seen.add(ab.id);
      let wrap = this.wrappers.get(ab.id);
      if (!wrap) {
        wrap = this.fdoc.createElement('div');
        wrap.className = 'cb-ab';
        this.wrappers.set(ab.id, wrap);
      }
      const [x, y] = positions.get(ab.id)!;
      wrap.style.left = `${x}px`;
      wrap.style.top = `${y}px`;
      const el = this.syncNode(ab, false);
      if (el.parentNode !== wrap) wrap.replaceChildren(el);
      if (this.stage.children[i] !== wrap) this.stage.insertBefore(wrap, this.stage.children[i] ?? null);
      this.resizeObserver.observe(el);
      i++;
    }
    for (const [id, wrap] of this.wrappers) {
      if (!seen.has(id)) {
        wrap.remove();
        this.wrappers.delete(id);
      }
    }
    // Drop bookkeeping for nodes that no longer exist.
    for (const id of this.els.keys()) {
      if (!this.index!.nodes.has(id)) {
        this.els.delete(id);
        this.prev.delete(id);
      }
    }
  }

  private measuredWidths() {
    const m = new Map<string, { w: number; h: number }>();
    for (const [id] of this.wrappers) {
      const el = this.els.get(id) as HTMLElement | undefined;
      if (el?.offsetWidth) m.set(id, { w: el.offsetWidth, h: el.offsetHeight });
    }
    return m;
  }

  private syncNode(node: DesignNode, inSvg: boolean): Element {
    const svg = inSvg || node.tag === 'svg';
    const prev = this.prev.get(node.id);
    let el = this.els.get(node.id);
    if (!el || !prev || prev.tag !== node.tag) {
      el = svg && SVG_TAGS.has(node.tag) ? this.fdoc.createElementNS(SVG_NS, node.tag) : this.fdoc.createElement(node.tag);
      el.setAttribute('data-cb-id', node.id);
      this.els.set(node.id, el);
      this.applyAttributes(el, node, undefined);
      this.applyStyles(el, node);
      this.applyText(el, node);
    } else {
      if (!shallowEqual(prev.attributes, node.attributes)) this.applyAttributes(el, node, prev);
      if (this.previewed.has(node.id) || prev.visible !== node.visible || !shallowEqual(prev.styles, node.styles)) this.applyStyles(el, node);
      if (prev.text !== node.text) this.applyText(el, node);
    }
    this.prev.set(node.id, node);
    if (!VOID_TAGS.has(node.tag)) {
      let i = 0;
      for (const child of node.children) {
        const c = this.syncNode(child, svg);
        if (el.children[i] !== c) el.insertBefore(c, el.children[i] ?? null);
        i++;
      }
      while (el.children.length > node.children.length) el.lastElementChild!.remove();
    }
    return el;
  }

  private applyAttributes(el: Element, node: DesignNode, prev: DesignNode | undefined) {
    if (prev) for (const k of Object.keys(prev.attributes)) if (!(k in node.attributes)) el.removeAttribute(k);
    for (const [k, v] of Object.entries(node.attributes)) {
      try {
        el.setAttribute(k, v);
      } catch {
        /* invalid attribute names are ignored */
      }
    }
    if (node.tag === 'img') el.setAttribute('draggable', 'false');
  }

  private applyStyles(el: Element, node: DesignNode) {
    let css = Object.entries(node.styles)
      .map(([k, v]) => `${k}:${v}`)
      .join(';');
    if (!node.visible) css += ';display:none!important';
    el.setAttribute('style', css);
  }

  private applyText(el: Element, node: DesignNode) {
    if (this.editing?.id === node.id) return;
    const texts = [...el.childNodes].filter((n) => n.nodeType === 3);
    if (node.text) {
      if (texts.length) texts[0].nodeValue = node.text;
      else el.prepend(this.fdoc.createTextNode(node.text));
      for (const t of texts.slice(1)) t.remove();
    } else for (const t of texts) t.remove();
  }

  // ───────────────────────────── previews (inspector) ─────────────────────────────

  /** Live-apply styles without touching the document. The next sync restores truth. */
  preview(ids: string[], styles: Record<string, string>) {
    for (const id of ids) {
      const el = this.els.get(id) as HTMLElement | undefined;
      if (!el) continue;
      for (const [k, v] of Object.entries(styles)) {
        if (v === '') el.style.removeProperty(k);
        else el.style.setProperty(k, v);
      }
      this.previewed.add(id);
    }
    this.requestDraw();
  }
  restore(ids: string[]) {
    for (const id of ids) {
      const node = this.index?.nodes.get(id);
      const el = this.els.get(id);
      if (node && el) this.applyStyles(el, node);
      this.previewed.delete(id);
    }
    this.requestDraw();
  }
  previewPosition(id: string, x: number, y: number) {
    const w = this.wrappers.get(id);
    if (w) {
      w.style.left = `${x}px`;
      w.style.top = `${y}px`;
      this.requestDraw();
    }
  }

  // ───────────────────────────── view ─────────────────────────────

  setView(v: View, settle = true) {
    this.view = { ...v, zoom: Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, v.zoom)) };
    this.applyView(settle);
    this.cb.view(this.view);
  }
  private applyView(settle = true) {
    if (!this.ready) return;
    const { x, y, zoom } = this.view;
    this.stage.style.transform = `translate(${x}px, ${y}px) scale(${zoom})`;
    // Compositor-only while moving; re-raster crisply once the view settles.
    if (settle) {
      this.stage.style.willChange = 'transform';
      clearTimeout(this.settleTimer);
      this.settleTimer = window.setTimeout(() => {
        this.stage.style.willChange = '';
      }, 180);
    }
    this.requestDraw();
  }
  zoomAt(factor: number, cx = this.host.clientWidth / 2, cy = this.host.clientHeight / 2) {
    const { x, y, zoom } = this.view;
    const z = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * factor));
    const k = z / zoom;
    this.setView({ zoom: z, x: cx - (cx - x) * k, y: cy - (cy - y) * k });
  }
  zoomTo(z: number) {
    this.zoomAt(z / this.view.zoom);
  }
  /** World-space bounds of nodes (or all artboards). */
  private worldBounds(ids?: string[]): Rect | null {
    const list = ids?.length ? ids : this.page?.artboards.map((a) => a.id) ?? [];
    let r: Rect | null = null;
    for (const id of list) {
      const s = this.screenRect(id);
      if (!s) continue;
      const w = this.toWorld(s);
      r = r ? union(r, w) : w;
    }
    return r;
  }
  zoomToFit(selectionOnly = false) {
    const b = this.worldBounds(selectionOnly ? this.selection : undefined);
    if (!b) return;
    const pad = 64;
    const W = this.host.clientWidth;
    const H = this.host.clientHeight;
    const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.min((W - pad * 2) / b.w, (H - pad * 2) / b.h, selectionOnly ? 8 : 1)));
    this.setView({ zoom, x: (W - b.w * zoom) / 2 - b.x * zoom, y: (H - b.h * zoom) / 2 - b.y * zoom });
  }
  private toWorld(r: Rect): Rect {
    const { x, y, zoom } = this.view;
    return { x: (r.x - x) / zoom, y: (r.y - y) / zoom, w: r.w / zoom, h: r.h / zoom };
  }

  // ───────────────────────────── geometry ─────────────────────────────

  screenRect(id: string): Rect | null {
    const el = this.els.get(id);
    if (!el || !el.isConnected) return null;
    const r = el.getBoundingClientRect();
    if (!r.width && !r.height && !(el as HTMLElement).offsetParent) {
      if (!el.getClientRects().length) return null;
    }
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  }
  /** Unscaled geometry relative to the parent's border box (or the canvas for artboards). */
  geometry(id: string): Rect | null {
    const s = this.screenRect(id);
    if (!s || !this.index) return null;
    const z = this.view.zoom;
    const parent = this.index.parents.get(id);
    if (!parent) {
      const w = this.toWorld(s);
      return { x: Math.round(w.x), y: Math.round(w.y), w: round2(w.w), h: round2(w.h) };
    }
    const p = this.screenRect(parent);
    if (!p) return null;
    return { x: round2((s.x - p.x) / z), y: round2((s.y - p.y) / z), w: round2(s.w / z), h: round2(s.h / z) };
  }
  computed(id: string): CSSStyleDeclaration | null {
    const el = this.els.get(id);
    return el ? this.fdoc.defaultView!.getComputedStyle(el) : null;
  }

  // ───────────────────────────── hit testing ─────────────────────────────

  private local(e: { clientX: number; clientY: number }) {
    const r = this.host.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }
  /** Node path [artboard … deepest] under a host-local point, skipping locked/excluded subtrees. */
  private pathAt(x: number, y: number, exclude?: Set<string>): string[] {
    if (!this.ready || !this.index) return [];
    for (const el of this.fdoc.elementsFromPoint(x, y)) {
      const hit = el.closest('[data-cb-id]');
      if (!hit) continue;
      const id = hit.getAttribute('data-cb-id')!;
      if (!this.index.nodes.has(id)) continue;
      const path = [...ancestors(this.index, id).reverse(), id];
      if (exclude && path.some((p) => exclude.has(p))) continue;
      if (path.some((p) => this.index!.nodes.get(p)!.locked)) {
        const firstLocked = path.findIndex((p) => this.index!.nodes.get(p)!.locked);
        if (firstLocked === 0) return [];
        return path.slice(0, firstLocked);
      }
      return path;
    }
    return [];
  }
  /** Which node a plain click would select (Figma-style contextual depth). */
  private target(path: string[], deep: boolean): string | null {
    if (!path.length) return null;
    if (deep) return path[path.length - 1];
    for (const s of this.selection) if (path.includes(s)) return s;
    for (const s of this.selection) {
      for (const a of ancestors(this.index!, s)) {
        const i = path.indexOf(a);
        if (i >= 0) return path[Math.min(i + 1, path.length - 1)];
      }
    }
    return path.length > 1 ? path[1] : path[0];
  }
  private labelAt(x: number, y: number): string | null {
    for (const [id, label] of this.labels) {
      const r = label.getBoundingClientRect();
      const h = this.host.getBoundingClientRect();
      if (x >= r.left - h.left && x <= r.right - h.left && y >= r.top - h.top && y <= r.bottom - h.top) return id;
    }
    return null;
  }
  private handleAt(x: number, y: number): Handle | null {
    if (this.selection.length !== 1) return null;
    const id = this.selection[0];
    if (this.index?.nodes.get(id)?.locked) return null;
    const r = this.screenRect(id);
    if (!r) return null;
    const pts: Record<Handle, [number, number]> = {
      nw: [r.x, r.y],
      n: [r.x + r.w / 2, r.y],
      ne: [r.x + r.w, r.y],
      e: [r.x + r.w, r.y + r.h / 2],
      se: [r.x + r.w, r.y + r.h],
      s: [r.x + r.w / 2, r.y + r.h],
      sw: [r.x, r.y + r.h],
      w: [r.x, r.y + r.h / 2],
    };
    // Small boxes keep their body for dragging: grab zones only extend outward.
    const small = r.w < 28 || r.h < 28;
    const inside = x > r.x && x < r.x + r.w && y > r.y && y < r.y + r.h;
    if (small && inside) return null;
    for (const h of ['nw', 'ne', 'se', 'sw'] as Handle[]) if (Math.abs(x - pts[h][0]) <= 6 && Math.abs(y - pts[h][1]) <= 6) return h;
    if (small) return null;
    const edge = 4;
    const inX = x > r.x + 6 && x < r.x + r.w - 6;
    const inY = y > r.y + 6 && y < r.y + r.h - 6;
    if (inX && Math.abs(y - r.y) <= edge) return 'n';
    if (inX && Math.abs(y - (r.y + r.h)) <= edge) return 's';
    if (inY && Math.abs(x - r.x) <= edge) return 'w';
    if (inY && Math.abs(x - (r.x + r.w)) <= edge) return 'e';
    return null;
  }

  // ───────────────────────────── input ─────────────────────────────

  private onKey = (e: KeyboardEvent) => {
    if (e.code !== 'Space') return;
    const t = e.target as HTMLElement;
    if (t && (t.isContentEditable || /INPUT|TEXTAREA|SELECT/.test(t.tagName))) return;
    if (this.editing) return;
    if (e.type === 'keydown') {
      e.preventDefault();
      if (!this.space) {
        this.space = true;
        this.updateCursor();
      }
    } else {
      this.space = false;
      this.updateCursor();
    }
  };

  private onWheel(e: WheelEvent) {
    if ((e.target as HTMLElement).closest('.cb-overlay, .cb-frame') === null && e.target !== this.host) return;
    e.preventDefault();
    const p = this.local(e);
    if (e.ctrlKey || e.metaKey) {
      // Pinch gestures arrive as ctrl+wheel with small deltas; mouse wheels with large ones.
      // Clamp so one mouse-wheel notch is ~1.25x instead of a jump.
      const delta = Math.max(-50, Math.min(50, e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY));
      this.zoomAt(Math.exp(-delta * 0.0045), p.x, p.y);
    } else {
      let dx = e.deltaX;
      let dy = e.deltaY;
      if (e.deltaMode === 1) {
        dx *= 16;
        dy *= 16;
      }
      if (e.shiftKey && !dx) {
        dx = dy;
        dy = 0;
      }
      this.setView({ ...this.view, x: this.view.x - dx, y: this.view.y - dy });
    }
  }

  private onPointerDown(e: PointerEvent) {
    this.pressSelection = this.selection.slice();
    if (this.editing) {
      const p = this.local(e);
      const path = this.pathAt(p.x, p.y);
      if (path.includes(this.editing.id)) return;
      this.stopEditing(true);
    }
    (document.activeElement as HTMLElement | null)?.blur?.();
    const p = this.local(e);
    if (e.button === 1 || (e.button === 0 && (this.space || this.tool === 'hand'))) {
      e.preventDefault();
      this.overlay.setPointerCapture(e.pointerId);
      this.gesture = { kind: 'pan', sx: p.x, sy: p.y, vx: this.view.x, vy: this.view.y };
      this.updateCursor();
      return;
    }
    if (e.button !== 0) return;
    this.overlay.setPointerCapture(e.pointerId);

    if (this.tool === 'frame' || this.tool === 'rect' || this.tool === 'text') {
      this.gesture = { kind: 'draw', sx: p.x, sy: p.y, x: p.x, y: p.y, tool: this.tool };
      return;
    }

    const handle = this.handleAt(p.x, p.y);
    if (handle) {
      this.beginResize(this.selection[0], handle, p);
      return;
    }
    const label = this.labelAt(p.x, p.y);
    if (label) {
      if (e.shiftKey) this.cb.select(toggle(this.selection, label));
      else if (!this.selection.includes(label)) this.cb.select([label]);
      const ids = this.selection.includes(label) && !e.shiftKey ? this.selection : [label];
      this.beginMove(ids.filter((id) => this.index!.parents.get(id) === null), p, e.altKey);
      return;
    }
    const path = this.pathAt(p.x, p.y);
    const deep = e.ctrlKey || e.metaKey;
    const target = this.target(path, deep);
    if (!target) {
      this.gesture = { kind: 'marquee', sx: p.x, sy: p.y, x: p.x, y: p.y, scope: null, clickTarget: null, additive: e.shiftKey, base: e.shiftKey ? this.selection : [] };
      return;
    }
    const isAb = this.index!.parents.get(target) === null;
    if (isAb && !this.selection.includes(target) && !deep) {
      // Dragging on an artboard's empty area draws a marquee inside it; a click selects it.
      this.gesture = { kind: 'marquee', sx: p.x, sy: p.y, x: p.x, y: p.y, scope: target, clickTarget: target, additive: e.shiftKey, base: e.shiftKey ? this.selection : [] };
      return;
    }
    let ids: string[];
    if (e.shiftKey) {
      ids = toggle(this.selection, target);
      this.cb.select(ids);
      if (!ids.includes(target)) return;
    } else if (this.selection.includes(target)) {
      ids = this.selection;
    } else {
      ids = [target];
      this.cb.select(ids);
    }
    this.selection = ids;
    this.beginMove(ids, p, e.altKey);
    // Pressing on an already-selected container keeps it draggable; releasing without a drag
    // drills one level into the child under the pointer (Figma-style).
    if (this.gesture?.kind === 'move' && !e.shiftKey && !deep) {
      const i = path.indexOf(target);
      if (i >= 0 && i < path.length - 1 && this.selection.length === 1) this.gesture.deeper = path[i + 1];
    }
  }

  private beginMove(ids: string[], p: { x: number; y: number }, duplicate: boolean) {
    const idx = this.index!;
    ids = ids.filter((id) => !idx.nodes.get(id)?.locked);
    if (!ids.length) return;
    const first = ids[0];
    const artboard = idx.parents.get(first) === null;
    const comp = this.computed(first);
    const abs = !artboard && comp && (comp.position === 'absolute' || comp.position === 'fixed');
    const mode = artboard ? 'artboard' : abs ? 'absolute' : 'flow';
    // Mixed selections move only the nodes compatible with the first one.
    ids = ids.filter((id) => {
      if (mode === 'artboard') return idx.parents.get(id) === null;
      if (idx.parents.get(id) === null) return false;
      const c = this.computed(id);
      const isAbs = c?.position === 'absolute' || c?.position === 'fixed';
      return mode === 'absolute' ? isAbs : !isAbs;
    });
    const origin = new Map<string, [number, number]>();
    const positions = artboardPositions(this.page!, this.measuredWidths());
    for (const id of ids) {
      if (mode === 'artboard') origin.set(id, positions.get(id)!);
      else if (mode === 'absolute') {
        const el = this.els.get(id) as HTMLElement;
        origin.set(id, [el.offsetLeft, el.offsetTop]);
      } else origin.set(id, [0, 0]);
    }
    this.gesture = { kind: 'move', sx: p.x, sy: p.y, ids, moved: false, dx: 0, dy: 0, mode, origin, drop: null, duplicate, deeper: null };
  }

  private beginResize(id: string, handle: Handle, p: { x: number; y: number }) {
    const el = this.els.get(id) as HTMLElement;
    const comp = this.computed(id)!;
    const artboard = this.index!.parents.get(id) === null;
    const pos = artboard ? artboardPositions(this.page!, this.measuredWidths()).get(id)! : null;
    this.gesture = {
      kind: 'resize',
      sx: p.x,
      sy: p.y,
      id,
      handle,
      w: el.offsetWidth || this.screenRect(id)!.w / this.view.zoom,
      h: el.offsetHeight || this.screenRect(id)!.h / this.view.zoom,
      left: el.offsetLeft,
      top: el.offsetTop,
      abs: comp.position === 'absolute' || comp.position === 'fixed',
      artboard,
      moved: false,
      styles: {},
      pos,
    };
  }

  private onPointerMove(e: PointerEvent) {
    const p = this.local(e);
    const g = this.gesture;
    if (!g) {
      if (this.tool !== 'select' || this.space) return this.setHover(null);
      const handle = this.handleAt(p.x, p.y);
      this.overlay.style.cursor = handle ? cursorFor(handle) : '';
      if (handle) return this.setHover(null);
      const label = this.labelAt(p.x, p.y);
      this.setHover(label ?? this.target(this.pathAt(p.x, p.y), e.ctrlKey || e.metaKey));
      return;
    }
    const z = this.view.zoom;
    switch (g.kind) {
      case 'pan':
        this.setView({ ...this.view, x: g.vx + p.x - g.sx, y: g.vy + p.y - g.sy });
        break;
      case 'marquee':
        g.x = p.x;
        g.y = p.y;
        if (Math.hypot(p.x - g.sx, p.y - g.sy) > 3) {
          g.clickTarget = null;
          this.cb.select(this.marqueeHits(g));
        }
        this.requestDraw();
        break;
      case 'draw':
        g.x = p.x;
        g.y = p.y;
        this.requestDraw();
        break;
      case 'move': {
        if (!g.moved && Math.hypot(p.x - g.sx, p.y - g.sy) < 3) return;
        if (!g.moved) {
          g.moved = true;
          this.setHover(null);
        }
        let dx = (p.x - g.sx) / z;
        let dy = (p.y - g.sy) / z;
        if (e.shiftKey) {
          if (Math.abs(dx) > Math.abs(dy)) dy = 0;
          else dx = 0;
        }
        g.dx = dx;
        g.dy = dy;
        if (g.mode === 'flow') g.drop = this.findDrop(p, new Set(g.ids), e.ctrlKey || e.metaKey);
        this.reapplyGesture();
        break;
      }
      case 'resize': {
        let dx = (p.x - g.sx) / z;
        let dy = (p.y - g.sy) / z;
        let w = g.w;
        let h = g.h;
        let mx = 0;
        let my = 0;
        if (g.handle.includes('e')) w = g.w + dx;
        if (g.handle.includes('w')) {
          w = g.w - dx;
          mx = g.w - Math.max(1, w);
        }
        if (g.handle.includes('s')) h = g.h + dy;
        if (g.handle.includes('n')) {
          h = g.h - dy;
          my = g.h - Math.max(1, h);
        }
        if (e.shiftKey && g.handle.length === 2) {
          const ratio = g.w / g.h;
          if (Math.abs(w / g.w) > Math.abs(h / g.h)) h = w / ratio;
          else w = h * ratio;
          if (g.handle.includes('w')) mx = g.w - w;
          if (g.handle.includes('n')) my = g.h - h;
        }
        w = Math.max(1, Math.round(w));
        h = Math.max(1, Math.round(h));
        g.moved = true;
        const styles: Record<string, string> = {};
        if (g.handle !== 'n' && g.handle !== 's') styles.width = `${w}px`;
        if (g.handle !== 'e' && g.handle !== 'w') styles.height = `${h}px`;
        if (g.abs) {
          if (mx) styles.left = `${Math.round(g.left + mx)}px`;
          if (my) styles.top = `${Math.round(g.top + my)}px`;
        }
        g.styles = styles;
        dx = mx;
        dy = my;
        this.reapplyGesture();
        break;
      }
    }
  }

  /** Re-apply the in-progress gesture's preview (also after a document push mid-gesture). */
  private reapplyGesture() {
    const g = this.gesture;
    if (!g) return;
    if (g.kind === 'move' && g.moved) {
      for (const id of g.ids) {
        const [ox, oy] = g.origin.get(id)!;
        if (g.mode === 'artboard') this.previewPosition(id, Math.round(ox + g.dx), Math.round(oy + g.dy));
        else if (g.mode === 'absolute') this.preview([id], { left: `${Math.round(ox + g.dx)}px`, top: `${Math.round(oy + g.dy)}px` });
        else this.preview([id], { translate: `${g.dx}px ${g.dy}px`, 'z-index': '2147483000', position: 'relative', opacity: '0.85', 'pointer-events': 'none' });
      }
    }
    if (g.kind === 'resize' && g.moved) {
      if (g.artboard && g.pos && (g.handle.includes('w') || g.handle.includes('n'))) {
        const w = parseFloat(g.styles.width ?? `${g.w}`);
        const h = parseFloat(g.styles.height ?? `${g.h}`);
        this.previewPosition(g.id, g.pos[0] + (g.handle.includes('w') ? g.w - w : 0), g.pos[1] + (g.handle.includes('n') ? g.h - h : 0));
      }
      this.preview([g.id], g.styles);
    }
    this.requestDraw();
  }

  private onPointerUp(e: PointerEvent) {
    const g = this.gesture;
    this.gesture = null;
    if (this.overlay.hasPointerCapture(e.pointerId)) this.overlay.releasePointerCapture(e.pointerId);
    this.updateCursor();
    if (!g) return;
    const p = this.local(e);
    switch (g.kind) {
      case 'marquee':
        if (g.clickTarget) this.cb.select(g.additive ? toggle(this.selection, g.clickTarget) : [g.clickTarget]);
        else if (Math.hypot(p.x - g.sx, p.y - g.sy) <= 3) this.cb.select(g.additive ? this.selection : []);
        break;
      case 'move':
        if (g.moved) this.commitMove(g);
        else if (g.deeper && this.wasSelected(g.ids)) this.cb.select([g.deeper]);
        break;
      case 'resize':
        if (g.moved) this.commitResize(g);
        break;
      case 'draw':
        this.commitDraw(g);
        break;
    }
    this.requestDraw();
  }

  cancelGesture() {
    const g = this.gesture;
    this.gesture = null;
    if (!g) return;
    if (g.kind === 'move') {
      for (const id of g.ids) {
        if (g.mode === 'artboard') {
          const [x, y] = g.origin.get(id)!;
          this.previewPosition(id, x, y);
        } else this.restore([id]);
      }
    }
    if (g.kind === 'resize') {
      this.restore([g.id]);
      if (g.pos) this.previewPosition(g.id, g.pos[0], g.pos[1]);
    }
    this.updateCursor();
    this.requestDraw();
  }

  /** True when the ids were already the selection before this press (not just selected by it). */
  private wasSelected(ids: string[]) {
    return this.pressSelection.length === ids.length && ids.every((id) => this.pressSelection.includes(id));
  }

  private commitMove(g: Extract<Gesture, { kind: 'move' }>) {
    if (g.duplicate) return this.commitDuplicate(g);
    const ops: Operation[] = [];
    if (g.mode === 'artboard') {
      ops.push({
        tool: 'set_positions',
        args: { updates: g.ids.map((id) => ({ nodeId: id, x: Math.round(g.origin.get(id)![0] + g.dx), y: Math.round(g.origin.get(id)![1] + g.dy) })) },
      });
    } else if (g.mode === 'absolute') {
      ops.push({
        tool: 'update_styles',
        args: { updates: g.ids.map((id) => ({ nodeIds: [id], styles: { left: `${Math.round(g.origin.get(id)![0] + g.dx)}px`, top: `${Math.round(g.origin.get(id)![1] + g.dy)}px` } })) },
      });
    } else {
      this.restore(g.ids);
      if (!g.drop) return;
      const { parent, index } = g.drop;
      // Indices count siblings that stay; adjust for moved nodes that were before the slot.
      const siblings = this.index!.nodes.get(parent)!.children.map((c) => c.id);
      let i = index;
      const moves = g.ids.map((id) => {
        const cur = siblings.indexOf(id);
        const target = cur >= 0 && cur < index ? i - 1 : i;
        if (!(cur >= 0 && cur < index)) i++;
        return { nodeId: id, parentId: parent, index: Math.max(0, target) };
      });
      ops.push({ tool: 'move_nodes', args: { moves } });
    }
    this.cb.commit(ops).then((r) => {
      if (r === undefined) this.cancelGestureLeftovers(g.ids);
    });
  }
  /** Alt-drag: originals snap back, copies land where the drag ended (one undo step). */
  private commitDuplicate(g: Extract<Gesture, { kind: 'move' }>) {
    const nodes: Record<string, unknown>[] = [];
    if (g.mode === 'artboard') {
      for (const id of g.ids) {
        const [x, y] = g.origin.get(id)!;
        this.previewPosition(id, x, y);
        nodes.push({ id, parentId: 'root', x: Math.round(x + g.dx), y: Math.round(y + g.dy) });
      }
    } else if (g.mode === 'absolute') {
      this.restore(g.ids);
      for (const id of g.ids) {
        const [x, y] = g.origin.get(id)!;
        nodes.push({ id, styles: { left: `${Math.round(x + g.dx)}px`, top: `${Math.round(y + g.dy)}px` } });
      }
    } else {
      this.restore(g.ids);
      if (!g.drop) return;
      g.ids.forEach((id, k) => nodes.push({ id, parentId: g.drop!.parent, index: g.drop!.index + k }));
    }
    this.cb.commit([{ tool: 'duplicate_nodes', args: { nodes } }]).then((r: any) => {
      const copies = (r?.duplicates ?? r?.results?.[0]?.duplicates ?? []).map((d: any) => d.nodeId);
      if (copies.length) this.cb.select(copies);
      else if (this.page) this.syncArtboards();
    });
  }
  private cancelGestureLeftovers(ids: string[]) {
    this.restore(ids);
    if (this.page) this.syncArtboards();
  }

  private commitResize(g: Extract<Gesture, { kind: 'resize' }>) {
    const ops: Operation[] = [];
    const styles = { ...g.styles };
    const parent = this.index!.parents.get(g.id);
    if (parent) {
      const pc = this.computed(parent);
      if (pc && pc.display.includes('flex')) styles['flex-shrink'] = '0';
    }
    ops.push({ tool: 'update_styles', args: { updates: [{ nodeIds: [g.id], styles }] } });
    if (g.artboard && g.pos && (g.handle.includes('w') || g.handle.includes('n'))) {
      const w = parseFloat(g.styles.width ?? `${g.w}`);
      const h = parseFloat(g.styles.height ?? `${g.h}`);
      ops.push({
        tool: 'set_positions',
        args: { updates: [{ nodeId: g.id, x: Math.round(g.pos[0] + (g.handle.includes('w') ? g.w - w : 0)), y: Math.round(g.pos[1] + (g.handle.includes('n') ? g.h - h : 0)) }] },
      });
    }
    this.cb.commit(ops).then((r) => {
      if (r === undefined) this.cancelGestureLeftovers([g.id]);
    });
  }

  private commitDraw(g: Extract<Gesture, { kind: 'draw' }>) {
    const z = this.view.zoom;
    const x0 = Math.min(g.sx, g.x);
    const y0 = Math.min(g.sy, g.y);
    let w = Math.abs(g.x - g.sx) / z;
    let h = Math.abs(g.y - g.sy) / z;
    const clicked = w < 4 && h < 4;
    const path = this.pathAt(g.sx, g.sy);
    const containerId = [...path].reverse().find((id) => this.isContainer(id)) ?? null;
    this.cb.toolDone();
    if (g.tool === 'text') {
      if (!containerId) {
        const world = this.toWorld({ x: x0, y: y0, w: 0, h: 0 });
        this.cb.createArtboard({ x: world.x, y: world.y, w: 320, h: 120 }, 'Text', { 'background-color': 'transparent', padding: '0px' });
        return;
      }
      const index = this.insertionIndex(containerId, g.sx, g.sy);
      this.cb.insertHtml(containerId, '<p layer-name="Text" style="margin:0;font-size:16px;line-height:1.4">Text</p>', index, (ids) => {
        if (ids[0]) requestAnimationFrame(() => this.startEditing(ids[0], true));
      });
      return;
    }
    if (clicked) {
      w = g.tool === 'frame' ? 400 : 100;
      h = g.tool === 'frame' ? 300 : 100;
    }
    w = Math.round(w);
    h = Math.round(h);
    if (!containerId) {
      const world = this.toWorld({ x: clicked ? g.sx : x0, y: clicked ? g.sy : y0, w, h });
      this.cb.createArtboard(
        { x: Math.round(world.x), y: Math.round(world.y), w, h },
        g.tool === 'frame' ? 'Frame' : 'Rectangle',
        g.tool === 'frame' ? { 'background-color': '#ffffff' } : { 'background-color': '#d9d9d9' },
      );
      return;
    }
    const index = this.insertionIndex(containerId, g.sx, g.sy);
    const html =
      g.tool === 'frame'
        ? `<div layer-name="Frame" style="display:flex;flex-direction:column;flex-shrink:0;width:${w}px;height:${h}px;background-color:#ffffff"></div>`
        : `<div layer-name="Rectangle" style="flex-shrink:0;width:${w}px;height:${h}px;background-color:#d9d9d9"></div>`;
    this.cb.insertHtml(containerId, html, index);
  }

  private isContainer(id: string) {
    const n = this.index!.nodes.get(id);
    if (!n || VOID_TAGS.has(n.tag) || SVG_TAGS.has(n.tag)) return false;
    if (n.children.length) return true;
    if (n.text) return false;
    return !isTextLeaf(n);
  }

  /** Child slot nearest to a host point along the container's main axis. */
  private insertionIndex(containerId: string, x: number, y: number, exclude?: Set<string>) {
    const n = this.index!.nodes.get(containerId)!;
    const comp = this.computed(containerId);
    const row = !!comp && comp.display.includes('flex') && comp.flexDirection.startsWith('row');
    let i = 0;
    for (const c of n.children) {
      if (exclude?.has(c.id)) {
        continue;
      }
      const r = this.screenRect(c.id);
      if (!r) {
        i++;
        continue;
      }
      const mid = row ? r.x + r.w / 2 : r.y + r.h / 2;
      if ((row ? x : y) > mid) i++;
      else break;
    }
    // Translate "index among non-excluded" back to an index in the full child list.
    let full = 0;
    let seen = 0;
    for (const c of n.children) {
      if (seen === i) break;
      if (!exclude?.has(c.id)) seen++;
      full++;
    }
    return full;
  }

  private findDrop(p: { x: number; y: number }, moving: Set<string>, deep: boolean): Drop | null {
    const path = this.pathAt(p.x, p.y, moving);
    // While the pointer stays inside the original parent, dragging reorders among siblings;
    // nesting into a sibling container needs Ctrl. Outside it, the deepest container wins.
    const home = this.index!.parents.get(moving.values().next().value!) ?? null;
    const parent = home && !deep && path.includes(home) ? home : [...path].reverse().find((id) => this.isContainer(id));
    if (!parent) return null;
    const index = this.insertionIndex(parent, p.x, p.y, moving);
    const n = this.index!.nodes.get(parent)!;
    const comp = this.computed(parent);
    const row = !!comp && comp.display.includes('flex') && comp.flexDirection.startsWith('row');
    const pr = this.screenRect(parent)!;
    const siblings = n.children.filter((c) => !moving.has(c.id));
    const before = n.children[index - 1] && !moving.has(n.children[index - 1].id) ? n.children[index - 1] : siblings.filter((c) => n.children.indexOf(c) < index).pop();
    const after = siblings.find((c) => n.children.indexOf(c) >= index);
    const br = before ? this.screenRect(before.id) : null;
    const ar = after ? this.screenRect(after.id) : null;
    let line: Rect;
    if (row) {
      const x = br && ar ? (br.x + br.w + ar.x) / 2 : br ? br.x + br.w + 2 : ar ? ar.x - 2 : pr.x + 8;
      const ref = ar ?? br;
      line = { x: x - 1, y: ref ? ref.y : pr.y + 8, w: 2, h: ref ? ref.h : Math.max(16, pr.h - 16) };
    } else {
      const y = br && ar ? (br.y + br.h + ar.y) / 2 : br ? br.y + br.h + 2 : ar ? ar.y - 2 : pr.y + 8;
      const ref = ar ?? br;
      line = { x: ref ? ref.x : pr.x + 8, y: y - 1, w: ref ? ref.w : Math.max(16, pr.w - 16), h: 2 };
    }
    return { parent, index, line };
  }

  private marqueeHits(g: Extract<Gesture, { kind: 'marquee' }>) {
    const r = normRect(g.sx, g.sy, g.x, g.y);
    const candidates = g.scope ? this.index!.nodes.get(g.scope)!.children.map((c) => c.id) : this.page!.artboards.map((a) => a.id);
    const hits = candidates.filter((id) => {
      const s = this.screenRect(id);
      return s && !this.index!.nodes.get(id)!.locked && intersects(r, s);
    });
    return g.additive ? [...new Set([...g.base, ...hits])] : hits;
  }

  private onDoubleClick(e: MouseEvent) {
    if (this.tool !== 'select') return;
    const p = this.local(e);
    if (this.labelAt(p.x, p.y)) return;
    const path = this.pathAt(p.x, p.y);
    // Single clicks already drill one level per release; a double-click on text goes straight to editing.
    const leaf = path[path.length - 1];
    const node = leaf ? this.index!.nodes.get(leaf) : null;
    if (!node || !isTextLeaf(node)) return;
    if (this.selection.length !== 1 || this.selection[0] !== leaf) this.cb.select([leaf]);
    this.selection = [leaf];
    this.startEditing(leaf, false);
  }

  private onContextMenu(e: MouseEvent) {
    e.preventDefault();
    const p = this.local(e);
    const label = this.labelAt(p.x, p.y);
    const target = label ?? this.target(this.pathAt(p.x, p.y), false);
    if (target && !this.selection.includes(target)) this.cb.select([target]);
    this.cb.contextMenu(e.clientX, e.clientY, target);
  }

  // ───────────────────────────── text editing ─────────────────────────────

  startEditing(id: string, selectAll: boolean) {
    const el = this.els.get(id) as HTMLElement | undefined;
    const node = this.index?.nodes.get(id);
    if (!el || !node || node.locked || !isTextLeaf(node)) return;
    this.stopEditing(true);
    this.editing = { id, el, original: node.text };
    el.contentEditable = 'plaintext-only';
    el.spellcheck = false;
    this.iframe.style.pointerEvents = 'auto';
    this.overlay.classList.add('editing');
    el.focus();
    const sel = this.fdoc.getSelection();
    if (sel) {
      const range = this.fdoc.createRange();
      range.selectNodeContents(el);
      if (!selectAll) range.collapse(false);
      sel.removeAllRanges();
      sel.addRange(range);
    }
    el.addEventListener('keydown', this.onEditKey);
    el.addEventListener('input', this.onEditInput);
    el.addEventListener('blur', this.onEditBlur);
    this.fdoc.addEventListener('pointerdown', this.onEditOutside, true);
    this.cb.textEditing(id);
    this.requestDraw();
  }
  private onEditKey = (e: KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === 'Escape' || (e.key === 'Enter' && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      this.stopEditing(true);
    }
  };
  private onEditInput = () => this.requestDraw();
  private onEditBlur = () => setTimeout(() => this.editing && this.fdoc.activeElement !== this.editing.el && this.stopEditing(true));
  private onEditOutside = (e: PointerEvent) => {
    if (this.editing && !this.editing.el.contains(e.target as globalThis.Node)) this.stopEditing(true);
  };
  stopEditing(commit: boolean) {
    const ed = this.editing;
    if (!ed) return;
    this.editing = null;
    ed.el.removeEventListener('keydown', this.onEditKey);
    ed.el.removeEventListener('input', this.onEditInput);
    ed.el.removeEventListener('blur', this.onEditBlur);
    this.fdoc.removeEventListener('pointerdown', this.onEditOutside, true);
    ed.el.removeAttribute('contenteditable');
    this.iframe.style.pointerEvents = '';
    this.overlay.classList.remove('editing');
    const text = (ed.el.innerText ?? ed.el.textContent ?? '').replace(/\n$/, '');
    this.fdoc.getSelection()?.removeAllRanges();
    if (commit && text !== ed.original) {
      this.cb.commit([{ tool: 'set_text_content', args: { updates: [{ nodeId: ed.id, textContent: text }] } }]);
    } else {
      const node = this.index?.nodes.get(ed.id);
      if (node) {
        ed.el.textContent = '';
        this.applyText(ed.el, node);
      }
    }
    this.cb.textEditing(null);
    this.requestDraw();
  }
  get isEditingText() {
    return !!this.editing;
  }

  // ───────────────────────────── drawing the overlay ─────────────────────────────

  setTool(tool: Tool) {
    this.tool = tool;
    this.updateCursor();
  }
  setSelection(ids: string[]) {
    this.selection = ids;
    this.requestDraw();
  }
  private setHover(id: string | null) {
    if (this.hover === id) return;
    this.hover = id;
    this.requestDraw();
  }
  private updateCursor() {
    const c =
      this.gesture?.kind === 'pan'
        ? 'grabbing'
        : this.space || this.tool === 'hand'
          ? 'grab'
          : this.tool === 'text'
            ? 'text'
            : this.tool === 'frame' || this.tool === 'rect'
              ? 'crosshair'
              : '';
    this.overlay.style.cursor = c;
  }
  requestDraw() {
    if (!this.raf) this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.draw();
    });
  }

  private draw() {
    if (!this.ready || !this.index || !this.page) return;
    const g = this.gesture;
    const moving = g?.kind === 'move' && g.moved;
    // Artboard labels
    const seen = new Set<string>();
    for (const ab of this.page.artboards) {
      seen.add(ab.id);
      let label = this.labels.get(ab.id);
      if (!label) {
        label = document.createElement('div');
        label.className = 'cb-label';
        this.labelLayer.append(label);
        this.labels.set(ab.id, label);
      }
      if (label.textContent !== ab.name) label.textContent = ab.name;
      const r = this.screenRect(ab.id);
      if (!r) {
        label.style.display = 'none';
        continue;
      }
      label.style.display = '';
      label.style.transform = `translate(${Math.round(r.x)}px, ${Math.round(r.y - 20)}px)`;
      label.style.maxWidth = `${Math.max(24, r.w)}px`;
      label.classList.toggle('selected', this.selection.includes(ab.id));
      label.classList.toggle('hovered', this.hover === ab.id);
    }
    for (const [id, l] of this.labels) if (!seen.has(id)) (l.remove(), this.labels.delete(id));

    // Hover
    const hr = this.hover && !this.selection.includes(this.hover) && !g ? this.screenRect(this.hover) : null;
    place(this.hoverBox, hr);

    // Selection
    const boxes: string[] = [];
    const single = this.selection.length === 1;
    let bounds: Rect | null = null;
    for (const id of this.selection) {
      const r = this.screenRect(id);
      if (!r) continue;
      bounds = bounds ? union(bounds, r) : r;
      if (!single) boxes.push(`<div class="cb-box" style="${rectCss(r)}"></div>`);
    }
    if (bounds && !this.editing) {
      const locked = single && this.index.nodes.get(this.selection[0])?.locked;
      let html = `<div class="cb-box main${locked ? ' locked' : ''}" style="${rectCss(bounds)}">`;
      if (single && !locked && !moving && bounds.w >= 8 && bounds.h >= 8) for (const h of HANDLES) if (h.length === 2) html += `<span class="cb-handle ${h}"></span>`;
      html += '</div>';
      html += `<div class="cb-dims" style="left:${bounds.x + bounds.w / 2}px;top:${bounds.y + bounds.h + 8}px">${this.dimsLabel(bounds)}</div>`;
      this.selLayer.innerHTML = boxes.join('') + html;
    } else if (bounds && this.editing) {
      this.selLayer.innerHTML = `<div class="cb-box main editing" style="${rectCss(bounds)}"></div>`;
    } else this.selLayer.innerHTML = '';

    // Marquee / draw / drop
    place(this.marquee, g?.kind === 'marquee' && !g.clickTarget ? normRect(g.sx, g.sy, g.x, g.y) : null);
    place(this.drawBox, g?.kind === 'draw' && g.tool !== 'text' ? normRect(g.sx, g.sy, g.x, g.y) : null);
    const drop = g?.kind === 'move' && g.mode === 'flow' ? g.drop : null;
    place(this.dropLine, drop?.line ?? null);
    place(this.dropBox, drop ? this.screenRect(drop.parent) : null);
  }

  private dimsLabel(b: Rect) {
    const z = this.view.zoom;
    const w = Math.round(b.w / z);
    const h = Math.round(b.h / z);
    if (this.selection.length !== 1) return `${w} × ${h}`;
    const n = this.index!.nodes.get(this.selection[0])!;
    const pid = this.index!.parents.get(n.id);
    const pc = pid ? this.computed(pid) : null;
    const flex = !!pc && pc.display.includes('flex');
    const row = flex && pc!.flexDirection.startsWith('row');
    const mode = (k: 'width' | 'height') => ({ fit: 'Fit ', fill: 'Fill ', fixed: '' })[sizeMode(n.styles, k, flex, row)];
    return `${mode('width')}${w} × ${mode('height')}${h}`;
  }
}

function shallowEqual(a: Record<string, string>, b: Record<string, string>) {
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  for (const k of ka) if (a[k] !== b[k]) return false;
  return true;
}
function toggle(list: string[], id: string) {
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}
function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}
function normRect(x1: number, y1: number, x2: number, y2: number): Rect {
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
}
function intersects(a: Rect, b: Rect) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}
function rectCss(r: Rect) {
  return `transform:translate(${r.x}px,${r.y}px);width:${r.w}px;height:${r.h}px`;
}
function place(el: HTMLElement, r: Rect | null) {
  if (!r) {
    if (el.style.display !== 'none') el.style.display = 'none';
    return;
  }
  el.style.display = 'block';
  el.style.transform = `translate(${r.x}px,${r.y}px)`;
  el.style.width = `${r.w}px`;
  el.style.height = `${r.h}px`;
}
function round2(n: number) {
  return Math.round(n * 100) / 100;
}
function cursorFor(h: Handle) {
  return h === 'n' || h === 's' ? 'ns-resize' : h === 'e' || h === 'w' ? 'ew-resize' : h === 'nw' || h === 'se' ? 'nwse-resize' : 'nesw-resize';
}
