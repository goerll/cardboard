import { useMemo, useRef, useState } from 'react';
import {
  AlignCenterHorizontal,
  ArrowDown,
  ArrowRight,
  Blend,
  ChevronDown,
  Copy,
  Eye,
  EyeOff,
  Image as ImageIcon,
  Minus,
  Plus,
  RotateCw,
  Scan,
  SquareDashed,
  WrapText,
  AlignLeft,
  AlignCenter,
  AlignRight,
  AlignJustify,
  Droplet,
  Check,
} from 'lucide-react';
import type { CardboardDocument, DesignNode } from '../types';
import { activePage, displayName, indexPage, isTextLeaf, nodeToHtml, sizeMode } from '../lib/tree';
import { boxSides, formatGradient, formatShadows, num, parseColor, parseGradient, parseShadows, round, type Gradient, type Shadow } from '../lib/css';
import { Checkbox, IconButton, NumberField, Popover, Section, Segmented, Select, Slider, TextField } from '../ui/controls';
import { ColorRow } from '../ui/ColorPicker';
import { edit, engineRef, preview, setEditor, setStyles, toast, useEditor } from './store';
import { call } from '../api';

const I = 13;

export function Inspector({ doc }: { doc: CardboardDocument }) {
  const page = activePage(doc);
  const index = indexPage(page);
  const ids = doc.selection.filter((id) => index.nodes.has(id));
  const nodes = ids.map((id) => index.nodes.get(id)!);
  useEditor((s) => s.view);
  return (
    <aside className="inspector">
      <InspectorHeader />
      <div className="inspector-scroll">
        {nodes.length === 0 ? (
          <PageInspector doc={doc} />
        ) : (
          <NodeInspector key={ids.join(',')} doc={doc} nodes={nodes} parentOf={(id) => index.parents.get(id) ?? null} />
        )}
      </div>
    </aside>
  );
}

function InspectorHeader() {
  const zoom = useEditor((s) => s.view.zoom);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const e = () => engineRef.current;
  const items: [string, string, () => void][] = [
    ['Zoom in', 'Ctrl +', () => e()?.zoomAt(1.25)],
    ['Zoom out', 'Ctrl −', () => e()?.zoomAt(0.8)],
    ['Zoom to 100%', 'Ctrl 0', () => e()?.zoomTo(1)],
    ['Zoom to fit', 'Shift 1', () => e()?.zoomToFit(false)],
    ['Zoom to selection', 'Shift 2', () => e()?.zoomToFit(true)],
  ];
  return (
    <div className="inspector-header">
      <span className="local-badge" title="Files are stored on this computer">
        Local
      </span>
      <span className="spacer" />
      <button ref={ref} type="button" className="zoom-btn" onClick={() => setOpen(!open)}>
        {Math.round(zoom * 100)}%<ChevronDown size={12} />
      </button>
      <button type="button" className="btn primary small" onClick={() => setEditor({ exportOpen: true })}>
        Export
      </button>
      {open && (
        <Popover anchor={ref.current} onClose={() => setOpen(false)} className="menu" width={200}>
          {items.map(([label, key, fn]) => (
            <button
              key={label}
              type="button"
              className="menu-item"
              onClick={() => {
                setOpen(false);
                fn();
              }}
            >
              <span>{label}</span>
              <span className="menu-key">{key}</span>
            </button>
          ))}
        </Popover>
      )}
    </div>
  );
}

