import { useSyncExternalStore } from 'react';
import type { OpenDoc, Tab } from './types';

/** The desktop app's updater (absent in `cardboard serve`). */
export interface UpdateStatus {
  state: 'idle' | 'checking' | 'downloading' | 'ready' | 'disabled' | 'unmanaged';
  current: string;
  /** Version downloaded and verified, when `state` is `ready`. */
  version?: string;
  notesUrl?: string;
  managed?: boolean;
  autoUpdate?: boolean;
  /** Why updates are off (environment variable, not an installer-managed copy). */
  reason?: string;
  lastCheck?: number;
  error?: string;
}

/** Everything the server pushes. Documents are replaced wholesale on every push. */
export interface ServerState {
  connected: boolean;
  tabs: Tab[];
  activeFile: string | null;
  library: string;
  docs: Record<string, OpenDoc>;
  /** Bumped whenever the server reports library changes; the dashboard refetches on change. */
  libraryVersion: number;
  update: UpdateStatus | null;
}

let state: ServerState = { connected: false, tabs: [], activeFile: null, library: '', docs: {}, libraryVersion: 0, update: null };
const listeners = new Set<() => void>();
function set(patch: Partial<ServerState>) {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}
export function getState() {
  return state;
}
export function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
export function useServer<T>(select: (s: ServerState) => T): T {
  return useSyncExternalStore(subscribe, () => select(state));
}

type Pending = { resolve: (v: any) => void; reject: (e: Error) => void };
const pending = new Map<number, Pending>();
let socket: WebSocket | null = null;
let nextId = 1;
let queue: string[] = [];
let errorHandler: (message: string) => void = (m) => console.error(m);
export function onError(handler: (message: string) => void) {
  errorHandler = handler;
}

function url() {
  const params = new URLSearchParams(location.search);
  const token = params.get('t');
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${proto}://${location.host}/ws${token ? `?t=${token}` : ''}`;
}

export function connect() {
  const ws = new WebSocket(url());
  socket = ws;
  ws.onopen = () => {
    set({ connected: true });
    for (const m of queue) ws.send(m);
    queue = [];
  };
  ws.onclose = () => {
    set({ connected: false });
    for (const p of pending.values()) p.reject(new Error('Connection lost'));
    pending.clear();
    setTimeout(connect, 600);
  };
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    switch (msg.type) {
      case 'workspace':
        set({ tabs: msg.tabs, activeFile: msg.activeFile, library: msg.library ?? state.library });
        break;
      case 'document':
        set({ docs: { ...state.docs, [msg.fileId]: { doc: msg.doc, canUndo: !!msg.canUndo, canRedo: !!msg.canRedo, agents: msg.agents ?? {} } } });
        break;
      case 'closed': {
        const docs = { ...state.docs };
        delete docs[msg.fileId];
        set({ docs });
        break;
      }
      case 'update':
        set({ update: msg.update });
        break;
      case 'library':
        set({ libraryVersion: state.libraryVersion + 1 });
        break;
      case 'result': {
        const p = pending.get(msg.id);
        pending.delete(msg.id);
        if (!p) break;
        if (msg.ok) p.resolve(msg.result);
        else p.reject(new Error(msg.error));
        break;
      }
    }
  };
}

/** Raw request; rejects on tool errors. */
export function request<T = any>(tool: string, args: Record<string, unknown> = {}): Promise<T> {
  const id = nextId++;
  const message = JSON.stringify({ id, tool, args });
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    if (socket?.readyState === WebSocket.OPEN) socket.send(message);
    else queue.push(message);
  });
}

/** Fire-and-report: tool errors surface as a toast instead of throwing. */
export async function call<T = any>(tool: string, args: Record<string, unknown> = {}): Promise<T | undefined> {
  try {
    return await request<T>(tool, args);
  } catch (e) {
    errorHandler((e as Error).message);
    return undefined;
  }
}

export interface Operation {
  tool: string;
  args: Record<string, unknown>;
}
/** Several edits as one undo step. */
export function batch(fileId: string, operations: Operation[]) {
  if (operations.length === 1) return call(operations[0].tool, { fileId, ...operations[0].args });
  return call('batch', { fileId, operations });
}
