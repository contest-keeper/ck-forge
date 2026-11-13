// The look of the window. Classic is the Windows 9x style of the browser extension, modern the new one.

export type Theme = 'classic' | 'modern';

const KEY = 'ck-theme';

function stored(): Theme {
  try {
    return localStorage.getItem(KEY) === 'classic' ? 'classic' : 'modern';
  } catch {
    return 'modern';
  }
}

export const look: { theme: Theme } = $state({ theme: stored() });

/** Switch the look, remember it for the next start and apply it to the page. */
export function setTheme(theme: Theme) {
  look.theme = theme;
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // The choice then lasts until the window closes.
  }
}