function PageInspector({ doc }: { doc: CardboardDocument }) {
  const page = activePage(doc);
  const bg = page.background || '#282828';
  const [copied, setCopied] = useState(false);
  return (
    <>
      <Section title="Page">
        <ColorRow
          value={bg}
          onPreview={(v) => engineRef.current?.host.style.setProperty('--canvas-bg', v)}
          onPreviewEnd={() => engineRef.current?.host.style.setProperty('--canvas-bg', page.background || '#282828')}
          onCommit={(v) => edit([{ tool: 'update_pages', args: { updates: [{ pageId: page.id, background: v }] } }])}
        />
      </Section>
      <Section title="Agents">
        <p className="muted small">Agents edit this file live over MCP. Add this server to your agent's config:</p>
        <pre className="code-snippet">{`{ "cardboard": { "command": "cardboard", "args": ["mcp"] } }`}</pre>
        <button
          type="button"
          className="btn wide"
          onClick={() => {
            navigator.clipboard.writeText(JSON.stringify({ mcpServers: { cardboard: { command: 'cardboard', args: ['mcp'] } } }, null, 2));
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
        >
          {copied ? <Check size={I} /> : <Copy size={I} />} {copied ? 'Copied' : 'Copy MCP config'}
        </button>
      </Section>
    </>
  );
}

type Mode = 'fixed' | 'fit' | 'fill';

function NodeInspector({ doc, nodes, parentOf }: { doc: CardboardDocument; nodes: DesignNode[]; parentOf: (id: string) => string | null }) {
  const node = nodes[0];
  const ids = nodes.map((n) => n.id);
  const s = node.styles;
  const engine = engineRef.current;
  const artboard = parentOf(node.id) === null;
  const parentId = parentOf(node.id);
  const comp = engine?.computed(node.id);
  const parentComp = parentId ? engine?.computed(parentId) : null;
  const parentFlex = !!parentComp && parentComp.display.includes('flex');
  const parentRow = parentFlex && parentComp!.flexDirection.startsWith('row');
  const geo = engine?.geometry(node.id);
  const absolute = s.position === 'absolute' || s.position === 'fixed';
  const text = isTextLeaf(node);
  const set = (styles: Record<string, string>) => setStyles(ids, styles);
  const pv = (styles: Record<string, string>) => preview(ids, styles);
  const pvEnd = () => engineRef.current?.restore(ids);

  // ── layout helpers
  const mode = (axis: 'width' | 'height'): Mode => sizeMode(s, axis, parentFlex, parentRow);
  const setMode = (axis: 'width' | 'height', m: Mode) => {
    const main = axis === 'width' ? parentRow : parentFlex && !parentRow;
    const g = engine?.geometry(node.id);
    const styles: Record<string, string> = {};
    const minKey = axis === 'width' ? 'min-width' : 'min-height';
    if (m === 'fixed') {
      styles[axis] = `${Math.round(axis === 'width' ? g?.w ?? 100 : g?.h ?? 100)}px`;
      if (main) styles.flex = '';
      else if (parentFlex) styles['align-self'] = '';
      if (parentFlex) styles['flex-shrink'] = '0';
    } else if (m === 'fit') {
      styles[axis] = '';
      if (main) styles.flex = '';
      else styles['align-self'] = '';
    } else if (parentFlex && main) {
      styles[axis] = '';
      styles.flex = '1 1 0px';
      styles[minKey] = '0px';
    } else if (parentFlex) {
      styles[axis] = '';
      styles['align-self'] = 'stretch';
    } else styles[axis] = '100%';
    set(styles);
  };
  const setXY = (axis: 'x' | 'y', value: string) => {
    const n = parseFloat(value);
    if (!Number.isFinite(n)) return;
    if (artboard) {
      const pos = node.position ?? [geo?.x ?? 0, geo?.y ?? 0];
      edit([{ tool: 'set_positions', args: { updates: [{ nodeId: node.id, x: axis === 'x' ? n : pos[0], y: axis === 'y' ? n : pos[1] }] } }]);
    } else if (absolute) set({ [axis === 'x' ? 'left' : 'top']: `${n}px` });
    else
      set({
        position: 'absolute',
        left: `${axis === 'x' ? n : geo?.x ?? 0}px`,
        top: `${axis === 'y' ? n : geo?.y ?? 0}px`,
      });
  };
  const sizeOptions = () => [
    { value: 'fixed' as Mode, label: 'Fixed' },
    { value: 'fit' as Mode, label: 'Fit content' },
    { value: 'fill' as Mode, label: artboard ? 'Fill (100%)' : 'Fill container' },
  ];
  const sizeField = (axis: 'width' | 'height') => {
    const m = mode(axis);
    const val = m === 'fixed' && num(s[axis]) !== null ? String(num(s[axis])) : String(Math.round((axis === 'width' ? geo?.w : geo?.h) ?? 0));
    return (
      <div className="field-with-select">
        <NumberField
          label={axis === 'width' ? 'W' : 'H'}
          value={m === 'fixed' && num(s[axis]) === null ? s[axis] : val}
          min={0}
          onPreview={(v) => pv({ [axis]: v })}
          onPreviewEnd={pvEnd}
          onCommit={(v) => set({ [axis]: v, ...(parentFlex ? { 'flex-shrink': '0' } : {}), ...(m === 'fill' && ((axis === 'width') === parentRow) ? { flex: '' } : {}) })}
          suffix={m !== 'fixed' ? (m === 'fit' ? 'Fit' : 'Fill') : undefined}
        />
        <Select value={m} options={sizeOptions()} onChange={(v) => setMode(axis, v)} className="mini" width={140} />
      </div>
    );
  };

  const isFlex = s.display === 'flex' || s.display === 'inline-flex';
  const isGrid = s.display === 'grid';
  const canLayout = !text && !['img', 'svg', 'input', 'hr', 'br'].includes(node.tag);

  return (
    <>
      <Section
        title={
          <span className="layer-title">
            {nodes.length > 1 ? `${nodes.length} layers` : displayName(node)}
            <span className="tag">{nodes.length > 1 ? 'mixed' : `<${node.tag}>`}</span>
          </span>
        }
        actions={
          <IconButton
            icon={node.visible ? <Eye size={I} /> : <EyeOff size={I} />}
            title={node.visible ? 'Hide' : 'Show'}
            onClick={() => edit([{ tool: 'set_node_state', args: { updates: ids.map((id) => ({ nodeId: id, visible: !node.visible })) } }])}
          />
        }
        className="title-section"
      />
      <Section title="Layout">
        <div className="grid3">
          <NumberField label="X" value={String(round(artboard ? node.position?.[0] ?? geo?.x ?? 0 : geo?.x ?? 0))} onCommit={(v) => setXY('x', v)} />
          <NumberField label="Y" value={String(round(artboard ? node.position?.[1] ?? geo?.y ?? 0 : geo?.y ?? 0))} onCommit={(v) => setXY('y', v)} />
          <NumberField
            label={<RotateCw size={11} />}
            title="Rotation"
            value={String(num(s.rotate) ?? 0)}
            suffix="°"
            format={(n) => (n ? `${n}deg` : '')}
            onPreview={(v) => pv({ rotate: v })}
            onPreviewEnd={pvEnd}
            onCommit={(v) => set({ rotate: v })}
          />
        </div>
        <div className="grid2">
          {sizeField('width')}
          {sizeField('height')}
        </div>
        {!artboard && (
          <Checkbox
            checked={absolute}
            label="Absolute position"
            onChange={(on) => {
              if (on) {
                const styles: Record<string, string> = { position: 'absolute', left: `${geo?.x ?? 0}px`, top: `${geo?.y ?? 0}px` };
                const ps = parentId ? doc.pages.flatMap((p) => p.artboards).length && engine?.computed(parentId) : null;
                const ops = [{ tool: 'update_styles', args: { updates: [{ nodeIds: ids, styles }] } }];
                if (parentId && ps && ps.position === 'static') ops.push({ tool: 'update_styles', args: { updates: [{ nodeIds: [parentId], styles: { position: 'relative' } }] } });
                edit(ops);
              } else set({ position: '', left: '', top: '', right: '', bottom: '' });
            }}
          />
        )}
        {canLayout && <Checkbox checked={s.overflow === 'hidden' || s.overflow === 'clip'} label="Clip content" hint="Alt C" onChange={(on) => set({ overflow: on ? 'hidden' : '' })} />}
      </Section>
      {canLayout && (isFlex || isGrid ? <FlexSection node={node} set={set} pv={pv} pvEnd={pvEnd} /> : (
        <Section title="Flex" empty onAdd={() => set({ display: 'flex', 'flex-direction': 'column', gap: s.gap || '0px' })} actions={<IconButton icon={<Plus size={I} />} title="Add flex (Shift A)" onClick={() => set({ display: 'flex', 'flex-direction': 'column' })} />} />
      ))}
      {canLayout && !isFlex && !isGrid && <PaddingOnly node={node} set={set} pv={pv} pvEnd={pvEnd} />}
      {(text || node.text) && <TypographySection doc={doc} node={node} comp={comp ?? null} set={set} pv={pv} pvEnd={pvEnd} />}
      {node.markup != null && (
        <Section title="Icon" actions={<span className="muted small">{node.attributes['data-icon']}</span>}>
          <ColorRow value={s.color ?? (comp ? comp.color : '#000000')} tokens={doc.tokens} onPreview={(v) => pv({ color: v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ color: v })} />
        </Section>
      )}
      <RadiusSection node={node} set={set} pv={pv} pvEnd={pvEnd} />
      <BlendingSection node={node} set={set} pv={pv} pvEnd={pvEnd} ids={ids} />
      {!text && <FillSection doc={doc} node={node} set={set} pv={pv} pvEnd={pvEnd} />}
      <BorderSection doc={doc} node={node} set={set} pv={pv} pvEnd={pvEnd} />
      <ShadowSection doc={doc} node={node} set={set} pv={pv} pvEnd={pvEnd} inset={false} />
      <ShadowSection doc={doc} node={node} set={set} pv={pv} pvEnd={pvEnd} inset />
      <FiltersSection node={node} set={set} pv={pv} pvEnd={pvEnd} />
      {(node.tag === 'img' || node.tag === 'a') && <AttributesSection node={node} />}
      <CssSection node={node} ids={ids} />
      <Section
        title="Export"
        actions={
          <>
            <IconButton icon={<Copy size={I} />} title="Copy as HTML" onClick={() => copyHtml(node)} />
          </>
        }
      >
        <div className="grid2">
          <button type="button" className="btn" onClick={() => copyHtml(node)}>
            Copy HTML
          </button>
          <button
            type="button"
            className="btn"
            onClick={async () => {
              const r = await call<{ jsx: string }>('get_jsx', { fileId: doc.id, nodeId: node.id, maxChars: 0 });
              if (r) {
                await navigator.clipboard.writeText(r.jsx);
                toast('JSX copied');
              }
            }}
          >
            Copy JSX
          </button>
        </div>
      </Section>
    </>
  );
}

export function copyHtml(node: DesignNode) {
  navigator.clipboard.writeText(nodeToHtml(node));
  toast('HTML copied');
}

interface SectionProps {
  node: DesignNode;
  set: (styles: Record<string, string>) => void;
  pv: (styles: Record<string, string>) => void;
  pvEnd: () => void;
}

const JUSTIFY = ['flex-start', 'center', 'flex-end'];
function FlexSection({ node, set, pv, pvEnd }: SectionProps) {
  const s = node.styles;
  const row = !(s['flex-direction'] ?? 'row').startsWith('column');
  const justify = s['justify-content'] ?? 'flex-start';
  const align = s['align-items'] ?? 'stretch';
  const spaced = justify === 'space-between';
  const [pt, pr, pb, pl] = boxSides(s.padding);
  const [expanded, setExpanded] = useState(false);
  const norm = (v: string) => (v === 'start' ? 'flex-start' : v === 'end' ? 'flex-end' : v);
  const cell = (main: number, cross: number) => {
    const j = JUSTIFY[main];
    const a = JUSTIFY[cross];
    const active = (spaced || norm(justify) === j) && norm(align) === a;
    return (
      <button
        key={`${main}${cross}`}
        type="button"
        className={`align-cell ${active ? 'active' : ''}`}
        onClick={() => set({ 'justify-content': spaced ? 'space-between' : j, 'align-items': a })}
      >
        <span />
      </button>
    );
  };
  const cells = [];
  for (let y = 0; y < 3; y++) for (let x = 0; x < 3; x++) cells.push(row ? cell(x, y) : cell(y, x));
  const pad = (side: string) => (v: string) => set({ padding: '', [`padding-${side}`]: '', ...expandPadding(s, side, v) });
  return (
    <Section title={s.display === 'grid' ? 'Grid' : 'Flex'} actions={<IconButton icon={<Minus size={I} />} title="Remove flex" onClick={() => set({ display: '', 'flex-direction': '', 'justify-content': '', 'align-items': '', gap: '', 'flex-wrap': '' })} />}>
      <div className="flex-grid">
        <div className={`align-grid ${row ? 'row' : 'col'} ${spaced ? 'spaced' : ''}`}>{cells}</div>
        <div className="flex-side">
          <div className="flex-row-controls">
            <Segmented
              value={row ? 'row' : 'column'}
              options={[
                { value: 'column', label: <ArrowDown size={I} />, title: 'Vertical' },
                { value: 'row', label: <ArrowRight size={I} />, title: 'Horizontal' },
              ]}
              onChange={(v) => set({ 'flex-direction': v })}
            />
            <IconButton icon={<WrapText size={I} />} title="Wrap" active={s['flex-wrap'] === 'wrap'} onClick={() => set({ 'flex-wrap': s['flex-wrap'] === 'wrap' ? '' : 'wrap' })} />
          </div>
          <div className="flex-row-controls">
            <NumberField
              label={<span className="gap-icon">{row ? '⇹' : '⇳'}</span>}
              title="Gap"
              value={spaced ? 'Auto' : String(num(s.gap) ?? 0)}
              min={0}
              onPreview={(v) => pv({ gap: v })}
              onPreviewEnd={pvEnd}
              onCommit={(v) => set({ gap: v, ...(spaced ? { 'justify-content': 'flex-start' } : {}) })}
            />
            <IconButton icon={<AlignCenterHorizontal size={I} />} title="Space between" active={spaced} onClick={() => set({ 'justify-content': spaced ? 'flex-start' : 'space-between' })} />
          </div>
        </div>
      </div>
      {expanded ? (
        <div className="grid2">
          {(['top', 'right', 'bottom', 'left'] as const).map((side, i) => (
            <NumberField key={side} label={side[0].toUpperCase()} title={`Padding ${side}`} value={String(num(s[`padding-${side}`] ?? [pt, pr, pb, pl][i]) ?? 0)} min={0} onCommit={pad(side)} />
          ))}
        </div>
      ) : (
        <div className="grid2 with-action">
          <NumberField
            label={<span className="pad-icon h" />}
            title="Horizontal padding"
            value={String(num(s['padding-left'] ?? pl) ?? 0)}
            min={0}
            onPreview={(v) => pv({ 'padding-left': v, 'padding-right': v })}
            onPreviewEnd={pvEnd}
            onCommit={(v) => set({ ...expandPadding(s, 'left', v), ...expandPadding({ ...s, ...expandPadding(s, 'left', v), padding: '' }, 'right', v), padding: '' })}
          />
          <NumberField
            label={<span className="pad-icon v" />}
            title="Vertical padding"
            value={String(num(s['padding-top'] ?? pt) ?? 0)}
            min={0}
            onPreview={(v) => pv({ 'padding-top': v, 'padding-bottom': v })}
            onPreviewEnd={pvEnd}
            onCommit={(v) => set({ ...expandPadding(s, 'top', v), ...expandPadding({ ...s, ...expandPadding(s, 'top', v), padding: '' }, 'bottom', v), padding: '' })}
          />
          <IconButton icon={<Scan size={I} />} title="Individual sides" onClick={() => setExpanded(true)} />
        </div>
      )}
    </Section>
  );
}

/** Padding as four longhands (shorthand removed) with one side replaced. */
function expandPadding(s: Record<string, string>, side: string, v: string): Record<string, string> {
  const [t, r, b, l] = boxSides(s.padding);
  const out: Record<string, string> = {
    'padding-top': s['padding-top'] ?? t,
    'padding-right': s['padding-right'] ?? r,
    'padding-bottom': s['padding-bottom'] ?? b,
    'padding-left': s['padding-left'] ?? l,
  };
  out[`padding-${side}`] = v || '0px';
  for (const k of Object.keys(out)) if (out[k] === '0' ) out[k] = '0px';
  return out;
}

function PaddingOnly({ node, set }: SectionProps) {
  const s = node.styles;
  const [pt, pr, pb, pl] = boxSides(s.padding);
  if (!s.padding && !s['padding-top'] && !s['padding-left'] && !s['padding-right'] && !s['padding-bottom']) return null;
  return (
    <Section title="Padding">
      <div className="grid2">
        {(['top', 'right', 'bottom', 'left'] as const).map((side, i) => (
          <NumberField key={side} label={side[0].toUpperCase()} value={String(num(s[`padding-${side}`] ?? [pt, pr, pb, pl][i]) ?? 0)} min={0} onCommit={(v) => set({ ...expandPadding(s, side, v), padding: '' })} />
        ))}
      </div>
    </Section>
  );
}

const FONTS = ['Inter', 'system-ui', 'Arial', 'Helvetica', 'Georgia', 'Times New Roman', 'Courier New', 'ui-monospace', 'ui-serif', 'ui-sans-serif'];
const WEIGHTS = [
  ['100', 'Thin'],
  ['200', 'Extra Light'],
  ['300', 'Light'],
  ['400', 'Regular'],
  ['500', 'Medium'],
  ['600', 'Semi Bold'],
  ['700', 'Bold'],
  ['800', 'Extra Bold'],
  ['900', 'Black'],
] as const;

function TypographySection({ doc, node, comp, set, pv, pvEnd }: SectionProps & { doc: CardboardDocument; comp: CSSStyleDeclaration | null }) {
  const s = node.styles;
  const family = s['font-family'] ?? comp?.fontFamily ?? '';
  const weight = s['font-weight'] ?? comp?.fontWeight ?? '400';
  const size = s['font-size'] ?? comp?.fontSize ?? '16px';
  const lh = s['line-height'] ?? '';
  const ls = s['letter-spacing'] ?? '';
  const align = s['text-align'] ?? 'left';
  const [fontOpen, setFontOpen] = useState(false);
  const fref = useRef<HTMLButtonElement>(null);
  const display = family.split(',')[0].replace(/["']/g, '').trim() || 'Default';
  return (
    <Section title="Typography">
      <button ref={fref} type="button" className="select wide" onClick={() => setFontOpen(!fontOpen)}>
        <span className="select-value" style={{ fontFamily: family }}>
          {display}
        </span>
        <ChevronDown size={12} className="select-chevron" />
      </button>
      {fontOpen && (
        <Popover anchor={fref.current} onClose={() => setFontOpen(false)} className="menu" width={240}>
          <div className="menu-input">
            <TextField value={family} placeholder="Font family" autoFocus onCommit={(v) => (set({ 'font-family': v }), setFontOpen(false))} />
          </div>
          {FONTS.map((f) => (
            <button key={f} type="button" className="menu-item" style={{ fontFamily: f }} onClick={() => (set({ 'font-family': f.includes(' ') ? `"${f}"` : f }), setFontOpen(false))}>
              <span className="menu-check">{display === f && <Check size={12} />}</span>
              {f}
            </button>
          ))}
        </Popover>
      )}
      <div className="grid2">
        <Select
          value={String(weight) as (typeof WEIGHTS)[number][0]}
          options={WEIGHTS.map(([v, l]) => ({ value: v, label: l }))}
          onChange={(v) => set({ 'font-weight': v })}
          placeholder={String(weight)}
        />
        <NumberField label="Aa" title="Font size" value={String(num(size) ?? size)} min={1} onPreview={(v) => pv({ 'font-size': v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ 'font-size': v })} />
      </div>
      <div className="grid2">
        <NumberField
          label="LH"
          title="Line height (unitless multiplier or px)"
          value={lh || 'Auto'}
          step={0.05}
          format={(n) => String(n)}
          onPreview={(v) => pv({ 'line-height': v })}
          onPreviewEnd={pvEnd}
          onCommit={(v) => set({ 'line-height': v === 'Auto' ? '' : v })}
        />
        <NumberField
          label="LS"
          title="Letter spacing"
          value={String(num(ls) ?? (ls || 0))}
          step={0.1}
          onPreview={(v) => pv({ 'letter-spacing': v })}
          onPreviewEnd={pvEnd}
          onCommit={(v) => set({ 'letter-spacing': v === '0px' ? '' : v })}
        />
      </div>
      <Segmented
        className="wide"
        value={align === 'start' ? 'left' : align}
        options={[
          { value: 'left', label: <AlignLeft size={I} />, title: 'Left' },
          { value: 'center', label: <AlignCenter size={I} />, title: 'Center' },
          { value: 'right', label: <AlignRight size={I} />, title: 'Right' },
          { value: 'justify', label: <AlignJustify size={I} />, title: 'Justify' },
        ]}
        onChange={(v) => set({ 'text-align': v === 'left' ? '' : v })}
      />
      <ColorRow value={s.color ?? (comp ? comp.color : '#000000')} tokens={doc.tokens} onPreview={(v) => pv({ color: v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ color: v })} />
    </Section>
  );
}

function RadiusSection({ node, set, pv, pvEnd }: SectionProps) {
  const s = node.styles;
  const r = num(s['border-radius']) ?? 0;
  const [expanded, setExpanded] = useState(false);
  const corners = ['top-left', 'top-right', 'bottom-right', 'bottom-left'] as const;
  const sides = boxSides(s['border-radius']);
  return (
    <Section title="Radius" actions={<IconButton icon={<SquareDashed size={I} />} title="Individual corners" active={expanded} onClick={() => setExpanded(!expanded)} />}>
      {expanded ? (
        <div className="grid2">
          {corners.map((c, i) => (
            <NumberField
              key={c}
              label={c.split('-').map((w) => w[0].toUpperCase()).join('')}
              value={String(num(s[`border-${c}-radius`] ?? sides[i]) ?? 0)}
              min={0}
              onCommit={(v) => {
                const all = Object.fromEntries(corners.map((k, j) => [`border-${k}-radius`, s[`border-${k}-radius`] ?? sides[j] ?? '0px']));
                all[`border-${c}-radius`] = v || '0px';
                set({ 'border-radius': '', ...all });
              }}
            />
          ))}
        </div>
      ) : (
        <div className="slider-row">
          <Slider
            value={r}
            min={0}
            max={Math.max(100, r)}
            onPreview={(v) => pv({ 'border-radius': `${v}px` })}
            onCommit={(v) => {
              pvEnd();
              set({ 'border-radius': v ? `${v}px` : '' });
            }}
          />
          <NumberField value={s['border-radius'] && num(s['border-radius']) === null ? s['border-radius'] : String(r)} min={0} onPreview={(v) => pv({ 'border-radius': v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ 'border-radius': v === '0px' ? '' : v })} />
        </div>
      )}
    </Section>
  );
}

const BLENDS = ['normal', 'multiply', 'screen', 'overlay', 'darken', 'lighten', 'color-dodge', 'color-burn', 'hard-light', 'soft-light', 'difference', 'exclusion', 'hue', 'saturation', 'color', 'luminosity'];
function BlendingSection({ node, set, pv, pvEnd, ids }: SectionProps & { ids: string[] }) {
  const s = node.styles;
  const opacity = s.opacity != null ? Math.round(parseFloat(s.opacity) * 100) : 100;
  return (
    <Section
      title="Blending"
      actions={
        <IconButton
          icon={node.visible ? <Eye size={I} /> : <EyeOff size={I} />}
          title="Toggle visibility"
          onClick={() => edit([{ tool: 'set_node_state', args: { updates: ids.map((id) => ({ nodeId: id, visible: !node.visible })) } }])}
        />
      }
    >
      <div className="grid2">
        <NumberField
          label={<Droplet size={11} />}
          title="Opacity"
          value={String(opacity)}
          suffix="%"
          min={0}
          max={100}
          format={(n) => (n >= 100 ? '' : String(round(n / 100, 3)))}
          onPreview={(v) => pv({ opacity: v || '1' })}
          onPreviewEnd={pvEnd}
          onCommit={(v) => set({ opacity: v })}
        />
        <Select
          icon={<Blend size={12} />}
          value={s['mix-blend-mode'] ?? 'normal'}
          options={BLENDS.map((b) => ({ value: b, label: b[0].toUpperCase() + b.slice(1).replace('-', ' ') }))}
          onChange={(v) => set({ 'mix-blend-mode': v === 'normal' ? '' : v })}
        />
      </div>
    </Section>
  );
}

function FillSection({ doc, node, set, pv, pvEnd }: SectionProps & { doc: CardboardDocument }) {
  const s = node.styles;
  const bg = s.background ?? '';
  const bgColor = s['background-color'] ?? '';
  const image = s['background-image'] ?? (/url\(|gradient\(/.test(bg) ? bg : '');
  const gradient = parseGradient(image);
  const hasUrl = /url\(/.test(image);
  const solid = bgColor || (!image && bg ? bg : '');
  const kind: 'solid' | 'gradient' | 'image' | null = gradient ? 'gradient' : hasUrl ? 'image' : solid ? 'solid' : null;
  const clearAll = { background: '', 'background-color': '', 'background-image': '', 'background-size': '', 'background-position': '', 'background-repeat': '' };
  const setKind = (k: 'solid' | 'gradient' | 'image') => {
    const base = parseColor(solid) ? solid : '#d9d9d9';
    if (k === 'solid') set({ ...clearAll, 'background-color': gradient?.stops[0]?.color ?? base });
    if (k === 'gradient') set({ ...clearAll, 'background-image': formatGradient({ kind: 'linear', angle: 180, stops: [{ color: base, pos: 0 }, { color: '#000000', pos: 100 }] }) });
    if (k === 'image') pickImage((url) => set({ ...clearAll, 'background-image': `url("${url}")`, 'background-size': 'cover', 'background-position': 'center' }));
  };
  if (!kind) {
    return <Section title="Fill" empty onAdd={() => set({ 'background-color': '#ffffff' })} actions={<IconButton icon={<Plus size={I} />} title="Add fill" onClick={() => set({ 'background-color': '#ffffff' })} />} />;
  }
  return (
    <Section title="Fill" actions={<IconButton icon={<Minus size={I} />} title="Remove fill" onClick={() => set(clearAll)} />}>
      <Segmented
        className="fill-kinds"
        value={kind}
        options={[
          { value: 'solid', label: <span className="fill-kind solid" />, title: 'Solid' },
          { value: 'gradient', label: <span className="fill-kind gradient" />, title: 'Gradient' },
          { value: 'image', label: <ImageIcon size={I} />, title: 'Image' },
        ]}
        onChange={setKind}
      />
      {kind === 'solid' && (
        <ColorRow
          value={solid}
          tokens={doc.tokens}
          onPreview={(v) => pv({ 'background-color': v, background: '' })}
          onPreviewEnd={pvEnd}
          onCommit={(v) => set({ 'background-color': v, background: '' })}
        />
      )}
      {kind === 'gradient' && gradient && <GradientEditor doc={doc} gradient={gradient} onPreview={(g) => pv({ 'background-image': formatGradient(g), background: '' })} onPreviewEnd={pvEnd} onCommit={(g) => set({ 'background-image': formatGradient(g), background: '' })} />}
      {kind === 'image' && (
        <div className="image-fill">
          <div className="image-thumb checker" style={{ backgroundImage: image, backgroundSize: 'cover', backgroundPosition: 'center' }} />
          <div className="image-actions">
            <Select
              value={(s['background-size'] ?? 'cover') as 'cover'}
              options={[
                { value: 'cover', label: 'Fill' },
                { value: 'contain', label: 'Fit' },
                { value: 'auto', label: 'Original' },
              ]}
              onChange={(v) => set({ 'background-size': v, 'background-repeat': v === 'auto' ? 'repeat' : 'no-repeat' })}
            />
            <button type="button" className="btn" onClick={() => setKind('image')}>
              Replace…
            </button>
          </div>
        </div>
      )}
    </Section>
  );
}

function GradientEditor({ doc, gradient, onPreview, onPreviewEnd, onCommit }: { doc: CardboardDocument; gradient: Gradient; onPreview: (g: Gradient) => void; onPreviewEnd: () => void; onCommit: (g: Gradient) => void }) {
  const bar = useRef<HTMLDivElement>(null);
  const drag = (i: number) => (e: React.PointerEvent) => {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    let g = gradient;
    const move = (ev: PointerEvent) => {
      const r = bar.current!.getBoundingClientRect();
      const pos = Math.round(Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)) * 100);
      g = { ...gradient, stops: gradient.stops.map((s, j) => (j === i ? { ...s, pos } : s)) };
      onPreview(g);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      onCommit({ ...g, stops: [...g.stops].sort((a, b) => a.pos - b.pos) });
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };
  const preview = formatGradient({ ...gradient, kind: 'linear', angle: 90 });
  return (
    <div className="gradient">
      <div className="gradient-bar checker" ref={bar}>
        <div style={{ background: preview }} />
        {gradient.stops.map((s, i) => (
          <span key={i} className="gradient-stop" style={{ left: `${s.pos}%` }} onPointerDown={drag(i)}>
            <span style={{ background: s.color }} />
          </span>
        ))}
      </div>
      <div className="grid2">
        <Select
          value={gradient.kind}
          options={[
            { value: 'linear', label: 'Linear' },
            { value: 'radial', label: 'Radial' },
          ]}
          onChange={(kind) => onCommit({ ...gradient, kind })}
        />
        {gradient.kind === 'linear' && (
          <NumberField label="∠" title="Angle" value={String(gradient.angle)} suffix="°" format={(n) => String(n)} onPreview={(v) => onPreview({ ...gradient, angle: parseFloat(v) })} onPreviewEnd={onPreviewEnd} onCommit={(v) => onCommit({ ...gradient, angle: parseFloat(v) || 0 })} />
        )}
      </div>
      {gradient.stops.map((stop, i) => (
        <ColorRow
          key={i}
          value={stop.color}
          tokens={doc.tokens}
          onPreview={(c) => onPreview({ ...gradient, stops: gradient.stops.map((s, j) => (j === i ? { ...s, color: c } : s)) })}
          onPreviewEnd={onPreviewEnd}
          onCommit={(c) => onCommit({ ...gradient, stops: gradient.stops.map((s, j) => (j === i ? { ...s, color: c } : s)) })}
          actions={
            gradient.stops.length > 2 && <IconButton icon={<Minus size={I} />} title="Remove stop" onClick={() => onCommit({ ...gradient, stops: gradient.stops.filter((_, j) => j !== i) })} />
          }
        />
      ))}
      <button type="button" className="btn ghost small" onClick={() => onCommit({ ...gradient, stops: [...gradient.stops, { color: '#ffffff', pos: 50 }].sort((a, b) => a.pos - b.pos) })}>
        <Plus size={12} /> Add stop
      </button>
    </div>
  );
}

function pickImage(done: (url: string) => void) {
  const input = document.createElement('input');
  input.type = 'file';
  input.accept = 'image/*';
  input.onchange = () => {
    const f = input.files?.[0];
    if (!f) return;
    if (f.size > 8 * 1024 * 1024) return toast('Images over 8 MB are not supported yet');
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result));
    reader.readAsDataURL(f);
  };
  input.click();
}
export { pickImage };

function BorderSection({ doc, node, set, pv, pvEnd }: SectionProps & { doc: CardboardDocument }) {
  const s = node.styles;
  const parts = s.border ? s.border.split(/\s+(?![^(]*\))/) : [];
  const width = s['border-width'] ?? parts.find((p) => num(p) !== null) ?? '';
  const style = s['border-style'] ?? parts.find((p) => ['solid', 'dashed', 'dotted', 'double'].includes(p)) ?? 'solid';
  const color = s['border-color'] ?? parts.find((p) => num(p) === null && !['solid', 'dashed', 'dotted', 'double', 'none'].includes(p)) ?? '#000000';
  const has = !!(s.border && s.border !== 'none') || !!s['border-width'];
  const write = (w: string, st: string, c: string) => set({ border: `${w || '1px'} ${st} ${c}`, 'border-width': '', 'border-style': '', 'border-color': '' });
  if (!has) return <Section title="Border" empty onAdd={() => write('1px', 'solid', '#000000')} actions={<IconButton icon={<Plus size={I} />} title="Add border" onClick={() => write('1px', 'solid', '#000000')} />} />;
  return (
    <Section title="Border" actions={<IconButton icon={<Minus size={I} />} title="Remove border" onClick={() => set({ border: '', 'border-width': '', 'border-style': '', 'border-color': '' })} />}>
      <ColorRow value={color} tokens={doc.tokens} onPreview={(c) => pv({ 'border-color': c })} onPreviewEnd={pvEnd} onCommit={(c) => write(width, style, c)} />
      <div className="grid2">
        <NumberField label="W" title="Border width" value={String(num(width) ?? 1)} min={0} onPreview={(v) => pv({ 'border-width': v })} onPreviewEnd={pvEnd} onCommit={(v) => write(v, style, color)} />
        <Select
          value={style as 'solid'}
          options={['solid', 'dashed', 'dotted', 'double'].map((v) => ({ value: v as 'solid', label: v[0].toUpperCase() + v.slice(1) }))}
          onChange={(v) => write(width, v, color)}
        />
      </div>
    </Section>
  );
}

function ShadowSection({ doc, node, set, pv, pvEnd, inset }: SectionProps & { doc: CardboardDocument; inset: boolean }) {
  const all = parseShadows(node.styles['box-shadow']);
  const mine = all.filter((s) => s.inset === inset);
  const others = all.filter((s) => s.inset !== inset);
  const write = (list: Shadow[]) => formatShadows([...(inset ? others : list), ...(inset ? list : others)]) || '';
  const add = () => set({ 'box-shadow': write([...mine, { inset, x: 0, y: inset ? 2 : 4, blur: inset ? 4 : 12, spread: 0, color: 'rgba(0, 0, 0, 0.25)' }]) });
  const title = inset ? 'Inner shadow' : 'Shadow';
  if (!mine.length) return <Section title={title} empty onAdd={add} actions={<IconButton icon={<Plus size={I} />} title={`Add ${title.toLowerCase()}`} onClick={add} />} />;
  return (
    <Section title={title} actions={<IconButton icon={<Plus size={I} />} title={`Add ${title.toLowerCase()}`} onClick={add} />}>
      {mine.map((sh, i) => {
        const upd = (patch: Partial<Shadow>) => mine.map((m, j) => (j === i ? { ...m, ...patch } : m));
        const field = (k: 'x' | 'y' | 'blur' | 'spread', label: string) => (
          <NumberField
            label={label}
            value={String(sh[k])}
            min={k === 'blur' ? 0 : undefined}
            format={(n) => String(n)}
            onPreview={(v) => pv({ 'box-shadow': write(upd({ [k]: parseFloat(v) })) })}
            onPreviewEnd={pvEnd}
            onCommit={(v) => set({ 'box-shadow': write(upd({ [k]: parseFloat(v) || 0 })) })}
          />
        );
        return (
          <div className="shadow" key={i}>
            <ColorRow
              value={sh.color}
              tokens={doc.tokens}
              onPreview={(c) => pv({ 'box-shadow': write(upd({ color: c })) })}
              onPreviewEnd={pvEnd}
              onCommit={(c) => set({ 'box-shadow': write(upd({ color: c })) })}
              actions={<IconButton icon={<Minus size={I} />} title="Remove" onClick={() => set({ 'box-shadow': write(mine.filter((_, j) => j !== i)) })} />}
            />
            <div className="grid4">
              {field('x', 'X')}
              {field('y', 'Y')}
              {field('blur', 'B')}
              {field('spread', 'S')}
            </div>
          </div>
        );
      })}
    </Section>
  );
}

function FiltersSection({ node, set, pv, pvEnd }: SectionProps) {
  const s = node.styles;
  const blur = /blur\(([\d.]+)px\)/.exec(s.filter ?? '')?.[1];
  const bblur = /blur\(([\d.]+)px\)/.exec(s['backdrop-filter'] ?? '')?.[1];
  if (blur == null && bblur == null) {
    return <Section title="Filters" empty onAdd={() => set({ filter: 'blur(4px)' })} actions={<IconButton icon={<Plus size={I} />} title="Add layer blur" onClick={() => set({ filter: 'blur(4px)' })} />} />;
  }
  return (
    <Section title="Filters" actions={<IconButton icon={<Minus size={I} />} title="Remove filters" onClick={() => set({ filter: '', 'backdrop-filter': '' })} />}>
      <div className="grid2">
        <NumberField label="Blur" value={blur ?? '0'} min={0} format={(n) => (n ? `blur(${n}px)` : '')} onPreview={(v) => pv({ filter: v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ filter: v })} />
        <NumberField label="BG" title="Background blur" value={bblur ?? '0'} min={0} format={(n) => (n ? `blur(${n}px)` : '')} onPreview={(v) => pv({ 'backdrop-filter': v })} onPreviewEnd={pvEnd} onCommit={(v) => set({ 'backdrop-filter': v })} />
      </div>
    </Section>
  );
}

function AttributesSection({ node }: { node: DesignNode }) {
  const setAttr = (k: string, v: string) => edit([{ tool: 'set_attributes', args: { updates: [{ nodeId: node.id, attributes: { [k]: v } }] } }]);
  return (
    <Section title={node.tag === 'img' ? 'Image' : 'Link'}>
      {node.tag === 'img' && (
        <>
          <div className="grid2">
            <button type="button" className="btn" onClick={() => pickImage((url) => setAttr('src', url))}>
              Replace image…
            </button>
            <Select
              value={(node.styles['object-fit'] ?? 'fill') as 'fill'}
              options={['fill', 'cover', 'contain', 'none'].map((v) => ({ value: v as 'fill', label: v[0].toUpperCase() + v.slice(1) }))}
              onChange={(v) => setStyles([node.id], { 'object-fit': v === 'fill' ? '' : v })}
            />
          </div>
          <TextField label="Alt" value={node.attributes.alt ?? ''} onCommit={(v) => setAttr('alt', v)} />
        </>
      )}
      {node.tag === 'a' && <TextField label="URL" value={node.attributes.href ?? ''} onCommit={(v) => setAttr('href', v)} />}
    </Section>
  );
}

function CssSection({ node, ids }: { node: DesignNode; ids: string[] }) {
  const css = useMemo(() => Object.entries(node.styles).map(([k, v]) => `${k}: ${v};`).join('\n'), [node.styles]);
  const [draft, setDraft] = useState<string | null>(null);
  const [open, setOpen] = useState<boolean>(false);
  const commit = () => {
    if (draft === null) return;
    const next: Record<string, string> = {};
    for (const decl of draft.split(/;\s*(?![^(]*\))/)) {
      const i = decl.indexOf(':');
      if (i < 0) continue;
      const k = decl.slice(0, i).trim();
      const v = decl.slice(i + 1).trim();
      if (k) next[k] = v;
    }
    const patch: Record<string, string> = { ...next };
    for (const k of Object.keys(node.styles)) if (!(k in next)) patch[k] = '';
    setDraft(null);
    setStyles(ids, patch);
  };
  return (
    <Section title="CSS" actions={<IconButton icon={open ? <Minus size={I} /> : <Plus size={I} />} title={open ? 'Hide' : 'Show authored CSS'} onClick={() => setOpen(!open)} />} empty={!open} onAdd={() => setOpen(true)}>
      {open && (
        <textarea
          className="css-editor mono"
          value={draft ?? css}
          spellCheck={false}
          rows={Math.min(16, Math.max(4, (draft ?? css).split('\n').length + 1))}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) commit();
            if (e.key === 'Escape') setDraft(null);
          }}
        />
      )}
    </Section>
  );
}
