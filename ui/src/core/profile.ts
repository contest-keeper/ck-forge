// The profile as the Rust side saves it (ck-protocol `Settings`); keep the two in step by hand.

export type Platform = 'kilonova' | 'codeforces' | 'vjudge' | 'other';
export type RecordingMode = 'off' | 'snapshots' | 'low' | 'standard';
export type RecordingUpload = 'keep' | 'after_contest' | 'during_contest';

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
  upload?: RecordingUpload | undefined;
  retention_days?: number | undefined;
};

export type Api = { url: string; heartbeat_s: number };

/** The venue network. The password is not part of the profile: it is typed on the Make page. */
export type Wifi = { ssid: string };

export type Settings = {
  contest: Contest;
  team: string;
  site: string;
  branding: Branding;
  allowed_domains: string[];
  recording: Recording;
  api: Api;
  block_bluetooth: boolean;
  // Left out of the file while off.
  block_usb_storage?: boolean | undefined;
  wifi?: Wifi | undefined;
};

export type StickKeyWindow = { valid_from: string; valid_until: string };

export type Profile = {
  name: string;
  settings: Settings;
  hub_id?: string | undefined;
  stick_key_window?: StickKeyWindow | undefined;
};

export const PLATFORMS: Platform[] = ['kilonova', 'codeforces', 'vjudge', 'other'];
export const RECORDING_MODES: RecordingMode[] = ['off', 'snapshots', 'low', 'standard'];

export const RECORDING_MODE_LABELS: Record<RecordingMode, string> = {
  off: 'Off: no recording',
  snapshots: 'Snapshots: a picture every few seconds',
  low: 'Low: video at 1 frame a second, half size',
  standard: 'Standard: video at 2 frames a second, full size',
};

/** Which recording settings matter for a mode, so the form shows only those. Hidden settings keep
 * their values: switching the mode back brings them back as they were. */
export function recordingFields(mode: RecordingMode): {
  timing: boolean;
  snapshotInterval: boolean;
  keeping: boolean;
} {
  return {
    timing: mode !== 'off',
    snapshotInterval: mode === 'snapshots',
    keeping: mode !== 'off',
  };
}

/** The window a new limit starts from: this minute to a day later, as UTC times. */
export function defaultKeyWindow(nowMs: number): StickKeyWindow {
  const utc = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const minute = Math.floor(nowMs / 60_000) * 60_000;
  return { valid_from: utc(minute), valid_until: utc(minute + 24 * 60 * 60 * 1000) };
}

const two = (n: number): string => String(n).padStart(2, '0');

/** A UTC time such as `2026-11-19T08:00:00Z` as the admin's own clock shows it, in the form a
 * date and time picker wants (`2026-11-19T10:00`). Empty when it is not a time. */
export function utcToLocalInput(utc: string): string {
  const date = new Date(utc);
  if (Number.isNaN(date.getTime())) return '';
  return (
    `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())}` +
    `T${two(date.getHours())}:${two(date.getMinutes())}`
  );
}

/** What a date and time picker holds (the admin's own clock) as the UTC time a stick key needs,
 * such as `2026-11-19T08:00:00Z`. Empty when the picker is empty. */
export function localInputToUtc(local: string): string {
  if (local === '') return '';
  const date = new Date(local);
  if (Number.isNaN(date.getTime())) return '';
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/** One entry per non-empty line, trimmed. */
export function linesToList(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '');
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

/** A required number field: a whole number of at least `min`, otherwise `min`, so the settings
 * always deserialize. */
export function wholeNumber(value: unknown, min: number): number {
  const number = Math.trunc(Number(value));
  return Number.isFinite(number) && number >= min ? number : min;
}

/** What Codeforces uses for a contest number: digits only. Same rule as the Rust side's `is_codeforces_id`. */
export function isCodeforcesId(id: string): boolean {
  return /^[0-9]+$/.test(id.trim());
}
