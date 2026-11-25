// What the pages share: the profile being edited and the image just made.

import type { Profile } from './core/profile';
import * as api from './lib/api';

const BASE_KEY = 'ck.basePath';

export type AppState = {
  profile: Profile | null;
  /** Where the profile is saved. Building needs it, because the registry and the wallpaper sit next to it. */
  profilePath: string | null;
  /** The base image every stick starts from; remembered between runs. */
  basePath: string | null;
  /** A stick was written and read back in this session. */
  written: boolean;
  /** The admin signed in to the Hub in this window: sticks made now report to it. */
  hubEmail: string | null;
  /** The profile as it was last saved or loaded, to tell whether it changed since. */
  saved: string | null;
  /** Something long is running (an import, a download or a write), so other buttons wait. */
  busy: boolean;
  /** Goes up when a wallpaper file is replaced; replacing keeps the name, so the previews watch this. */
  wallpaperRevision: number;
};

export const app: AppState = $state({
  profile: null,
  profilePath: null,
  basePath: localStorage.getItem(BASE_KEY),
  written: false,
  hubEmail: null,
  saved: null,
  busy: false,
  wallpaperRevision: 0,
});

/** Takes a profile in as the one being edited. `path` is where it came from, `null` for a new one. */
export function useProfile(profile: Profile, path: string | null) {
  app.profile = profile;
  app.profilePath = path;
  app.saved = path === null ? null : JSON.stringify(profile);
}

/** Whether the profile has edits that are not saved. */
export function isUnsaved(): boolean {
  return app.profile !== null && JSON.stringify($state.snapshot(app.profile)) !== app.saved;
}

export function setBasePath(path: string) {
  app.basePath = path;
  localStorage.setItem(BASE_KEY, path);
}

/**
 * Saves the profile being edited and returns where it is. A profile that was saved or opened is
 * saved to the same file. A new one goes into Forge's own folder, in a folder of its own, without
 * a question, and keeps that place from then on. Throws the reason when the profile has problems.
 */
export async function saveProfile(): Promise<string> {
  const profile = app.profile;
  if (!profile) throw new Error('There is no profile to save.');
  const snapshot = $state.snapshot(profile);
  const known = app.profilePath;
  if (known) {
    await api.saveProfile(known, snapshot);
    // Another profile may have been opened while this one was saved; its baseline is not this save's.
    if (app.profile === profile && app.profilePath === known) app.saved = JSON.stringify(snapshot);
    return known;
  }
  const path = await api.saveProfileInForgeFolder(snapshot);
  if (app.profile === profile && app.profilePath === null) {
    app.profilePath = path;
    app.saved = JSON.stringify(snapshot);
  }
  return path;
}
