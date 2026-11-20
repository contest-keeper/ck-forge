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
  /** Something long is running (an import, a download or a write), so other buttons wait. */
  busy: boolean;
};

export const app: AppState = $state({
  profile: null,
  profilePath: null,
  basePath: localStorage.getItem(BASE_KEY),
  written: false,
  hubEmail: null,
  busy: false,
});

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
  if (app.profilePath) {
    await api.saveProfile(app.profilePath, snapshot);
    return app.profilePath;
  }
  const path = await api.saveProfileInForgeFolder(snapshot);
  app.profilePath = path;
  return path;
}
