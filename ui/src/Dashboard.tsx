import { memo, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { Archive, Clock, Copy, FolderOpen, LayoutGrid, List, MoreHorizontal, Plus, RotateCcw, Search, Settings, Trash2, Pencil } from 'lucide-react';
import { call, request, useServer } from './api';
import type { LibraryFile } from './types';
import { fontFaceCss } from './lib/css';
import { tokensCss } from './lib/tokens';
import { Popover, Segmented } from './ui/controls';
import { getSettings, setSettings, useSettings } from './settings';

type View = 'recents' | 'files' | 'archive' | 'settings';

export function Dashboard() {
  const [view, setView] = useState<View>('recents');
  const [query, setQuery] = useState('');
  const [layout, setLayout] = useState<'grid' | 'list'>(() => getSettings().dashboardLayout);
  const libraryVersion = useServer((s) => s.libraryVersion);
  const connected = useServer((s) => s.connected);
  const library = useServer((s) => s.library);
  const [files, setFiles] = useState<LibraryFile[] | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!connected || view === 'settings') return;
    let live = true;
    request<{ files: LibraryFile[] }>('list_files', { archived: view === 'archive', previews: true })
      .then((r) => live && setFiles(r.files))
      .catch(() => live && setFiles([]));
    return () => {
      live = false;
    };
  }, [view, libraryVersion, connected]);
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, []);
  const shown = useMemo(() => {
    let list = files ?? [];
    if (view === 'files') list = [...list].sort((a, b) => a.name.localeCompare(b.name));
    const q = query.trim().toLowerCase();
    return q ? list.filter((f) => f.name.toLowerCase().includes(q)) : list;
  }, [files, query, view]);
  const nav = (v: View, icon: React.ReactNode, label: string) => (
    <button type="button" className={`nav-item ${view === v ? 'active' : ''}`} onClick={() => setView(v)}>
      {icon}
      <span>{label}</span>
    </button>
  );
  const title = { recents: 'Recents', files: 'All files', archive: 'Archive', settings: 'Settings' }[view];
  return (
    <div className="dashboard">
      <aside className="dash-side">
        <div className="dash-brand">
          <img className="logo" src="/logo.svg" alt="" />
          <span>Cardboard</span>
        </div>
        <label className="search">
          <Search size={14} />
          <input ref={searchRef} placeholder="Search" value={query} onChange={(e) => setQuery(e.target.value)} />
          <kbd>Ctrl F</kbd>
        </label>
        <nav>
          {nav('recents', <Clock size={15} />, 'Recents')}
          {nav('files', <LayoutGrid size={15} />, 'Files')}
          {nav('archive', <Archive size={15} />, 'Archive')}
          {nav('settings', <Settings size={15} />, 'Settings')}
        </nav>
        <div className="spacer" />
        <div className="dash-card">
          <div className="dash-card-title">Using agents</div>
          <p>Agents can read and edit your open files live through Cardboard's MCP server.</p>
          <button
            type="button"
            className="btn wide"
            onClick={() => navigator.clipboard.writeText(JSON.stringify({ mcpServers: { cardboard: { command: 'cardboard', args: ['mcp'] } } }, null, 2))}
          >
            Copy MCP config
          </button>
        </div>
        <div className="dash-foot" title={library}>
          <FolderOpen size={12} /> <span>{library ? library.replace(/^\/home\/[^/]+/, '~') : 'Local library'}</span>
        </div>
      </aside>
      <main className="dash-main">
        <header className="dash-header">
          <h1>{title}</h1>
          <span className="spacer" />
          {view !== 'settings' && (
            <>
              <button type="button" className="btn primary" onClick={() => call('create_file', {})}>
                <Plus size={14} /> New file
              </button>
              <Segmented
                className="layout-toggle"
                value={layout}
                options={[
                  { value: 'grid', label: <LayoutGrid size={14} />, title: 'Grid' },
                  { value: 'list', label: <List size={14} />, title: 'List' },
                ]}
                onChange={(v) => {
                  setLayout(v);
                  setSettings({ dashboardLayout: v });
                }}
              />
            </>
          )}
        </header>
        {view === 'settings' ? (
          <SettingsView />
        ) : files === null ? null : shown.length === 0 ? (
          <div className="dash-empty">
            {query ? (
              <p>No files match “{query}”.</p>
            ) : view === 'archive' ? (
              <p>Archived files show up here.</p>
            ) : (
              <>
                <div className="dash-empty-art" />
                <h2>Start your first design</h2>
                <p>Files are saved as .cardboard files on this computer.</p>
                <button type="button" className="btn primary" onClick={() => call('create_file', {})}>
                  <Plus size={14} /> New file
                </button>
              </>
            )}
          </div>
        ) : layout === 'grid' ? (
          <div className="file-grid">
            {shown.map((f) => (
              <FileCard key={f.path} file={f} archived={view === 'archive'} />
            ))}
          </div>
        ) : (
          <div className="file-list">
            <div className="file-list-head">
              <span>Name</span>
              <span>Edited</span>
              <span />
            </div>
            {shown.map((f) => (
              <FileListRow key={f.path} file={f} archived={view === 'archive'} />
            ))}
          </div>
        )}
      </main>
    </div>
  );
}

