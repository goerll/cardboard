import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Code2, Frame, Hand, ImagePlus, MousePointer2, PanelLeft, Redo2, SmilePlus, Square, Type, Undo2, X } from 'lucide-react';
import { call, getState, request, subscribe } from '../api';
import type { CardboardDocument } from '../types';
import { activePage, indexPage, isTextLeaf, nodeToHtml } from '../lib/tree';
import { CanvasEngine, type Tool } from './engine';
import { copyHtml, Inspector, pickImage } from './Inspector';
import { LeftPanel } from './LeftPanel';
import { edit, editorState, engineRef, select, setEditor, toast, useEditor } from './store';
import { Popover, Select } from '../ui/controls';

export function Editor({ fileId, doc, canUndo, canRedo }: { fileId: string; doc: CardboardDocument; canUndo: boolean; canRedo: boolean }) {
  const panels = useEditor((s) => s.panels);
  return (
    <div className={`editor ${panels ? '' : 'no-panels'}`}>
      {panels && <LeftPanel doc={doc} />}
      <ToolRail canUndo={canUndo} canRedo={canRedo} />
      <Canvas fileId={fileId} />
      {panels && <Inspector doc={doc} />}
      <ContextMenu doc={doc} />
      <ExportDialog doc={doc} />
      <InsertHtmlDialog doc={doc} />
      <IconDialog />
      <ShortcutsDialog />
      <Shortcuts doc={doc} />
    </div>
  );
}

const TOOLS: { tool: Tool; icon: React.ReactNode; title: string }[] = [
  { tool: 'select', icon: <MousePointer2 size={16} />, title: 'Select (V)' },
  { tool: 'hand', icon: <Hand size={16} />, title: 'Hand (H, or hold Space)' },
];
const CREATE: { tool: Tool; icon: React.ReactNode; title: string }[] = [
  { tool: 'frame', icon: <Frame size={16} />, title: 'Frame (F)' },
  { tool: 'rect', icon: <Square size={16} />, title: 'Rectangle (R)' },
  { tool: 'text', icon: <Type size={16} />, title: 'Text (T)' },
];

function ToolRail({ canUndo, canRedo }: { canUndo: boolean; canRedo: boolean }) {
  const tool = useEditor((s) => s.tool);
  const panels = useEditor((s) => s.panels);
  const btn = (t: (typeof TOOLS)[number]) => (
    <button key={t.tool} type="button" className={`rail-btn ${tool === t.tool ? 'active' : ''}`} title={t.title} onClick={() => setTool(t.tool)}>
      {t.icon}
    </button>
  );
  return (
    <nav className="rail">
      {!panels && (
        <button type="button" className="rail-btn" title="Show panels (Ctrl .)" onClick={() => setEditor({ panels: true })}>
          <PanelLeft size={16} />
        </button>
      )}
      {TOOLS.map(btn)}
      <div className="rail-sep" />
      {CREATE.map(btn)}
      <button type="button" className="rail-btn" title="Image (Shift Ctrl K)" onClick={insertImage}>
        <ImagePlus size={16} />
      </button>
      <button type="button" className="rail-btn" title="Icon" onClick={() => setEditor({ iconsOpen: true })}>
        <SmilePlus size={16} />
      </button>
      <button type="button" className="rail-btn" title="Insert HTML" onClick={() => setEditor({ insertHtmlOpen: true })}>
        <Code2 size={16} />
      </button>
      <div className="rail-sep" />
      <button type="button" className="rail-btn" title="Undo (Ctrl Z)" disabled={!canUndo} onClick={undo}>
        <Undo2 size={16} />
      </button>
      <button type="button" className="rail-btn" title="Redo (Ctrl Shift Z)" disabled={!canRedo} onClick={redo}>
        <Redo2 size={16} />
      </button>
    </nav>
  );
}

