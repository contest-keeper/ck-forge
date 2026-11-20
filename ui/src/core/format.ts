const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Decimal units, like the box of a USB stick: 29884416000 is "29.9 GB". */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return unit === 0 ? `${value} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
}

/** A time span as minutes and seconds (`4:12`), or with hours (`1:02:05`). */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = String(total % 60).padStart(2, '0');
  if (hours > 0) return `${hours}:${String(minutes).padStart(2, '0')}:${rest}`;
  return `${minutes}:${rest}`;
}

/** How fast bytes went, such as `9.5 MB/s`. Nothing until there are a few seconds to measure. */
export function formatRate(bytes: number, seconds: number): string | null {
  if (seconds < 3 || bytes <= 0) return null;
  return `${formatBytes(bytes / seconds)}/s`;
}

/** Seconds left at the speed so far, or nothing while that cannot be told. */
export function secondsLeft(done: number, total: number, seconds: number): number | null {
  if (seconds < 5 || done <= 0 || done >= total) return null;
  return Math.round((total - done) / (done / seconds));
}

/** A whole percent from 0 to 100; 0 when the total is not known yet. */
export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.floor((done / total) * 100));
}

/** The text the admin types to confirm erasing disks: "erase 3", or "erase 3 4" for two. */
export function eraseConfirmation(diskNumbers: number[]): string {
  return `erase ${[...diskNumbers].sort((a, b) => a - b).join(' ')}`;
}

/** What an admin calls a stick: its team, site and machine number, never its id. */
export function stickLabel(stick: {
  team: string;
  site: string;
  machine_number: number | null;
}): string {
  const parts = [stick.team, stick.site];
  if (stick.machine_number !== null) parts.push(`machine ${stick.machine_number}`);
  return parts.filter((part) => part.trim() !== '').join(' · ') || 'A stick without a name';
}

/** What went wrong, in words, from whatever was thrown. */
export function describeError(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'something went wrong';
}
