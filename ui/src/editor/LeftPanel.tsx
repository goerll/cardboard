import { memo, useEffect, useMemo, useRef, useState } from 'react';
import {
  Bot,
  ChevronDown,
  ChevronRight,
  Columns2,
  Eye,
  EyeOff,
  File,
  Frame,
  Grid2x2,
  Image as ImageIcon,
  Lock,
  LockOpen,
  PanelLeft,
  Plus,
  Rows2,
  Shapes,
  Smile,
  Square,
  Type,
  Spline,
  Trash2,
  Copy,
  Component,
  Diamond,
} from 'lucide-react';
import type { CardboardDocument, DesignNode } from '../types';
import { activePage, ancestors, displayName, indexPage, layerKind, type LayerKind } from '../lib/tree';
import { call, useServer } from '../api';
import { edit, engineRef, select, setEditor, useEditor } from './store';
import { IconButton, Popover, Segmented, TextField } from '../ui/controls';
import { ColorRow } from '../ui/ColorPicker';
import { parseColor } from '../lib/css';
import { componentRole } from './components';
import { BASE_MODE, modeNames, resolvedTokens } from '../lib/tokens';

const KIND_ICON: Record<LayerKind, React.ReactNode> = {
  frame: <Frame size={13} />,
  'flex-row': <Columns2 size={13} />,
  'flex-col': <Rows2 size={13} />,
  grid: <Grid2x2 size={13} />,
  text: <Type size={13} />,
  image: <ImageIcon size={13} />,
  icon: <Smile size={13} />,
  svg: <Shapes size={13} />,
  shape: <Spline size={13} />,
  rect: <Square size={13} />,
};

export function LeftPanel({ doc }: { doc: CardboardDocument }) {
  const tab = useEditor((s) => s.leftTab);
  const [renaming, setRenaming] = useState(false);
  return (
    <aside className="left-panel">
      <div className="panel-header">
        <File size={14} className="muted" />
        {renaming ? (
          <TextField
            className="title-input"
            value={doc.name}
            autoFocus
            onCommit={(name) => {
              setRenaming(false);
              if (name.trim()) call('rename_file', { fileId: doc.id, name: name.trim() });
            }}
            onCancel={() => setRenaming(false)}
          />
        ) : (
          <span className="file-title" onDoubleClick={() => setRenaming(true)} title="Double-click to rename">
            {doc.name}
          </span>
        )}
        <IconButton icon={<PanelLeft size={14} />} title="Hide panels (Ctrl .)" onClick={() => setEditor({ panels: false })} />
      </div>
      <div className="panel-tabs">
        <Segmented
          className="wide"
          value={tab}
          options={[
            { value: 'design', label: 'Design' },
            { value: 'theme', label: 'Theme' },
          ]}
          onChange={(v) => setEditor({ leftTab: v })}
        />
      </div>
      {tab === 'design' ? (
        <>
          <Pages doc={doc} />
          <Layers doc={doc} />
        </>
      ) : (
        <Theme doc={doc} />
      )}
      <div className="panel-footer">
        <button type="button" className="link" onClick={() => setEditor({ shortcutsOpen: true })}>
          Shortcuts
        </button>
        <span>•</span>
        <a className="link" href="https://github.com/" onClick={(e) => e.preventDefault()}>
          Cardboard 0.2
        </a>
      </div>
    </aside>
  );
}

function Pages({ doc }: { doc: CardboardDocument }) {
  const collapsed = useEditor((s) => s.collapsed.pages);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ id: string; el: HTMLElement } | null>(null);
  return (
    <div className="pages">
      <div className="tree-head" onClick={() => setEditor((s) => ({ collapsed: { ...s.collapsed, pages: !collapsed } }))}>
        {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <span>Pages</span>
        <span className="spacer" />
        <IconButton
          icon={<Plus size={14} />}
          title="Add page"
          onClick={async (e) => {
            e.stopPropagation();
            const r = await edit([{ tool: 'create_page', args: { name: `Page ${doc.pages.length + 1}` } }]);
            const id = r?.pageId ?? r?.results?.[0]?.pageId;
            if (id) call('set_active_page', { fileId: doc.id, pageId: id });
          }}
        />
      </div>
      {!collapsed &&
        doc.pages.map((p) => (
          <div
            key={p.id}
            className={`tree-row page-row ${p.id === doc.activePage ? 'selected' : ''}`}
            onClick={() => p.id !== doc.activePage && call('set_active_page', { fileId: doc.id, pageId: p.id })}
            onDoubleClick={() => setRenaming(p.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ id: p.id, el: e.currentTarget });
            }}
          >
            <File size={13} className="row-icon" />
            {renaming === p.id ? (
              <TextField
                className="row-input"
                value={p.name}
                autoFocus
                onCommit={(name) => {
                  setRenaming(null);
                  edit([{ tool: 'update_pages', args: { updates: [{ pageId: p.id, name }] } }]);
                }}
                onCancel={() => setRenaming(null)}
              />
            ) : (
              <span className="row-name">{p.name}</span>
            )}
          </div>
        ))}
      {menu && (
        <Popover anchor={menu.el} onClose={() => setMenu(null)} className="menu" width={180}>
          <button type="button" className="menu-item" onClick={() => (setRenaming(menu.id), setMenu(null))}>
            Rename
          </button>
          <button type="button" className="menu-item" onClick={() => (edit([{ tool: 'duplicate_page', args: { pageId: menu.id } }]), setMenu(null))}>
            <Copy size={13} /> Duplicate
          </button>
          <button
            type="button"
            className="menu-item danger"
            disabled={doc.pages.length < 2}
            onClick={() => (edit([{ tool: 'delete_pages', args: { pageIds: [menu.id] } }]), setMenu(null))}
          >
            <Trash2 size={13} /> Delete
          </button>
        </Popover>
      )}
    </div>
  );
}