function setTool(tool: Tool) {
  setEditor({ tool });
  engineRef.current?.setTool(tool);
}
const history = (dir: 'undo' | 'redo') => {
  const f = engineRef.fileId;
  const d = f ? getState().docs[f] : null;
  if (d && (dir === 'undo' ? d.canUndo : d.canRedo)) call(dir, { fileId: f });
};
const undo = () => history('undo');
const redo = () => history('redo');

/** Parent and index for inserting next to the current selection. */
function insertionTarget(): { parent: string; index?: number } | null {
  const fileId = engineRef.fileId;
  const doc = fileId ? getState().docs[fileId]?.doc : null;
  if (!doc) return null;
  const index = indexPage(activePage(doc));
  const sel = doc.selection[0];
  if (!sel) return null;
  const node = index.nodes.get(sel)!;
  const container = !isTextLeaf(node) && !['img', 'svg', 'input', 'hr', 'br'].includes(node.tag);
  if (container) return { parent: sel };
  const parent = index.parents.get(sel);
  if (!parent) return null;
  return { parent, index: index.nodes.get(parent)!.children.findIndex((c) => c.id === sel) + 1 };
}

async function insertHtmlAt(html: string, preferArtboard = true) {
  const fileId = engineRef.fileId!;
  const target = insertionTarget();
  if (target) {
    const r = await call<{ nodeIds: string[] }>('write_html', { fileId, html, targetNodeId: target.parent, mode: 'insert-children', index: target.index });
    if (r?.nodeIds?.length) select(r.nodeIds);
    return;
  }
  if (!preferArtboard) return toast('Select a frame first');
  // No selection: drop the content into a new artboard at the viewport center.
  const e = engineRef.current!;
  const cx = (e.host.clientWidth / 2 - e.view.x) / e.view.zoom;
  const cy = (e.host.clientHeight / 2 - e.view.y) / e.view.zoom;
  const ab = await call<{ nodeId: string }>('create_artboard', {
    fileId,
    name: 'Frame',
    x: Math.round(cx - 300),
    y: Math.round(cy - 200),
    styles: { width: '600px', height: 'auto', 'background-color': '#ffffff' },
  });
  if (!ab) return;
  const r = await call<{ nodeIds: string[] }>('write_html', { fileId, html, targetNodeId: ab.nodeId, mode: 'insert-children' });
  select(r?.nodeIds?.length ? r.nodeIds : [ab.nodeId]);
}

function insertImage() {
  pickImage((url) => {
    const img = new Image();
    img.onload = () => {
      const w = Math.min(img.naturalWidth, 800);
      const h = Math.round((img.naturalHeight / img.naturalWidth) * w);
      insertHtmlAt(`<img layer-name="Image" src="${url}" style="width:${w}px;height:${h}px;object-fit:cover;flex-shrink:0" />`);
    };
    img.src = url;
  });
}

function Canvas({ fileId }: { fileId: string }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const engine = new CanvasEngine(host.current!, {
      select: (ids) => select(ids),
      commit: (ops) => edit(ops),
      insertHtml: async (parent, html, index, then) => {
        const r = await call<{ nodeIds: string[] }>('write_html', { fileId, html, targetNodeId: parent, mode: 'insert-children', index });
        if (r?.nodeIds) {
          select(r.nodeIds);
          then?.(r.nodeIds);
        }
      },
      createArtboard: async (rect, name, styles) => {
        const r = await call<{ nodeId: string }>('create_artboard', {
          fileId,
          name,
          x: Math.round(rect.x),
          y: Math.round(rect.y),
          styles: { width: `${Math.round(rect.w)}px`, height: `${Math.round(rect.h)}px`, ...styles },
        });
        if (r?.nodeId) select([r.nodeId]);
      },
      view: (view) => setEditor({ view }),
      toolDone: () => setTool('select'),
      contextMenu: (x, y, id) => setEditor({ menu: { x, y, id } }),
      textEditing: (id) => setEditor({ editingText: id }),
      toast,
    });
    engine.setTool(editorState().tool);
    engineRef.current = engine;
    engineRef.fileId = fileId;
    // Sync straight from the socket, before React re-renders, so the inspector measures fresh layout.
    let last: CardboardDocument | null = null;
    const sync = () => {
      const doc = getState().docs[fileId]?.doc;
      if (doc && doc !== last) {
        last = doc;
        engine.setDocument(doc);
      }
    };
    sync();
    const unsub = subscribe(sync);
    return () => {
      unsub();
      engine.destroy();
      if (engineRef.current === engine) {
        engineRef.current = null;
        engineRef.fileId = null;
      }
    };
  }, [fileId]);
  return <div className="canvas-host" ref={host} />;
}

