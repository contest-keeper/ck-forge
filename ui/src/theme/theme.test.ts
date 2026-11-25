import { beforeEach, describe, expect, it, vi } from 'vitest';

// The store reads localStorage and the page when it loads, so each test loads it fresh with stand-ins.
async function load(saved: Record<string, string>, systemDark = false) {
  vi.resetModules();
  const kept = { ...saved };
  const root = { dataset: {} as Record<string, string> };
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => kept[key] ?? null,
    setItem: (key: string, value: string) => void (kept[key] = value),
  });
  vi.stubGlobal('document', { documentElement: root });
  vi.stubGlobal('matchMedia', () => ({ matches: systemDark }));
  return { kept, root, theme: await import('./theme.svelte') };
}

describe('the look of the window', () => {
  beforeEach(() => vi.unstubAllGlobals());

  it('starts new and from the system setting when nothing was chosen', async () => {
    const { theme } = await load({}, true);
    expect(theme.look).toEqual({ theme: 'modern', mode: 'dark' });
  });

  it('keeps an earlier classic choice and remembers both choices', async () => {
    const { theme, kept, root } = await load({ 'ck-theme': 'classic' });
    expect(theme.look.theme).toBe('classic');
    theme.setMode('dark');
    theme.setTheme('modern');
    expect(kept).toMatchObject({ 'ck-mode': 'dark', 'ck-theme': 'modern' });
    expect(root.dataset).toEqual({ theme: 'modern', mode: 'dark' });
  });

  it('puts a stored look on the page before anything is drawn', async () => {
    const { theme, root } = await load({ 'ck-mode': 'dark', 'ck-theme': 'classic' });
    theme.applyLook();
    expect(root.dataset).toEqual({ theme: 'classic', mode: 'dark' });
  });
});
