import { useEffect, useRef, useState } from 'react';
import { Pipette } from 'lucide-react';
import { formatColor, hsvToRgb, parseColor, rgbToHsv, toHex, type HSVA } from '../lib/css';
import { Popover } from './controls';

/** HSV picker. `onPreview` fires while dragging; `onCommit` once per gesture or typed value. */
export function ColorPicker(props: {
  value: string;
  onPreview: (css: string) => void;
  onCommit: (css: string) => void;
  tokens?: Record<string, string>;
}) {
  const initial = parseColor(props.value) ?? { r: 0, g: 0, b: 0, a: 1 };
  const [hsv, setHsv] = useState<HSVA>(() => rgbToHsv(initial));
  const hsvRef = useRef(hsv);
  hsvRef.current = hsv;
  // Follow external changes (undo, agent edits) unless they echo our own value.
  const lastEmitted = useRef(props.value);
  useEffect(() => {
    if (props.value === lastEmitted.current) return;
    const c = parseColor(props.value);
    if (c) setHsv(rgbToHsv(c));
    lastEmitted.current = props.value;
  }, [props.value]);
  const css = (h: HSVA) => formatColor(hsvToRgb(h));
  const update = (h: HSVA, commit: boolean) => {
    setHsv(h);
    const v = css(h);
    lastEmitted.current = v;
    if (commit) props.onCommit(v);
    else props.onPreview(v);
  };
  const drag = (e: React.PointerEvent, fn: (fx: number, fy: number) => HSVA) => {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const at = (ev: { clientX: number; clientY: number }) => {
      const r = el.getBoundingClientRect();
      return fn(Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)), Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height)));
    };
    update(at(e), false);
    const move = (ev: PointerEvent) => update(at(ev), false);
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      props.onCommit(css(hsvRef.current));
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  };
  const rgb = hsvToRgb(hsv);
  const pure = formatColor(hsvToRgb({ h: hsv.h, s: 1, v: 1, a: 1 }));
  const opaque = formatColor({ ...rgb, a: 1 });
  const [hex, setHex] = useState<string | null>(null);
  const colorTokens = Object.entries(props.tokens ?? {}).filter(([, v]) => parseColor(v));
  const eyedropper = 'EyeDropper' in window;
  return (
    <div className="picker">
      <div className="picker-sv" style={{ background: pure }} onPointerDown={(e) => drag(e, (fx, fy) => ({ ...hsvRef.current, s: fx, v: 1 - fy }))}>
        <div className="picker-sv-white" />
        <div className="picker-sv-black" />
        <div className="picker-dot" style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%`, background: opaque }} />
      </div>
      <div className="picker-row">
        {eyedropper && (
          <button
            type="button"
            className="icon-btn"
            title="Pick a color from the screen"
            onClick={async () => {
              try {
                const r = await new (window as any).EyeDropper().open();
                const c = parseColor(r.sRGBHex);
                if (c) update(rgbToHsv({ ...c, a: hsvRef.current.a }), true);
              } catch {
                /* cancelled */
              }
            }}
          >
            <Pipette size={14} />
          </button>
        )}
        <div className="picker-sliders">
          <div className="picker-hue" onPointerDown={(e) => drag(e, (fx) => ({ ...hsvRef.current, h: fx * 360 }))}>
            <div className="picker-knob" style={{ left: `${(hsv.h / 360) * 100}%`, background: pure }} />
          </div>
          <div className="picker-alpha checker" onPointerDown={(e) => drag(e, (fx) => ({ ...hsvRef.current, a: Math.round(fx * 100) / 100 }))}>
            <div className="picker-alpha-fill" style={{ background: `linear-gradient(to right, transparent, ${opaque})` }} />
            <div className="picker-knob" style={{ left: `${hsv.a * 100}%`, background: formatColor(rgb) }} />
          </div>
        </div>
      </div>
      <div className="picker-inputs">
        <label className="field grow">
          <span className="field-label">#</span>
          <input
            className="mono"
            value={hex ?? toHex(rgb)}
            spellCheck={false}
            onFocus={(e) => e.target.select()}
            onChange={(e) => setHex(e.target.value)}
            onBlur={() => {
              if (hex !== null) {
                const c = parseColor(hex.startsWith('#') ? hex : `#${hex}`) ?? parseColor(hex);
                if (c) update(rgbToHsv({ ...c, a: c.a < 1 ? c.a : hsv.a }), true);
              }
              setHex(null);
            }}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
          />
        </label>
        <label className="field alpha">
          <input
            key={hsv.a}
            defaultValue={`${Math.round(hsv.a * 100)}`}
            onFocus={(e) => e.target.select()}
            onBlur={(e) => {
              const n = parseFloat(e.target.value);
              if (Number.isFinite(n)) update({ ...hsv, a: Math.max(0, Math.min(100, n)) / 100 }, true);
            }}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
          />
          <span className="field-suffix">%</span>
        </label>
      </div>
      {colorTokens.length > 0 && (
        <div className="picker-tokens">
          <div className="picker-tokens-title">Theme colors</div>
          <div className="picker-swatches">
            {colorTokens.map(([name, v]) => (
              <button
                key={name}
                type="button"
                title={name}
                className="swatch checker"
                onClick={() => {
                  lastEmitted.current = `var(${name})`;
                  props.onCommit(`var(${name})`);
                }}
              >
                <span style={{ background: v }} />
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/** Swatch + hex + opacity row; clicking the swatch opens the picker. */
export function ColorRow(props: {
  value: string;
  onPreview: (css: string) => void;
  onCommit: (css: string) => void;
  onPreviewEnd?: () => void;
  tokens?: Record<string, string>;
  actions?: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const tokenName = /^var\((--[^)]+)\)$/.exec(props.value.trim())?.[1];
  const resolved = tokenName ? props.tokens?.[tokenName] : props.value;
  const c = parseColor(resolved);
  const [hex, setHex] = useState<string | null>(null);
  const [alpha, setAlpha] = useState<string | null>(null);
  const commitHex = (text: string) => {
    const t = text.trim();
    const parsed = parseColor(/^[0-9a-f]{3,8}$/i.test(t) ? `#${t}` : t);
    if (parsed) props.onCommit(formatColor({ ...parsed, a: parsed.a < 1 ? parsed.a : c?.a ?? 1 }));
    else if (t) props.onCommit(t);
  };
  return (
    <div className="color-row">
      <div className="field grow color-field">
        <button ref={ref} type="button" className="swatch checker" onClick={() => setOpen(!open)}>
          <span style={{ background: resolved || 'transparent' }} />
        </button>
        {tokenName ? (
          <span className="token-chip" onClick={() => setOpen(true)}>
            {tokenName.slice(2)}
          </span>
        ) : (
          <input
            className="mono"
            value={hex ?? (c ? toHex(c) : props.value)}
            spellCheck={false}
            onFocus={(e) => e.target.select()}
            onChange={(e) => setHex(e.target.value)}
            onBlur={() => {
              if (hex !== null) commitHex(hex);
              setHex(null);
            }}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
          />
        )}
        {c && !tokenName && (
          <input
            className="alpha-input"
            value={alpha ?? `${Math.round(c.a * 100)}%`}
            onFocus={(e) => e.target.select()}
            onChange={(e) => setAlpha(e.target.value)}
            onBlur={() => {
              if (alpha !== null) {
                const n = parseFloat(alpha);
                if (Number.isFinite(n)) props.onCommit(formatColor({ ...c, a: Math.max(0, Math.min(100, n)) / 100 }));
              }
              setAlpha(null);
            }}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
          />
        )}
      </div>
      {props.actions}
      {open && (
        <Popover
          anchor={ref.current}
          placement="left"
          onClose={() => {
            setOpen(false);
            props.onPreviewEnd?.();
          }}
          width={240}
        >
          <ColorPicker value={resolved ?? '#000000'} tokens={props.tokens} onPreview={props.onPreview} onCommit={props.onCommit} />
        </Popover>
      )}
    </div>
  );
}