function isTyping(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
}

/** Global editor keyboard shortcuts and clipboard. */
function Shortcuts({ doc }: { doc: CardboardDocument }) {
  const docRef = useRef(doc);
  docRef.current = doc;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (isTyping(e) || engineRef.current?.isEditingText) return;
      const doc = docRef.current;
      const mod = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      const sel = doc.selection;
      const index = indexPage(activePage(doc));
      const engine = engineRef.current;
      const handled = () => {
        e.preventDefault();
        e.stopPropagation();
      };
      if (mod && k === 'z') return handled(), e.shiftKey ? redo() : undo();
      if (mod && k === 'y') return handled(), redo();
      if (mod && k === 'd') {
        handled();
        if (sel.length) edit([{ tool: 'duplicate_nodes', args: { nodes: sel.map((id) => ({ id })) } }]).then((r: any) => {
          const d = (r?.duplicates ?? []).map((x: any) => x.nodeId);
          if (d.length) select(d);
        });
        return;
      }
      if (mod && k === 'a') {
        handled();
        const parent = sel[0] ? index.parents.get(sel[0]) : null;
        select(parent ? index.nodes.get(parent)!.children.map((c) => c.id) : activePage(doc).artboards.map((a) => a.id));
        return;
      }
      if (mod && k === '.') return handled(), setEditor((s) => ({ panels: !s.panels }));
      if (mod && k === '/') return handled(), setEditor({ shortcutsOpen: true });
      if (mod && (k === '=' || k === '+')) return handled(), engine?.zoomAt(1.25);
      if (mod && k === '-') return handled(), engine?.zoomAt(0.8);
      if (mod && k === '0') return handled(), engine?.zoomTo(1);
      if (mod && k === 'g') {
        handled();
        if (e.shiftKey) return ungroup(doc);
        return group(doc);
      }
      if (mod && e.shiftKey && k === 'k') return handled(), insertImage();
      if (mod && e.shiftKey && k === 'h') {
        handled();
        if (sel.length) {
          const hide = index.nodes.get(sel[0])!.visible;
          edit([{ tool: 'set_node_state', args: { updates: sel.map((id) => ({ nodeId: id, visible: !hide })) } }]);
        }
        return;
      }
      if (mod && e.shiftKey && k === 'l') {
        handled();
        if (sel.length) {
          const lock = !index.nodes.get(sel[0])!.locked;
          edit([{ tool: 'set_node_state', args: { updates: sel.map((id) => ({ nodeId: id, locked: lock })) } }]);
        }
        return;
      }
      if (mod && (k === ']' || k === '[')) {
        handled();
        reorder(doc, k === ']' ? 1 : -1, e.shiftKey);
        return;
      }
      if (mod || e.altKey) {
        if (e.altKey && k === 'c' && sel.length) {
          handled();
          const clip = index.nodes.get(sel[0])!.styles.overflow === 'hidden';
          edit([{ tool: 'update_styles', args: { updates: [{ nodeIds: sel, styles: { overflow: clip ? '' : 'hidden' } }] } }]);
        }
        return;
      }
      if (e.shiftKey && k === '!') return handled(), engine?.zoomToFit(false);
      if (e.shiftKey && k === '@') return handled(), engine?.zoomToFit(true);
      if (e.shiftKey && e.code === 'Digit1') return handled(), engine?.zoomToFit(false);
      if (e.shiftKey && e.code === 'Digit2') return handled(), engine?.zoomToFit(true);
      if (e.shiftKey && k === 'a' && sel.length) {
        handled();
        const flex = index.nodes.get(sel[0])!.styles.display === 'flex';
        edit([{ tool: 'update_styles', args: { updates: [{ nodeIds: sel, styles: flex ? { display: '', 'flex-direction': '' } : { display: 'flex', 'flex-direction': 'column' } }] } }]);
        return;
      }
      if (k === 'v') return setTool('select');
      if (k === 'h') return setTool('hand');
      if (k === 'f' || k === 'a') return setTool('frame');
      if (k === 'r') return setTool('rect');
      if (k === 't') return setTool('text');
      if (k === 'escape') {
        if (editorState().tool !== 'select') return setTool('select');
        const parent = sel[0] ? index.parents.get(sel[0]) : null;
        return select(parent ? [parent] : []);
      }
      if (k === 'enter' && sel.length === 1) {
        handled();
        const n = index.nodes.get(sel[0])!;
        if (e.shiftKey) {
          const p = index.parents.get(sel[0]);
          if (p) select([p]);
        } else if (n.children.length) select(n.children.map((c) => c.id));
        else if (isTextLeaf(n)) engine?.startEditing(n.id, true);
        return;
      }
      if (k === 'tab' && sel.length === 1) {
        handled();
        const p = index.parents.get(sel[0]);
        const siblings = p ? index.nodes.get(p)!.children : activePage(doc).artboards;
        const i = siblings.findIndex((s) => s.id === sel[0]);
        const next = siblings[(i + (e.shiftKey ? -1 : 1) + siblings.length) % siblings.length];
        if (next) select([next.id]);
        return;
      }
      if ((k === 'delete' || k === 'backspace') && sel.length) {
        handled();
        edit([{ tool: 'delete_nodes', args: { nodeIds: sel } }]);
        return;
      }
      if (k.startsWith('arrow') && sel.length) {
        handled();
        nudge(doc, k, e.shiftKey ? 10 : 1);
      }
    };
    const copy = (e: ClipboardEvent) => {
      if (isTyping(e as unknown as KeyboardEvent) || engineRef.current?.isEditingText) return;
      const doc = docRef.current;
      const index = indexPage(activePage(doc));
      const nodes = doc.selection.map((id) => index.nodes.get(id)).filter(Boolean);
      if (!nodes.length) return;
      e.preventDefault();
      const html = nodes.map((n) => nodeToHtml(n!)).join('\n');
      e.clipboardData?.setData('text/html', `<!--cardboard-->${html}`);
      e.clipboardData?.setData('text/plain', html);
      if (e.type === 'cut') edit([{ tool: 'delete_nodes', args: { nodeIds: doc.selection } }]);
    };
    // On Linux, releasing the middle button pastes the primary selection (whatever text was
    // last highlighted). The middle button pans the canvas here, so drop those pastes.
    let middleReleasedAt = -Infinity;
    const pointerUp = (e: PointerEvent) => {
      if (e.button === 1) middleReleasedAt = performance.now();
    };
    const paste = (e: ClipboardEvent) => {
      if (performance.now() - middleReleasedAt < 500) return e.preventDefault();
      if (isTyping(e as unknown as KeyboardEvent) || engineRef.current?.isEditingText) return;
      const dt = e.clipboardData;
      if (!dt) return;
      const file = [...dt.files].find((f) => f.type.startsWith('image/'));
      if (file) {
        e.preventDefault();
        const reader = new FileReader();
        reader.onload = () => {
          const url = String(reader.result);
          const img = new Image();
          img.onload = () => {
            const w = Math.min(img.naturalWidth, 800);
            const h = Math.round((img.naturalHeight / img.naturalWidth) * w);
            insertHtmlAt(`<img layer-name="Image" src="${url}" style="width:${w}px;height:${h}px;object-fit:cover;flex-shrink:0" />`);
          };
          img.src = url;
        };
        reader.readAsDataURL(file);
        return;
      }
      let html = dt.getData('text/html');
      const text = dt.getData('text/plain');
      if (html.includes('<!--cardboard-->')) html = html.slice(html.indexOf('<!--cardboard-->') + 16);
      else if (/^\s*</.test(text)) html = text;
      else if (text.trim()) html = `<p layer-name="Text" style="margin:0;font-size:16px">${text.replace(/&/g, '&amp;').replace(/</g, '&lt;')}</p>`;
      else return;
      e.preventDefault();
      // Strip anything the HTML importer would reject (metadata wrappers from other apps).
      html = html.replace(/<meta[^>]*>/gi, '').replace(/<\/?(html|body|head)[^>]*>/gi, '').replace(/<!--[\s\S]*?-->/g, '').trim();
      insertHtmlAt(html);
    };
    window.addEventListener('keydown', key);
    window.addEventListener('copy', copy);
    window.addEventListener('cut', copy);
    window.addEventListener('paste', paste);
    window.addEventListener('pointerup', pointerUp, true);
    return () => {
      window.removeEventListener('pointerup', pointerUp, true);
      window.removeEventListener('keydown', key);
      window.removeEventListener('copy', copy);
      window.removeEventListener('cut', copy);
      window.removeEventListener('paste', paste);
    };
  }, []);
  return null;
}

