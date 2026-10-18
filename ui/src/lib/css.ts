import type { FontFace } from '../types';

/** @font-face rules for document fonts; mirrors fonts::css in src/fonts.rs. */
export function fontFaceCss(fonts: FontFace[] | undefined): string {
  const formats: Record<string, string> = { woff2: 'woff2', woff: 'woff', otf: 'opentype' };
  return (fonts ?? [])
    .map((f) => {
      const format = formats[f.src.split('.').pop() ?? ''] ?? 'truetype';
      const range = f.unicodeRange ? `;unicode-range:${f.unicodeRange}` : '';
      return `@font-face{font-family:"${f.family}";src:url("${f.src}") format("${format}");font-weight:${f.weight};font-style:${f.style};font-display:swap${range}}`;
    })
    .join('');
}

export interface RGBA {
  r: number;
  g: number;
  b: number;
  a: number;
}
export interface HSVA {
  h: number;
  s: number;
  v: number;
  a: number;
}

let probe: CanvasRenderingContext2D | null = null;
/** Parses any CSS color the browser understands. Returns null for var(), gradients, etc. */
export function parseColor(value: string | undefined): RGBA | null {
  if (!value) return null;
  const v = value.trim();
  if (!v || v.includes('var(') || v.includes('gradient') || v.includes('url(')) return null;
  const hex = /^#([0-9a-f]{3,8})$/i.exec(v);
  if (hex) {
    let h = hex[1];
    if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('');
    if (h.length !== 6 && h.length !== 8) return null;
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    };
  }
  if (v === 'transparent') return { r: 0, g: 0, b: 0, a: 0 };
  if (!CSS.supports('color', v)) return null;
  probe ??= document.createElement('canvas').getContext('2d');
  if (!probe) return null;
  probe.fillStyle = '#000';
  probe.fillStyle = v;
  const out = String(probe.fillStyle);
  if (out.startsWith('#')) return parseColor(out);
  const m = /rgba?\(([^)]+)\)/.exec(out);
  if (!m) return null;
  const [r, g, b, a = '1'] = m[1].split(/[\s,/]+/).filter(Boolean);
  return { r: +r, g: +g, b: +b, a: +a };
}

const h2 = (n: number) => Math.round(Math.max(0, Math.min(255, n))).toString(16).padStart(2, '0');
export function toHex({ r, g, b }: RGBA) {
  return (h2(r) + h2(g) + h2(b)).toUpperCase();
}
export function formatColor(c: RGBA) {
  if (c.a >= 0.999) return `#${toHex(c).toLowerCase()}`;
  return `rgba(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)}, ${+c.a.toFixed(3)})`;
}

export function rgbToHsv({ r, g, b, a }: RGBA): HSVA {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max ? d / max : 0, v: max, a };
}
export function hsvToRgb({ h, s, v, a }: HSVA): RGBA {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return { r: f(5) * 255, g: f(3) * 255, b: f(1) * 255, a };
}

/** "12px" → 12, "1.5" → 1.5, else null. */
export function num(value: string | undefined): number | null {
  if (value == null) return null;
  const m = /^(-?\d*\.?\d+)(px|deg)?$/.exec(value.trim());
  return m ? parseFloat(m[1]) : null;
}
export function round(n: number, places = 2) {
  const f = 10 ** places;
  return Math.round(n * f) / f;
}
/** Bare numbers become px; anything else passes through untouched. */
export function length(input: string): string {
  const t = input.trim();
  if (/^-?\d*\.?\d+$/.test(t)) return `${t}px`;
  return t;
}

/** Split on top-level commas (ignores commas inside parentheses). */
export function splitTop(value: string, sep = ','): string[] {
  const out: string[] = [];
  let depth = 0;
  let cur = '';
  for (const ch of value) {
    if (ch === '(') depth++;
    if (ch === ')') depth--;
    if (ch === sep && depth === 0) {
      out.push(cur.trim());
      cur = '';
    } else cur += ch;
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}
export function splitSpaces(value: string) {
  return splitTop(value.replace(/\s+/g, ' '), ' ');
}

export interface Shadow {
  inset: boolean;
  x: number;
  y: number;
  blur: number;
  spread: number;
  color: string;
}
export function parseShadows(value: string | undefined): Shadow[] {
  if (!value || value === 'none') return [];
  return splitTop(value).map((part) => {
    const tokens = splitSpaces(part);
    const inset = tokens.includes('inset');
    const lengths: number[] = [];
    let color = 'rgba(0, 0, 0, 0.25)';
    for (const t of tokens) {
      if (t === 'inset') continue;
      const n = num(t);
      if (n !== null || t === '0') lengths.push(n ?? 0);
      else color = t;
    }
    const [x = 0, y = 0, blur = 0, spread = 0] = lengths;
    return { inset, x, y, blur, spread, color };
  });
}
export function formatShadows(shadows: Shadow[]) {
  return shadows.map((s) => `${s.inset ? 'inset ' : ''}${s.x}px ${s.y}px ${s.blur}px ${s.spread}px ${s.color}`).join(', ');
}

export interface GradientStop {
  color: string;
  pos: number;
}
export interface Gradient {
  kind: 'linear' | 'radial';
  angle: number;
  stops: GradientStop[];
}
export function parseGradient(value: string | undefined): Gradient | null {
  if (!value) return null;
  const m = /^(linear|radial)-gradient\((.*)\)$/s.exec(value.trim());
  if (!m) return null;
  const parts = splitTop(m[2]);
  let angle = 180;
  if (parts.length && /^(-?\d*\.?\d+)deg$/.test(parts[0])) angle = parseFloat(parts.shift()!);
  else if (parts.length && /^to |^circle|^ellipse|^closest|^farthest|^at /.test(parts[0])) {
    const dir = parts.shift()!;
    angle = ({ 'to top': 0, 'to right': 90, 'to bottom': 180, 'to left': 270 } as Record<string, number>)[dir] ?? 180;
  }
  const stops = parts.map((p, i) => {
    const tokens = splitSpaces(p);
    const posToken = tokens.length > 1 ? tokens[tokens.length - 1] : '';
    const pos = posToken.endsWith('%') ? parseFloat(posToken) : (i / Math.max(1, parts.length - 1)) * 100;
    return { color: posToken.endsWith('%') ? tokens.slice(0, -1).join(' ') : p, pos };
  });
  return { kind: m[1] as 'linear' | 'radial', angle, stops };
}
export function formatGradient(g: Gradient) {
  const stops = g.stops.map((s) => `${s.color} ${round(s.pos, 1)}%`).join(', ');
  return g.kind === 'linear' ? `linear-gradient(${round(g.angle, 1)}deg, ${stops})` : `radial-gradient(circle, ${stops})`;
}

/** Expand a 1–4 value box shorthand into [top, right, bottom, left]. */
export function boxSides(value: string | undefined): [string, string, string, string] {
  const p = value ? splitSpaces(value) : [];
  const [t = '0', r = t, b = t, l = r] = p;
  return [t, r, b, l];
}
