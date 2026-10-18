import { useSyncExternalStore } from 'react';
import { batch, call, type Operation } from '../api';
import type { CanvasEngine, Tool, View } from './engine';

/** Editor UI state that is not part of the document (per window, not persisted to the file). */
export interface EditorState {
  tool: Tool;
  view: View;
  leftTab: 'design' | 'theme';
  panels: boolean;
  editingText: string | null;
  menu: { x: number; y: number; id: string | null } | null;
  toast: { id: number; text: string } | null;
  /** Bumped when the canvas lays out, so geometry-dependent UI can re-measure. */
  layoutTick: number;
  collapsed: Record<string, boolean>;
  insertHtmlOpen: boolean;
  iconsOpen: boolean;
  exportOpen: boolean;
  shortcutsOpen: boolean;
}

let state: EditorState = {
  tool: 'select',
  view: { x: 0, y: 0, zoom: 1 },
  leftTab: 'design',
  panels: true,
  editingText: null,
  menu: null,
  toast: null,
  layoutTick: 0,
  collapsed: {},
  insertHtmlOpen: false,
  iconsOpen: false,
  exportOpen: false,
  shortcutsOpen: false,
};
const listeners = new Set<() => void>();
export function setEditor(patch: Partial<EditorState> | ((s: EditorState) => Partial<EditorState>)) {
  state = { ...state, ...(typeof patch === 'function' ? patch(state) : patch) };
  for (const l of listeners) l();
}
export function editorState() {
  return state;
}
export function useEditor<T>(select: (s: EditorState) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => select(state),
  );
}

let toastId = 0;
export function toast(text: string) {
  const id = ++toastId;
  setEditor({ toast: { id, text } });
  setTimeout(() => {
    if (state.toast?.id === id) setEditor({ toast: null });
  }, 3200);
}

/** The engine of the currently visible file. */
export const engineRef: { current: CanvasEngine | null; fileId: string | null } = { current: null, fileId: null };

export function edit(ops: Operation[]) {
  if (!engineRef.fileId) return Promise.resolve(undefined);
  return batch(engineRef.fileId, ops);
}
export function setStyles(ids: string[], styles: Record<string, string>) {
  if (!ids.length) return Promise.resolve(undefined);
  return edit([{ tool: 'update_styles', args: { updates: [{ nodeIds: ids, styles }] } }]);
}
export function select(ids: string[]) {
  if (!engineRef.fileId) return;
  engineRef.current?.setSelection(ids);
  call('select_nodes', { fileId: engineRef.fileId, nodeIds: ids });
}
export function preview(ids: string[], styles: Record<string, string>) {
  engineRef.current?.preview(ids, styles);
}