function nudge(doc: CardboardDocument, key: string, amount: number) {
  const index = indexPage(activePage(doc));
  const engine = engineRef.current;
  const dx = key === 'arrowleft' ? -amount : key === 'arrowright' ? amount : 0;
  const dy = key === 'arrowup' ? -amount : key === 'arrowdown' ? amount : 0;
  const positions: { nodeId: string; x: number; y: number }[] = [];
  const styles: { nodeIds: string[]; styles: Record<string, string> }[] = [];
  for (const id of doc.selection) {
    const n = index.nodes.get(id)!;
    if (n.locked) continue;
    if (index.parents.get(id) === null) {
      const g = engine?.geometry(id);
      const [x, y] = n.position ?? [g?.x ?? 0, g?.y ?? 0];
      positions.push({ nodeId: id, x: x + dx, y: y + dy });
    } else if (n.styles.position === 'absolute' || n.styles.position === 'fixed') {
      const g = engine?.geometry(id);
      styles.push({ nodeIds: [id], styles: { left: `${(parseFloat(n.styles.left) || g?.x || 0) + dx}px`, top: `${(parseFloat(n.styles.top) || g?.y || 0) + dy}px` } });
    } else {
      // In a flow layout, arrows reorder along the parent's axis.
      reorder(doc, dx + dy > 0 ? 1 : -1, false);
      return;
    }
  }
  const ops = [];
  if (positions.length) ops.push({ tool: 'set_positions', args: { updates: positions } });
  if (styles.length) ops.push({ tool: 'update_styles', args: { updates: styles } });
  if (ops.length) edit(ops);
}

