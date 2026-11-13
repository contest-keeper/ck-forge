// The profile as the Rust side saves it. Pure data and helpers: no browser, no Tauri.

export type Platform = 'kilonova' | 'codeforces' | 'vjudge' | 'other';
export type RecordingMode = 'off' | 'snapshots' | 'low' | 'standard';

export type Bookmark = { title: string; url: string };

export type Contest = {
  name: string;
  platform: Platform;
  contest_id: string;
  start?: string | undefined;
  end?: string | undefined;
  practice: boolean;
};

export type Branding = {
  display_name: string;
  computer_name_pattern: string;
  usb_label: string;
  wallpaper?: string | undefined;
  bookmarks: Bookmark[];
  home_page?: string | undefined;
  welcome_text: string;
};

export type Recording = {
  mode: RecordingMode;
  start_delay_s: number;
  start_at?: string | undefined;
  snapshot_interval_s?: number | undefined;
};

export type Api = { url: string; heartbeat_s: number };

export type Settings = {
  contest: Contest;
  team: string;
  site: string;
  branding: Branding;
  allowed_domains: string[];
  recording: Recording;
  api: Api;
};

export type Profile = { name: string; settings: Settings };

export const PLATFORMS: Platform[] = ['kilonova', 'codeforces', 'vjudge', 'other'];
export const RECORDING_MODES: RecordingMode[] = ['off', 'snapshots', 'low', 'standard'];

/** One entry per non-empty line, trimmed. */
export function linesToList(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '');
}

export function listToLines(list: string[]): string {
  return list.join('\n');
}

/** An optional text field is left out of the profile when it is blank, as the Rust side expects. */
export function optionalText(text: string): string | undefined {
  const trimmed = text.trim();
  return trimmed === '' ? undefined : trimmed;
}

/** An optional number field is left out when blank or not a whole number of at least 1. */
export function optionalCount(text: string): number | undefined {
  const value = Number.parseInt(text, 10);
  return Number.isInteger(value) && value >= 1 ? value : undefined;
}
