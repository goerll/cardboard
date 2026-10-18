import { useEffect } from 'react';
import { ArrowUpCircle, FileText, LayoutGrid, Plus, X } from 'lucide-react';
import { call, onError, useServer } from './api';
import { Dashboard } from './Dashboard';
import { Editor } from './editor/Editor';
import { toast, useEditor } from './editor/store';

onError((m) => toast(m));

export function App() {
  const connected = useServer((s) => s.connected);
  const tabs = useServer((s) => s.tabs);
  const active = useServer((s) => s.activeFile);
  const open = useServer((s) => (s.activeFile ? s.docs[s.activeFile] : undefined));
  useEffect(() => {
    document.title = open ? `${open.doc.name} – Cardboard` : 'Cardboard';
  }, [open?.doc.name]);
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (!mod) return;
      const key = e.key.toLowerCase();
      if (key === 'w' && active) {
        e.preventDefault();
        call('close_file', { fileId: active });
      } else if (key === 'n' || key === 't') {
        e.preventDefault();
        call('create_file', {});
      } else if (key === 'tab' && tabs.length) {
        e.preventDefault();
        const ids = [null, ...tabs.map((t) => t.fileId)];
        const i = ids.indexOf(active);
        const next = ids[(i + (e.shiftKey ? -1 : 1) + ids.length) % ids.length];
        if (next) call('open_file', { fileId: next });
        else call('show_dashboard');
      }
    };
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, [active, tabs]);
  return (
    <div className="app">
      <div className="tabbar">
        <button type="button" className={`tab home ${active ? '' : 'active'}`} onClick={() => call('show_dashboard')}>
          <LayoutGrid size={14} />
          <span>Dashboard</span>
        </button>
        {tabs.map((t) => (
          <div
            key={t.fileId}
            className={`tab ${t.fileId === active ? 'active' : ''}`}
            onClick={() => t.fileId !== active && call('open_file', { fileId: t.fileId })}
            onAuxClick={(e) => e.button === 1 && call('close_file', { fileId: t.fileId })}
            title={t.path}
          >
            <FileText size={14} />
            <span className="tab-name">{t.name}</span>
            <button
              type="button"
              className="tab-close"
              onClick={(e) => {
                e.stopPropagation();
                call('close_file', { fileId: t.fileId });
              }}
            >
              <X size={12} />
            </button>
          </div>
        ))}
        <button type="button" className="tab-new" title="New file (Ctrl N)" onClick={() => call('create_file', {})}>
          <Plus size={14} />
        </button>
        <div className="tabbar-drag" />
        <UpdatePill />
      </div>
      <div className="content">
        {active && open ? <Editor key={active} fileId={active} doc={open.doc} canUndo={open.canUndo} canRedo={open.canRedo} /> : <Dashboard />}
      </div>
      {!connected && <div className="offline">Reconnecting to Cardboard…</div>}
      <Toast />
    </div>
  );
}

function Toast() {
  const t = useEditor((s) => s.toast);
  if (!t) return null;
  return (
    <div className="toast" key={t.id}>
      {t.text}
    </div>
  );
}

/** Quiet notice once an update is downloaded and verified. Ignoring it is fine: it installs on quit. */
function UpdatePill() {
  const update = useServer((s) => s.update);
  if (update?.state !== 'ready' || !update.version) return null;
  return (
    <div className="update-pill" title={`Cardboard ${update.version} is downloaded. It installs when you restart or quit.`}>
      <ArrowUpCircle size={13} />
      <span>Update ready</span>
      {update.notesUrl && (
        <a href={update.notesUrl} target="_blank" rel="noreferrer">
          What's new
        </a>
      )}
      <button type="button" onClick={() => call('restart_to_update')}>
        Restart
      </button>
    </div>
  );
}
