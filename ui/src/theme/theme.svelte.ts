// The look of the window: a style (Classic is the Windows 9x style of the browser extension, modern is
// the new one) and a mode (light or dark). Both are chosen in the Settings panel and remembered.

export type Theme = 'classic' | 'modern';
export type Mode = 'light' | 'dark';

const KEY = 'ck-theme';
const MODE_KEY = 'ck-mode';

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storedMode(): Mode {
  const saved = read(MODE_KEY);
  if (saved === 'light' || saved === 'dark') return saved;
  // Nothing chosen yet: start from the system setting, as the window did before the choice existed.
  return globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

export const look: { theme: Theme; mode: Mode } = $state({
  theme: read(KEY) === 'classic' ? 'classic' : 'modern',
  mode: storedMode(),
});

function apply() {
  document.documentElement.dataset.theme = look.theme;
  document.documentElement.dataset.mode = look.mode;
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice then lasts until the window closes.
  }
}

/** Applies the stored look to the page. Called before the first page is drawn. */
export function applyLook() {
  apply();
}

/** Switch the style, remember it for the next start and apply it to the page. */
export function setTheme(theme: Theme) {
  look.theme = theme;
  apply();
  remember(KEY, theme);
}

/** Switch between light and dark, remember it for the next start and apply it to the page. */
export function setMode(mode: Mode) {
  look.mode = mode;
  apply();
  remember(MODE_KEY, mode);
}
