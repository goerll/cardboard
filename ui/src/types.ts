// Mirrors src/document.rs (serde camelCase). Style keys are kebab-case CSS property names.
export type Styles = Record<string, string>;

export interface DesignNode {
  id: string;
  name: string;
  tag: string;
  text: string;
  attributes: Record<string, string>;
  styles: Styles;
  children: DesignNode[];
  locked: boolean;
  visible: boolean;
  /** Canvas position, top-level artboards only. */
  position?: [number, number];
  /** Sanitized inner SVG of an icon layer (an `<svg>` with no child nodes). */
  markup?: string;
}

export interface Page {
  id: string;
  name: string;
  artboards: DesignNode[];
  background?: string;
}

/** One @font-face whose file lives in the asset store (src is `/asset/<sha256>.<ext>`). */
export interface FontFace {
  family: string;
  src: string;
  weight: string;
  style: string;
  unicodeRange?: string;
  source?: string;
}

export interface CardboardDocument {
  id: string;
  name: string;
  revision: number;
  nextId: number;
  activePage: string;
  pages: Page[];
  tokens: Record<string, string>;
  selection: string[];
  fonts?: FontFace[];
}

export interface Tab {
  fileId: string;
  name: string;
  path: string;
}

export interface LibraryFile {
  path: string;
  name: string;
  fileId: string | null;
  open: boolean;
  archived: boolean;
  modified: number;
  opened: number | null;
  tokens: Record<string, string>;
  fonts?: FontFace[];
  background?: string | null;
  artboards: { id: string; name: string; x: number; y: number; w: number; h: number; html: string }[];
}

export interface OpenDoc {
  doc: CardboardDocument;
  canUndo: boolean;
  canRedo: boolean;
  /** Layers agents are working on: node id → agent name. */
  agents: Record<string, string>;
}
