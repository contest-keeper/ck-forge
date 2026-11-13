// What the pages share: the profile being edited and the image just made.

import type { Profile } from './core/profile';

export type AppState = {
  profile: Profile | null;
  /** Where the profile is saved. Building needs it, because the registry and the wallpaper sit next to it. */
  profilePath: string | null;
  /** The newest stick image made here, offered on the page that writes sticks. */
  imagePath: string | null;
  /** A stick was written and read back in this session. */
  written: boolean;
  /** The registry was sent to the Hub in this session. */
  registered: boolean;
};

export const app: AppState = $state({
  profile: null,
  profilePath: null,
  imagePath: null,
  written: false,
  registered: false,
});