function reorder(doc: CardboardDocument, dir: 1 | -1, toEnd: boolean) {
  const index = indexPage(activePage(doc));
  const id = doc.selection[0];
  if (!id) return;
  const parent = index.parents.get(id) ?? 'root';
  const siblings = parent === 'root' ? activePage(doc).artboards : index.nodes.get(parent)!.children;
  const i = siblings.findIndex((s) => s.id === id);
  const target = toEnd ? (dir > 0 ? siblings.length - 1 : 0) : Math.max(0, Math.min(siblings.length - 1, i + dir));
  if (target === i) return;
  edit([{ tool: 'move_nodes', args: { moves: [{ nodeId: id, parentId: parent, index: target }] } }]);
}

/** Wrap the selection in a new flex frame (Ctrl G). */
async function group(doc: CardboardDocument) {
  const index = indexPage(activePage(doc));
  const sel = doc.selection.filter((id) => index.parents.get(id));
  if (!sel.length) return toast('Select layers inside a frame to group them');
  const parent = index.parents.get(sel[0])!;
  if (sel.some((id) => index.parents.get(id) !== parent)) return toast('Grouped layers must share a parent');
  const siblings = index.nodes.get(parent)!.children.map((c) => c.id);
  const first = Math.min(...sel.map((id) => siblings.indexOf(id)));
  const pc = engineRef.current?.computed(parent);
  const row = pc?.display.includes('flex') && pc.flexDirection.startsWith('row');
  const r = await call<{ nodeIds: string[] }>('write_html', {
    fileId: doc.id,
    html: `<div layer-name="Group" style="display:flex;flex-direction:${row ? 'row' : 'column'};gap:0px"></div>`,
    targetNodeId: parent,
    mode: 'insert-children',
    index: first,
  });
  const g = r?.nodeIds?.[0];
  if (!g) return;
  const ordered = [...sel].sort((a, b) => siblings.indexOf(a) - siblings.indexOf(b));
  await edit([{ tool: 'move_nodes', args: { moves: ordered.map((id, i) => ({ nodeId: id, parentId: g, index: i })) } }]);
  select([g]);
}
async function ungroup(doc: CardboardDocument) {
  const index = indexPage(activePage(doc));
  const id = doc.selection[0];
  const parent = id ? index.parents.get(id) : null;
  if (!id || !parent) return;
  const node = index.nodes.get(id)!;
  const at = index.nodes.get(parent)!.children.findIndex((c) => c.id === id);
  const kids = node.children.map((c) => c.id);
  await edit([
    { tool: 'move_nodes', args: { moves: kids.map((k, i) => ({ nodeId: k, parentId: parent, index: at + i })) } },
    { tool: 'delete_nodes', args: { nodeIds: [id] } },
  ]);
  select(kids);
}

