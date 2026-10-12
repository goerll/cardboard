import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, ChevronDown } from 'lucide-react';

export const ICON = 14;

/** Numeric field that commits on Enter/blur, nudges with arrows, and scrubs when dragging its label. */
export function NumberField(props: {
  label?: ReactNode;
  value: string;
  placeholder?: string;
  title?: string;
  suffix?: string;
  step?: number;
  min?: number;
  max?: number;
  /** Converts a typed number into the CSS value to store (default `${n}px`). */
  format?: (n: number) => string;
  /** Called with the raw text for non-numeric input (e.g. "auto", "var(--x)"). */
  onCommit: (value: string) => void;
  onPreview?: (value: string) => void;
  onPreviewEnd?: () => void;
  className?: string;
  disabled?: boolean;
}) {
  const { value, step = 1, min = -Infinity, max = Infinity } = props;
  const format = props.format ?? ((n: number) => `${n}px`);
  const [draft, setDraft] = useState<string | null>(null);
  const display = draft ?? value;
  const numeric = parseFloat(value);
  const clamp = (n: number) => Math.min(max, Math.max(min, Math.round(n * 100) / 100));
  const commit = (raw: string) => {
    setDraft(null);
    const t = raw.trim();
    if (t === value) return;
    if (t === '') return props.onCommit('');
    if (/^-?\d*\.?\d+$/.test(t)) return props.onCommit(format(clamp(parseFloat(t))));
    // Simple arithmetic like "16*2" or "100/3".
    if (/^[\d\s.+\-*/()]+$/.test(t)) {
      try {
        const n = Function(`"use strict";return (${t})`)() as number;
        if (Number.isFinite(n)) return props.onCommit(format(clamp(n)));
      } catch {
        /* fall through to raw */
      }
    }
    props.onCommit(t);
  };
  const scrub = (e: React.PointerEvent) => {
    if (props.disabled || e.button !== 0) return;
    const start = Number.isFinite(numeric) ? numeric : 0;
    const sx = e.clientX;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    let last = value;
    let moved = false;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - sx;
      if (!moved && Math.abs(dx) < 2) return;
      moved = true;
      document.body.classList.add('scrubbing');
      const mult = ev.shiftKey ? 10 : ev.altKey ? 0.1 : 1;
      last = format(clamp(start + Math.round(dx / 2) * step * mult));
      setDraft(String(parseFloat(last)));
      props.onPreview?.(last);
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
      target.removeEventListener('pointercancel', up);
      document.body.classList.remove('scrubbing');
      setDraft(null);
      if (moved) {
        props.onPreviewEnd?.();
        if (last !== value) props.onCommit(last);
      }
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
    target.addEventListener('pointercancel', up);
  };
  return (
    <label className={`field ${props.className ?? ''} ${props.disabled ? 'disabled' : ''}`} title={props.title}>
      {props.label != null && (
        <span className="field-label scrub" onPointerDown={scrub}>
          {props.label}
        </span>
      )}
      <input
        value={display}
        placeholder={props.placeholder}
        disabled={props.disabled}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => e.target.select()}
        onBlur={(e) => draft !== null && commit(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            commit((e.target as HTMLInputElement).value);
            (e.target as HTMLInputElement).blur();
          } else if (e.key === 'Escape') {
            setDraft(null);
            (e.target as HTMLInputElement).blur();
          } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            const base = parseFloat(draft ?? value);
            const n = (Number.isFinite(base) ? base : 0) + (e.key === 'ArrowUp' ? 1 : -1) * step * (e.shiftKey ? 10 : 1);
            setDraft(null);
            props.onCommit(format(clamp(n)));
          }
        }}
      />
      {props.suffix && <span className="field-suffix">{props.suffix}</span>}
    </label>
  );
}

export function TextField(props: {
  value: string;
  onCommit: (v: string) => void;
  placeholder?: string;
  label?: ReactNode;
  className?: string;
  mono?: boolean;
  autoFocus?: boolean;
  onCancel?: () => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return (
    <label className={`field ${props.className ?? ''}`}>
      {props.label != null && <span className="field-label">{props.label}</span>}
      <input
        className={props.mono ? 'mono' : ''}
        value={draft ?? props.value}
        placeholder={props.placeholder}
        spellCheck={false}
        autoFocus={props.autoFocus}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => e.target.select()}
        onBlur={() => {
          if (draft !== null && draft !== props.value) props.onCommit(draft);
          setDraft(null);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
          if (e.key === 'Escape') {
            setDraft(null);
            props.onCancel?.();
            requestAnimationFrame(() => (e.target as HTMLInputElement).blur());
          }
        }}
      />
    </label>
  );
}

