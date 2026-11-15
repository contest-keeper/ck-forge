// What the pages share: the profile being edited and the image just made.

import type { Profile } from './core/profile';

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
};

export const app: AppState = $state({
  profile: null,
  profilePath: null,
  basePath: localStorage.getItem(BASE_KEY),
  written: false,
  hubEmail: null,
});

export function setBasePath(path: string) {
  app.basePath = path;
  localStorage.setItem(BASE_KEY, path);
}