function ContextMenu({ doc }: { doc: CardboardDocument }) {
  const menu = useEditor((s) => s.menu);
  const anchor = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  useLayoutEffect(() => setReady(!!menu), [menu]);
  if (!menu) return null;
  const close = () => setEditor({ menu: null });
  const index = indexPage(activePage(doc));
  const node = menu.id ? index.nodes.get(menu.id) : null;
  const sel = doc.selection;
  const item = (label: string, key: string, fn: () => void, disabled = false, danger = false) => (
    <button
      type="button"
      className={`menu-item ${danger ? 'danger' : ''}`}
      disabled={disabled}
      onClick={() => {
        close();
        fn();
      }}
    >
      <span>{label}</span>
      <span className="menu-key">{key}</span>
    </button>
  );
  return (
    <>
      <div ref={anchor} style={{ position: 'fixed', left: menu.x, top: menu.y, width: 1, height: 1 }} />
      {ready && anchor.current && (
        <Popover anchor={anchor.current} onClose={close} className="menu" width={220}>
          {node ? (
            <>
              {item('Copy', 'Ctrl C', () => document.execCommand('copy'))}
              {item('Copy as HTML', '', () => copyHtml(node))}
              {item('Paste', 'Ctrl V', () => toast('Use Ctrl V to paste'))}
              <div className="menu-sep" />
              {item('Duplicate', 'Ctrl D', () => edit([{ tool: 'duplicate_nodes', args: { nodes: sel.map((id) => ({ id })) } }]))}
              {item('Group', 'Ctrl G', () => group(doc), !index.parents.get(node.id))}
              {item('Ungroup', 'Ctrl Shift G', () => ungroup(doc), !index.parents.get(node.id) || !node.children.length)}
              <div className="menu-sep" />
              {item('Bring forward', 'Ctrl ]', () => reorder(doc, 1, false))}
              {item('Send backward', 'Ctrl [', () => reorder(doc, -1, false))}
              <div className="menu-sep" />
              {item(node.visible ? 'Hide' : 'Show', 'Ctrl Shift H', () => edit([{ tool: 'set_node_state', args: { updates: sel.map((id) => ({ nodeId: id, visible: !node.visible })) } }]))}
              {item(node.locked ? 'Unlock' : 'Lock', 'Ctrl Shift L', () => edit([{ tool: 'set_node_state', args: { updates: sel.map((id) => ({ nodeId: id, locked: !node.locked })) } }]))}
              {item('Zoom to selection', 'Shift 2', () => engineRef.current?.zoomToFit(true))}
              <div className="menu-sep" />
              {item('Delete', 'Del', () => edit([{ tool: 'delete_nodes', args: { nodeIds: sel } }]), false, true)}
            </>
          ) : (
            <>
              {item('Paste', 'Ctrl V', () => toast('Use Ctrl V to paste'))}
              {item('Zoom to fit', 'Shift 1', () => engineRef.current?.zoomToFit(false))}
              {item('Insert HTML…', '', () => setEditor({ insertHtmlOpen: true }))}
              {item('Insert icon…', '', () => setEditor({ iconsOpen: true }))}
            </>
          )}
        </Popover>
      )}
    </>
  );
}