/** Popover anchored to an element, rendered above everything. Outside clicks close it and are swallowed by the canvas. */
export function Popover(props: {
  anchor: HTMLElement | null;
  onClose: () => void;
  children: ReactNode;
  placement?: 'left' | 'below';
  className?: string;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const onClose = useRef(props.onClose);
  onClose.current = props.onClose;
  useLayoutEffect(() => {
    if (!props.anchor || !ref.current) return;
    const a = props.anchor.getBoundingClientRect();
    const p = ref.current.getBoundingClientRect();
    let x: number;
    let y: number;
    if (props.placement === 'left') {
      x = a.left - p.width - 8;
      y = a.top - 8;
    } else {
      x = a.left;
      y = a.bottom + 4;
    }
    x = Math.max(8, Math.min(x, innerWidth - p.width - 8));
    y = Math.max(8, Math.min(y, innerHeight - p.height - 8));
    setPos({ x, y });
  }, [props.anchor, props.placement]);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as HTMLElement;
      if (ref.current?.contains(t) || props.anchor?.contains(t)) return;
      // Popovers stacked above this one (e.g. a dropdown inside the color picker) manage themselves.
      if (t.closest('.popover')) return;
      if (t.closest('.canvas-host')) {
        e.stopPropagation();
        e.preventDefault();
      }
      onClose.current();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose.current();
      }
    };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('keydown', key, true);
    return () => {
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('keydown', key, true);
    };
  }, [props.anchor]);
  return createPortal(
    <div
      ref={ref}
      className={`popover ${props.className ?? ''}`}
      style={{ left: pos?.x ?? -9999, top: pos?.y ?? -9999, width: props.width }}
      onPointerDown={(e) => e.stopPropagation()}
    >
      {props.children}
    </div>,
    document.body,
  );
}

export interface Option<T extends string> {
  value: T;
  label: ReactNode;
  icon?: ReactNode;
}
export function Select<T extends string>(props: {
  value: T | '';
  options: Option<T>[];
  onChange: (v: T) => void;
  icon?: ReactNode;
  className?: string;
  placeholder?: string;
  width?: number;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const current = props.options.find((o) => o.value === props.value);
  return (
    <>
      <button ref={ref} type="button" className={`select ${props.className ?? ''} ${open ? 'open' : ''}`} onClick={() => setOpen(!open)}>
        {props.icon && <span className="select-icon">{props.icon}</span>}
        <span className="select-value">{current?.label ?? props.placeholder ?? props.value}</span>
        <ChevronDown size={12} className="select-chevron" />
      </button>
      {open && (
        <Popover anchor={ref.current} onClose={() => setOpen(false)} className="menu" width={props.width ?? Math.max(140, ref.current?.offsetWidth ?? 0)}>
          {props.options.map((o) => (
            <button
              key={o.value}
              type="button"
              className={`menu-item ${o.value === props.value ? 'active' : ''}`}
              onClick={() => {
                setOpen(false);
                props.onChange(o.value);
              }}
            >
              <span className="menu-check">{o.value === props.value && <Check size={12} />}</span>
              {o.icon}
              <span>{o.label}</span>
            </button>
          ))}
        </Popover>
      )}
    </>
  );
}

export function IconButton(props: {
  icon: ReactNode;
  title?: string;
  onClick?: (e: React.MouseEvent<HTMLButtonElement>) => void;
  active?: boolean;
  className?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className={`icon-btn ${props.active ? 'active' : ''} ${props.className ?? ''}`}
      title={props.title}
      onClick={props.onClick}
      disabled={props.disabled}
    >
      {props.icon}
    </button>
  );
}

export function Segmented<T extends string>(props: { value: T; options: { value: T; label: ReactNode; title?: string }[]; onChange: (v: T) => void; className?: string }) {
  return (
    <div className={`segmented ${props.className ?? ''}`}>
      {props.options.map((o) => (
        <button key={o.value} type="button" title={o.title} className={o.value === props.value ? 'active' : ''} onClick={() => props.onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Section(props: { title: ReactNode; actions?: ReactNode; children?: ReactNode; empty?: boolean; onAdd?: () => void; className?: string }) {
  return (
    <section className={`section ${props.empty ? 'empty' : ''} ${props.className ?? ''}`}>
      <header className="section-head" onClick={props.empty ? props.onAdd : undefined}>
        <span className="section-title">{props.title}</span>
        <span className="section-actions" onClick={(e) => e.stopPropagation()}>
          {props.actions}
        </span>
      </header>
      {props.children && <div className="section-body">{props.children}</div>}
    </section>
  );
}

export function Checkbox(props: { checked: boolean; onChange: (v: boolean) => void; label: ReactNode; hint?: string }) {
  return (
    <label className="checkbox">
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      <span className="checkbox-box">{props.checked && <Check size={10} strokeWidth={3} />}</span>
      <span>{props.label}</span>
      {props.hint && <span className="hint">{props.hint}</span>}
    </label>
  );
}

export function Slider(props: { value: number; min: number; max: number; onPreview: (v: number) => void; onCommit: (v: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const frac = Math.max(0, Math.min(1, (props.value - props.min) / (props.max - props.min || 1)));
  const at = (x: number) => {
    const r = ref.current!.getBoundingClientRect();
    return Math.round(props.min + Math.max(0, Math.min(1, (x - r.left) / r.width)) * (props.max - props.min));
  };
  return (
    <div
      ref={ref}
      className="slider"
      onPointerDown={(e) => {
        const el = e.currentTarget;
        el.setPointerCapture(e.pointerId);
        let v = at(e.clientX);
        props.onPreview(v);
        const move = (ev: PointerEvent) => {
          v = at(ev.clientX);
          props.onPreview(v);
        };
        const up = () => {
          el.removeEventListener('pointermove', move);
          el.removeEventListener('pointerup', up);
          props.onCommit(v);
        };
        el.addEventListener('pointermove', move);
        el.addEventListener('pointerup', up);
      }}
    >
      <div className="slider-track" />
      <div className="slider-thumb" style={{ left: `calc(${frac * 100}% - ${frac * 16}px)` }} />
    </div>
  );
}