interface DragState {
  ids: string[];
  over: string | null;
  where: 'before' | 'after' | 'inside';
}

function Layers({ doc }: { doc: CardboardDocument }) {
  const page = activePage(doc);
  const index = indexPage(page);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [drag, setDrag] = useState<DragState | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const selection = doc.selection;
  const claims = useServer((s) => s.docs[doc.id]?.agents);
  // Claimed layers show the agent's name; their ancestors show that work is happening inside.
  const agents = useMemo(() => {
    const out = new Map<string, string>();
    for (const [id, name] of Object.entries(claims ?? {})) {
      if (!index.nodes.has(id)) continue;
      out.set(id, name);
      for (const a of ancestors(index, id)) if (!out.has(a)) out.set(a, '');
    }
    return out;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [claims, page]);
  // Reveal selected layers.
  useEffect(() => {
    const add: Record<string, boolean> = {};
    for (const id of selection) for (const a of ancestors(index, id)) if (!open[a]) add[a] = true;
    if (Object.keys(add).length) setOpen((o) => ({ ...o, ...add }));
    requestAnimationFrame(() => {
      const el = listRef.current?.querySelector(`[data-layer="${CSS.escape(selection[0] ?? '')}"]`);
      (el as HTMLElement | null)?.scrollIntoView({ block: 'nearest' });
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection.join(','), page.id]);

  const rows = useMemo(() => {
    const out: { node: DesignNode; depth: number }[] = [];
    const walk = (n: DesignNode, depth: number) => {
      out.push({ node: n, depth });
      if (open[n.id]) for (const c of n.children) walk(c, depth + 1);
    };
    for (const a of page.artboards) walk(a, 0);
    return out;
  }, [page, open]);

  const onRowPointerDown = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button, input')) return;
    const startY = e.clientY;
    const ids = selection.includes(id) ? selection : [id];
    let started = false;
    let state: DragState | null = null;
    const move = (ev: PointerEvent) => {
      if (!started && Math.abs(ev.clientY - startY) < 4) return;
      started = true;
      const el = document.elementFromPoint(ev.clientX, ev.clientY)?.closest('[data-layer]') as HTMLElement | null;
      if (!el) return;
      const over = el.dataset.layer!;
      if (ids.some((i) => i === over || ancestors(index, over).includes(i))) {
        state = { ids, over: null, where: 'before' };
      } else {
        const r = el.getBoundingClientRect();
        const f = (ev.clientY - r.top) / r.height;
        const n = index.nodes.get(over)!;
        const container = !['img', 'svg', 'br', 'hr', 'input'].includes(n.tag) && !(n.children.length === 0 && n.text);
        const isRoot = index.parents.get(over) === null;
        const where = isRoot ? (f < 0.25 ? 'before' : f > 0.75 && !open[over] ? 'after' : 'inside') : f < 0.3 ? 'before' : f > 0.7 && !(open[over] && n.children.length) ? 'after' : container ? 'inside' : f < 0.5 ? 'before' : 'after';
        state = { ids, over, where };
      }
      setDrag(state);
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      setDrag(null);
      if (!started || !state?.over) return;
      const { over, where } = state;
      let parent: string;
      let idx: number;
      if (where === 'inside') {
        parent = over;
        idx = 0;
      } else {
        parent = index.parents.get(over) ?? 'root';
        const siblings = parent === 'root' ? page.artboards : index.nodes.get(parent)!.children;
        idx = siblings.findIndex((s) => s.id === over) + (where === 'after' ? 1 : 0);
      }
      // Account for moved nodes that sit before the slot in the same parent.
      const siblings = parent === 'root' ? page.artboards : index.nodes.get(parent)!.children;
      let i = idx;
      const moves = ids.map((nodeId) => {
        const cur = siblings.findIndex((s) => s.id === nodeId);
        const target = cur >= 0 && cur < idx ? i - 1 : i;
        if (!(cur >= 0 && cur < idx)) i++;
        return { nodeId, parentId: parent, index: Math.max(0, target) };
      });
      edit([{ tool: 'move_nodes', args: { moves } }]);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };

  return (
    <div className="layers" ref={listRef}>
      {page.artboards.length === 0 && <div className="empty-hint">Press F and drag on the canvas to create a frame.</div>}
      {rows.map(({ node, depth }) => (
        <LayerRow
          key={node.id}
          node={node}
          depth={depth}
          artboard={depth === 0}
          agent={agents.get(node.id)}
          selected={selection.includes(node.id)}
          parentSelected={!selection.includes(node.id) && ancestors(index, node.id).some((a) => selection.includes(a))}
          open={!!open[node.id]}
          drop={drag?.over === node.id ? drag.where : null}
          onToggle={(all) => {
            if (all) {
              const next = !open[node.id];
              const patch: Record<string, boolean> = {};
              const walk = (n: DesignNode) => {
                patch[n.id] = next;
                n.children.forEach(walk);
              };
              walk(node);
              setOpen((o) => ({ ...o, ...patch }));
            } else setOpen((o) => ({ ...o, [node.id]: !o[node.id] }));
          }}
          onPointerDown={(e) => onRowPointerDown(e, node.id)}
          onSelect={(e) => {
            if (e.shiftKey && selection.length) {
              // Range select among visible rows.
              const ids = rows.map((r) => r.node.id);
              const a = ids.indexOf(selection[selection.length - 1]);
              const b = ids.indexOf(node.id);
              if (a >= 0 && b >= 0) return select([...new Set([...selection, ...ids.slice(Math.min(a, b), Math.max(a, b) + 1)])]);
            }
            if (e.metaKey || e.ctrlKey) return select(selection.includes(node.id) ? selection.filter((s) => s !== node.id) : [...selection, node.id]);
            select([node.id]);
          }}
          onFocusCanvas={() => engineRef.current?.zoomToFit(true)}
        />
      ))}
    </div>
  );
}

const LayerRow = memo(function LayerRow(props: {
  node: DesignNode;
  depth: number;
  artboard: boolean;
  /** Agent editing this layer; '' when an agent is editing something inside it. */
  agent?: string;
  selected: boolean;
  parentSelected: boolean;
  open: boolean;
  drop: DragState['where'] | null;
  onToggle: (all: boolean) => void;
  onSelect: (e: React.MouseEvent) => void;
  onPointerDown: (e: React.PointerEvent) => void;
  onFocusCanvas: () => void;
}) {
  const { node, depth } = props;
  const [renaming, setRenaming] = useState(false);
  const kind = layerKind(node, props.artboard);
  const label = displayName(node);
  const role = componentRole(node);
  const icon = role === 'main' ? <Component size={13} /> : role === 'instance' ? <Diamond size={12} /> : KIND_ICON[kind];
  return (
    <div
      data-layer={node.id}
      title={role === 'main' ? 'Main component' : role === 'instance' ? 'Instance' : undefined}
      className={`tree-row layer-row ${props.selected ? 'selected' : ''} ${props.parentSelected ? 'child-of-selected' : ''} ${!node.visible ? 'hidden-layer' : ''} ${props.drop ? `drop-${props.drop}` : ''} ${role ? `role-${role}` : ''}`}
      style={{ paddingLeft: 8 + depth * 14 }}
      onClick={props.onSelect}
      onDoubleClick={(e) => {
        if ((e.target as HTMLElement).closest('.row-name')) setRenaming(true);
        else props.onFocusCanvas();
      }}
      onPointerDown={props.onPointerDown}
      onPointerEnter={() => engineRef.current && engineRef.current.requestDraw()}
    >
      <span
        className={`disclosure ${node.children.length ? '' : 'none'}`}
        onClick={(e) => {
          e.stopPropagation();
          props.onToggle(e.altKey);
        }}
      >
        {node.children.length > 0 && (props.open ? <ChevronDown size={12} /> : <ChevronRight size={12} />)}
      </span>
      <span className="row-icon">{icon}</span>
      {renaming ? (
        <TextField
          className="row-input"
          value={node.name}
          autoFocus
          onCommit={(name) => {
            setRenaming(false);
            edit([{ tool: 'rename_nodes', args: { updates: [{ nodeId: node.id, name }] } }]);
          }}
          onCancel={() => setRenaming(false)}
        />
      ) : (
        <span className="row-name">{label}</span>
      )}
      {props.agent !== undefined && (
        <span
          className={`agent-badge ${props.agent ? '' : 'inside'}`}
          title={props.agent ? `${props.agent} is editing this layer` : 'An agent is editing a layer inside this one'}
        >
          <Bot size={11} />
          {props.agent && <span className="agent-name">{props.agent}</span>}
        </span>
      )}
      <span className={`row-actions ${node.locked || !node.visible ? 'pinned' : ''}`}>
        <button
          type="button"
          className={`row-action ${node.locked ? 'on' : ''}`}
          title={node.locked ? 'Unlock' : 'Lock'}
          onClick={(e) => {
            e.stopPropagation();
            edit([{ tool: 'set_node_state', args: { updates: [{ nodeId: node.id, locked: !node.locked }] } }]);
          }}
        >
          {node.locked ? <Lock size={12} /> : <LockOpen size={12} />}
        </button>
        <button
          type="button"
          className={`row-action ${!node.visible ? 'on' : ''}`}
          title={node.visible ? 'Hide' : 'Show'}
          onClick={(e) => {
            e.stopPropagation();
            edit([{ tool: 'set_node_state', args: { updates: [{ nodeId: node.id, visible: !node.visible }] } }]);
          }}
        >
          {node.visible ? <Eye size={12} /> : <EyeOff size={12} />}
        </button>
      </span>
    </div>
  );
});

function Theme({ doc }: { doc: CardboardDocument }) {
  const modes = modeNames(doc.modes);
  const [picked, setMode] = useState(BASE_MODE);
  const mode = modes.includes(picked) ? picked : BASE_MODE;
  const tokens = Object.entries(resolvedTokens(doc.tokens, doc.modes, mode));
  const setToken = (name: string, value: string) => edit([{ tool: 'set_tokens', args: { mode, tokens: [{ name, value }] } }]);
  const colors = tokens.filter(([, v]) => parseColor(v));
  const other = tokens.filter(([, v]) => !parseColor(v));
  const add = (kind: 'color' | 'value') => {
    let i = 1;
    const base = kind === 'color' ? '--color' : '--space';
    while (doc.tokens[`${base}-${i}`]) i++;
    edit([{ tool: 'create_tokens', args: { tokens: [{ name: `${base}-${i}`, value: kind === 'color' ? '#3b82f6' : '16px' }] } }]);
  };
  const row = ([name, value]: [string, string], color: boolean) => (
    <div className="token-row" key={name}>
      <TextField
        className="token-name mono"
        value={name.slice(2)}
        onCommit={(n) => {
          const clean = n.trim().replace(/^-+/, '').replace(/\s+/g, '-');
          if (clean && `--${clean}` !== name) edit([{ tool: 'set_tokens', args: { tokens: [{ name, newName: `--${clean}` }] } }]);
        }}
      />
      {color ? (
        <ColorRow value={value} onPreview={(v) => engineRef.current?.iframe.contentDocument?.documentElement.style.setProperty(name, v)} onPreviewEnd={() => engineRef.current?.iframe.contentDocument?.documentElement.style.removeProperty(name)} onCommit={(v) => setToken(name, v)} />
      ) : (
        <TextField className="mono" value={value} onCommit={(v) => setToken(name, v)} />
      )}
      <IconButton icon={<Trash2 size={13} />} title="Delete token" onClick={() => edit([{ tool: 'set_tokens', args: { tokens: [{ name, delete: true }] } }])} />
    </div>
  );
  return (
    <div className="theme">
      {modes.length > 1 && (
        <div className="tree-head">
          <Segmented value={mode} options={modes.map((m) => ({ value: m, label: m, title: `Edit ${m} values` }))} onChange={setMode} />
        </div>
      )}
      <div className="tree-head">
        <span>Colors</span>
        <span className="spacer" />
        <IconButton icon={<Plus size={14} />} title="Add color token" onClick={() => add('color')} />
      </div>
      {colors.length === 0 && <div className="empty-hint">Color tokens become CSS variables your layers can reference.</div>}
      {colors.map((t) => row(t, true))}
      <div className="tree-head">
        <span>Values</span>
        <span className="spacer" />
        <IconButton icon={<Plus size={14} />} title="Add value token" onClick={() => add('value')} />
      </div>
      {other.length === 0 && <div className="empty-hint">Spacing, radii, fonts: any CSS value.</div>}
      {other.map((t) => row(t, false))}
    </div>
  );
}