function Dialog({ title, onClose, children, wide }: { title: string; onClose: () => void; children: React.ReactNode; wide?: boolean }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, [onClose]);
  return (
    <div className="dialog-backdrop" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`dialog ${wide ? 'wide' : ''}`} role="dialog">
        <header>
          <span>{title}</span>
          <button type="button" className="icon-btn" onClick={onClose}>
            <X size={14} />
          </button>
        </header>
        {children}
      </div>
    </div>
  );
}

function ExportDialog({ doc }: { doc: CardboardDocument }) {
  const open = useEditor((s) => s.exportOpen);
  const [html, setHtml] = useState('');
  useEffect(() => {
    if (open) request<{ html: string }>('export_html', { fileId: doc.id, maxChars: 0 }).then((r) => setHtml(r.html)).catch((e) => setHtml(String(e)));
  }, [open, doc.id, doc.revision]);
  if (!open) return null;
  const download = () => {
    const blob = new Blob([html], { type: 'text/html' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `${doc.name}.html`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  };
  return (
    <Dialog title={`Export “${activePage(doc).name}”`} onClose={() => setEditor({ exportOpen: false })} wide>
      <p className="muted small">Plain HTML and CSS for the current page. Tokens become CSS variables on :root.</p>
      <textarea className="code-area mono" readOnly value={html} />
      <footer>
        <button type="button" className="btn" onClick={() => (navigator.clipboard.writeText(html), toast('HTML copied'))}>
          Copy
        </button>
        <button type="button" className="btn primary" onClick={download}>
          Download .html
        </button>
      </footer>
    </Dialog>
  );
}

function InsertHtmlDialog({ doc }: { doc: CardboardDocument }) {
  const open = useEditor((s) => s.insertHtmlOpen);
  const [html, setHtml] = useState(
    '<div layer-name="Card" style="display:flex;flex-direction:column;gap:12px;padding:32px;background:#ffffff;border:1px solid #e5e5e5;border-radius:16px">\n  <h2 style="margin:0;font-size:24px">A new idea</h2>\n  <p style="margin:0;color:#666">Made with HTML and CSS.</p>\n</div>',
  );
  if (!open) return null;
  const target = insertionTarget();
  return (
    <Dialog title="Insert HTML" onClose={() => setEditor({ insertHtmlOpen: false })} wide>
      <p className="muted small">
        Static HTML with inline styles. {target ? 'It will be inserted into the selected frame.' : 'Nothing is selected, so a new frame will be created.'} Close void elements like <code>&lt;img /&gt;</code>.
      </p>
      <textarea className="code-area mono" value={html} spellCheck={false} onChange={(e) => setHtml(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      <footer>
        <span className="muted small">{doc.name}</span>
        <span className="spacer" />
        <button
          type="button"
          className="btn primary"
          onClick={async () => {
            await insertHtmlAt(html);
            setEditor({ insertHtmlOpen: false });
          }}
        >
          Insert
        </button>
      </footer>
    </Dialog>
  );
}

const ICON_SETS = [
  { value: 'lucide', label: 'Lucide' },
  { value: 'ph', label: 'Phosphor' },
  { value: 'tabler', label: 'Tabler' },
  { value: 'heroicons', label: 'Heroicons' },
  { value: 'material-symbols', label: 'Material Symbols' },
  { value: 'all', label: 'All sets' },
];
/** Search Iconify and insert the picked icon as one layer, next to or inside the selection. */
function IconDialog() {
  const open = useEditor((s) => s.iconsOpen);
  const [query, setQuery] = useState('');
  const [iconSet, setIconSet] = useState('lucide');
  const [results, setResults] = useState<{ icons: string[]; note?: string } | null>(null);
  useEffect(() => {
    const q = query.trim();
    if (!open || !q) return setResults(null);
    let live = true;
    const timer = setTimeout(() => {
      request<{ icons: string[]; note?: string }>('search_icons', { query: q, limit: 96, ...(iconSet === 'all' ? {} : { sets: [iconSet] }) })
        .then((r) => live && setResults(r))
        .catch((e: Error) => live && setResults({ icons: [], note: e.message }));
    }, 250);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [open, query, iconSet]);
  if (!open) return null;
  const close = () => setEditor({ iconsOpen: false });
  const status = results?.note ?? (results && !results.icons.length ? 'No icons found.' : insertionTarget() ? 'Inserts next to the selection as one layer.' : 'Nothing is selected, so a new frame will be created.');
  return (
    <Dialog title="Insert icon" onClose={close} wide>
      <div className="icon-search">
        <label className="field">
          <input autoFocus value={query} placeholder="Search icons: arrow, settings, user…" spellCheck={false} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key !== 'Escape' && e.stopPropagation()} />
        </label>
        <Select value={iconSet} options={ICON_SETS} onChange={setIconSet} width={180} />
      </div>
      <div className="icon-grid">
        {results?.icons.map((name) => {
          const url = `url("https://api.iconify.design/${name.replace(':', '/')}.svg")`;
          return (
            <button
              key={name}
              type="button"
              className="icon-cell"
              title={name}
              onClick={async () => {
                close();
                await insertHtmlAt(`<icon name="${name}" />`);
              }}
            >
              <span style={{ maskImage: url, WebkitMaskImage: url }} />
            </button>
          );
        })}
      </div>
      <footer>
        <span className="muted small">{status}</span>
        <span className="spacer" />
      </footer>
    </Dialog>
  );
}

const SHORTCUTS: [string, string][] = [
  ['Select / Hand', 'V / H (hold Space to pan)'],
  ['Frame / Rectangle / Text', 'F / R / T'],
  ['Select deeper / edit text', 'Click again, double-click text, Enter'],
  ['Select parent', 'Shift Enter, Esc'],
  ['Select deepest layer', 'Ctrl click'],
  ['Next / previous sibling', 'Tab / Shift Tab'],
  ['Duplicate', 'Ctrl D, or Alt drag'],
  ['Group / Ungroup', 'Ctrl G / Ctrl Shift G'],
  ['Add flex', 'Shift A'],
  ['Clip content', 'Alt C'],
  ['Hide / Lock', 'Ctrl Shift H / Ctrl Shift L'],
  ['Bring forward / back', 'Ctrl ] / Ctrl ['],
  ['Nudge (or reorder in flex)', 'Arrows, Shift for 10px'],
  ['Zoom', 'Ctrl scroll, Ctrl + / −'],
  ['Zoom to fit / selection / 100%', 'Shift 1 / Shift 2 / Ctrl 0'],
  ['Toggle panels', 'Ctrl .'],
  ['Keyboard shortcuts', 'Ctrl /'],
  ['Undo / Redo', 'Ctrl Z / Ctrl Shift Z'],
  ['Scrub a number', 'Drag the field label'],
];
function ShortcutsDialog() {
  const open = useEditor((s) => s.shortcutsOpen);
  if (!open) return null;
  return (
    <Dialog title="Keyboard shortcuts" onClose={() => setEditor({ shortcutsOpen: false })}>
      <div className="shortcuts">
        {SHORTCUTS.map(([a, b]) => (
          <div key={a} className="shortcut">
            <span>{a}</span>
            <kbd>{b}</kbd>
          </div>
        ))}
      </div>
    </Dialog>
  );
}
