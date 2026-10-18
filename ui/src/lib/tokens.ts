// Mirrors src/theme.rs `tokens_css`: `:root` renders the default mode, and each mode re-declares
// the full resolved set under [data-mode="…"] so tokens that reference other tokens re-resolve.
export const BASE_MODE = 'light';

export function resolvedTokens(tokens: Record<string, string>, modes: Record<string, Record<string, string>> | undefined, mode: string) {
  return { ...tokens, ...(modes?.[mode] ?? {}) };
}

export function modeNames(modes: Record<string, Record<string, string>> | undefined) {
  return [BASE_MODE, ...Object.keys(modes ?? {}).sort()];
}

export function tokensCss(tokens: Record<string, string>, modes?: Record<string, Record<string, string>>, defaultMode?: string | null) {
  const block = (selector: string, t: Record<string, string>) =>
    `${selector}{${Object.entries(t)
      .map(([k, v]) => `${k}:${v}`)
      .join(';')}}`;
  let css = block(':root', resolvedTokens(tokens, modes, defaultMode || BASE_MODE));
  if (modes && Object.keys(modes).length) {
    for (const mode of modeNames(modes)) css += block(`[data-mode="${mode}"]`, resolvedTokens(tokens, modes, mode));
  }
  return css;
}
