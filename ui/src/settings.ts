import { useSyncExternalStore } from 'react';

/** Per-machine UI preferences. Browser storage is fine here: losing them only resets cosmetics. */
export interface Settings {
  theme: 'system' | 'light' | 'dark';
  dashboardLayout: 'grid' | 'list';
}
const DEFAULTS: Settings = { theme: 'system', dashboardLayout: 'grid' };
let settings: Settings = (() => {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem('cardboard.settings') ?? '{}') };
  } catch {
    return DEFAULTS;
  }
})();
const listeners = new Set<() => void>();
export function getSettings() {
  return settings;
}
export function setSettings(patch: Partial<Settings>) {
  settings = { ...settings, ...patch };
  try {
    localStorage.setItem('cardboard.settings', JSON.stringify(settings));
  } catch {
    /* storage unavailable */
  }
  applyTheme();
  for (const l of listeners) l();
}
export function useSettings() {
  return useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => settings,
  );
}
export function applyTheme() {
  const t = settings.theme;
  if (t === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
}
