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

/** A whole percent from 0 to 100; 0 when the total is not known yet. */
export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.floor((done / total) * 100));
}

/** The text the admin types to confirm erasing a disk. */
export function eraseConfirmation(diskNumber: number): string {
  return `erase ${diskNumber}`;
}

/** What went wrong, in words, from whatever was thrown. */
export function describeError(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'something went wrong';
}