// Which file is being renamed inline, shared by grid cards and list rows.
let renamingPath: string | null = null;
const renameListeners = new Set<() => void>();
function startRename(path: string | null) {
  renamingPath = path;
  for (const l of renameListeners) l();
}
function useRenaming() {
  return useSyncExternalStore(
    (l) => (renameListeners.add(l), () => renameListeners.delete(l)),
    () => renamingPath,
  );
}

function RenameInput({ file }: { file: LibraryFile }) {
  return (
    <input
      className="rename-input"
      autoFocus
      defaultValue={file.name}
      onClick={(e) => e.stopPropagation()}
      onFocus={(e) => e.target.select()}
      onBlur={(e) => {
        const name = e.target.value.trim();
        startRename(null);
        if (name && name !== file.name) call('rename_file', { path: file.path, name });
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        if (e.key === 'Escape') {
          (e.target as HTMLInputElement).value = file.name;
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

function open(f: LibraryFile, archived: boolean) {
  if (archived) return;
  call('open_file', { path: f.path });
}

const FileCard = memo(function FileCard({ file, archived }: { file: LibraryFile; archived: boolean }) {
  const renaming = useRenaming() === file.path;
  return (
    <div className="file-card" onClick={() => !renaming && open(file, archived)}>
      <div className="file-card-head">
        <div>
          {renaming ? <RenameInput file={file} /> : <div className="file-name">{file.name}</div>}
          <div className="file-meta">Edited {ago(file.modified)}</div>
        </div>
        <FileMenu file={file} archived={archived} />
      </div>
      <Thumbnail file={file} />
    </div>
  );
});

function FileListRow({ file, archived }: { file: LibraryFile; archived: boolean }) {
  const renaming = useRenaming() === file.path;
  return (
    <div className="file-row" onClick={() => !renaming && open(file, archived)}>
      {renaming ? <RenameInput file={file} /> : <span className="file-name">{file.name}</span>}
      <span className="file-meta">{ago(file.modified)}</span>
      <FileMenu file={file} archived={archived} />
    </div>
  );
}

function FileMenu({ file, archived }: { file: LibraryFile; archived: boolean }) {
  const [openMenu, setOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);
  const item = (icon: React.ReactNode, label: string, fn: () => void, danger = false) => (
    <button
      type="button"
      className={`menu-item ${danger ? 'danger' : ''}`}
      onClick={(e) => {
        e.stopPropagation();
        setOpen(false);
        fn();
      }}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
  return (
    <>
      <button
        ref={ref}
        type="button"
        className="icon-btn file-more"
        onClick={(e) => {
          e.stopPropagation();
          setOpen(!openMenu);
        }}
      >
        <MoreHorizontal size={14} />
      </button>
      {openMenu && (
        <Popover anchor={ref.current} onClose={() => setOpen(false)} className="menu" width={180}>
          {archived ? (
            <>
              {item(<RotateCcw size={13} />, 'Restore', () => call('restore_file', { path: file.path }))}
              {confirmDelete ? (
                item(<Trash2 size={13} />, 'Click again to delete', () => call('delete_file', { path: file.path }), true)
              ) : (
                <button type="button" className="menu-item danger" onClick={(e) => (e.stopPropagation(), setConfirmDelete(true))}>
                  <Trash2 size={13} />
                  <span>Delete forever…</span>
                </button>
              )}
            </>
          ) : (
            <>
              {item(<FolderOpen size={13} />, 'Open', () => open(file, false))}
              {item(<Pencil size={13} />, 'Rename', () => startRename(file.path))}
              {item(<Copy size={13} />, 'Duplicate', () => call('duplicate_file', { path: file.path }))}
              {item(<Archive size={13} />, 'Archive', () => call('archive_file', { path: file.path }), true)}
            </>
          )}
        </Popover>
      )}
    </>
  );
}

/** A live, scaled-down render of the first page. Uses a script-less iframe, created lazily. */
function Thumbnail({ file }: { file: LibraryFile }) {
  const ref = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const io = new IntersectionObserver(([e]) => e.isIntersecting && setVisible(true), { rootMargin: '200px' });
    io.observe(ref.current!);
    return () => io.disconnect();
  }, []);
  const srcdoc = useMemo(() => {
    const abs = file.artboards.filter((a) => a.html);
    if (!abs.length) return null;
    const minX = Math.min(...abs.map((a) => a.x));
    const minY = Math.min(...abs.map((a) => a.y));
    const maxX = Math.max(...abs.map((a) => a.x + (a.w || 400)));
    const maxY = Math.max(...abs.map((a) => a.y + (a.h || 300)));
    const W = maxX - minX;
    const H = maxY - minY;
    const tokens = tokensCss(file.tokens, file.modes, file.defaultMode);
    const body = abs.map((a) => `<div style="position:absolute;left:${a.x - minX}px;top:${a.y - minY}px">${a.html}</div>`).join('');
    // The iframe is a fixed 520×320 box scaled down 2x by CSS (see .thumb iframe), so the scale is known.
    const scale = Math.min((520 - 48) / W, (320 - 48) / H) || 1;
    return `<!doctype html><html><head><meta charset="utf-8"><style>${fontFaceCss(file.fonts)}${tokens}*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:${file.background || '#282828'}}#s{position:absolute;left:50%;top:50%;width:${W}px;height:${H}px;transform:translate(-50%,-50%) scale(${scale});transform-origin:center}</style></head><body><div id="s">${body}</div></body></html>`;
  }, [file]);
  return (
    <div className="thumb" ref={ref} style={{ background: file.background || '#282828' }}>
      {visible && srcdoc && <iframe title={file.name} sandbox="" srcDoc={srcdoc} tabIndex={-1} loading="lazy" />}
    </div>
  );
}

function ago(ms: number) {
  if (!ms) return 'just now';
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} hours ago`.replace(/^1 hours/, '1 hour');
  const d = Math.floor(s / 86400);
  if (d < 60) return d === 1 ? '1 day ago' : `${d} days ago`;
  return new Date(ms).toLocaleDateString();
}

function SettingsView() {
  const settings = useSettings();
  const library = useServer((s) => s.library);
  return (
    <div className="settings">
      <div className="setting">
        <div>
          <div className="setting-title">Appearance</div>
          <div className="muted small">Interface theme. Your designs are unaffected.</div>
        </div>
        <Segmented
          value={settings.theme}
          options={[
            { value: 'system', label: 'System' },
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
          ]}
          onChange={(theme) => setSettings({ theme })}
        />
      </div>
      <div className="setting">
        <div>
          <div className="setting-title">Library folder</div>
          <div className="muted small">Every file is a .cardboard file in this folder. Set CARDBOARD_LIBRARY or pass --library to change it.</div>
        </div>
        <code className="path">{library}</code>
      </div>
      <div className="setting">
        <div>
          <div className="setting-title">Agents (MCP)</div>
          <div className="muted small">Run <code>cardboard mcp</code> from your agent. It connects to this window while Cardboard is open.</div>
        </div>
      </div>
    </div>
  );
}
