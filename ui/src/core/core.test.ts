import { describe, expect, it } from 'vitest';
import { newestOnly } from './latest';
import {
  describeError,
  eraseConfirmation,
  formatBytes,
  formatDuration,
  formatRate,
  percent,
  secondsLeft,
  stickLabel,
} from './format';
import {
  RECORDING_MODES,
  defaultKeyWindow,
  isCodeforcesId,
  linesToList,
  localInputToUtc,
  optionalCount,
  optionalText,
  recordingFields,
  utcToLocalInput,
  wholeNumber,
} from './profile';

describe('format', () => {
  it('shows sizes in decimal units', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(29_884_416_000)).toBe('29.9 GB');
  });

  it('keeps the percent between 0 and 100', () => {
    expect(percent(0, 0)).toBe(0);
    expect(percent(50, 200)).toBe(25);
    expect(percent(300, 200)).toBe(100);
  });

  it('asks for the numbers of every disk, in order, before erasing', () => {
    expect(eraseConfirmation([3])).toBe('erase 3');
    expect(eraseConfirmation([5, 3, 4])).toBe('erase 3 4 5');
  });

  it('names a stick by its team, site and machine, not its id', () => {
    expect(stickLabel({ team: 'Team A', site: 'Lab 1', machine_number: 3 })).toBe(
      'Team A · Lab 1 · machine 3',
    );
    expect(stickLabel({ team: '', site: 'Lab 1', machine_number: null })).toBe('Lab 1');
    expect(stickLabel({ team: '', site: '', machine_number: null })).toBe('A stick without a name');
  });

  it('shows a time span as minutes and seconds', () => {
    expect(formatDuration(0)).toBe('0:00');
    expect(formatDuration(72.9)).toBe('1:12');
    expect(formatDuration(872)).toBe('14:32');
    expect(formatDuration(3725)).toBe('1:02:05');
    expect(formatDuration(-5)).toBe('0:00');
  });

  it('shows the speed once there is something to measure', () => {
    expect(formatRate(1000, 1)).toBeNull();
    expect(formatRate(0, 60)).toBeNull();
    expect(formatRate(570_000_000, 60)).toBe('9.5 MB/s');
  });

  it('works out the time left from the speed so far', () => {
    expect(secondsLeft(100, 1000, 2)).toBeNull();
    expect(secondsLeft(0, 1000, 60)).toBeNull();
    expect(secondsLeft(1000, 1000, 60)).toBeNull();
    // 250 bytes in 10 s is 25 a second, so 750 more take 30 s.
    expect(secondsLeft(250, 1000, 10)).toBe(30);
  });

  it('describes whatever was thrown', () => {
    expect(describeError('plain')).toBe('plain');
    expect(describeError(new Error('boom'))).toBe('boom');
    expect(describeError(42)).toBe('something went wrong');
  });
});

describe('stick key times', () => {
  it('goes from a picker to UTC and back without changing the moment', () => {
    for (const utc of ['2026-11-19T08:00:00Z', '2026-03-29T01:30:00Z', '2026-12-31T23:59:00Z']) {
      expect(localInputToUtc(utcToLocalInput(utc))).toBe(utc);
    }
  });

  it('writes UTC the way the Rust side parses it', () => {
    expect(localInputToUtc('2026-11-19T10:00')).toMatch(/^2026-11-19T\d\d:\d\d:00Z$/);
  });

  it('starts a new limit from this minute to a day later', () => {
    expect(defaultKeyWindow(Date.UTC(2026, 10, 19, 8, 0, 42))).toEqual({
      valid_from: '2026-11-19T08:00:00Z',
      valid_until: '2026-11-20T08:00:00Z',
    });
  });

  it('gives nothing for an empty or unreadable time', () => {
    expect(localInputToUtc('')).toBe('');
    expect(localInputToUtc('soon')).toBe('');
    expect(utcToLocalInput('')).toBe('');
    expect(utcToLocalInput('soon')).toBe('');
  });
});

describe('recording settings by mode', () => {
  it('shows nothing else when recording is off', () => {
    expect(recordingFields('off')).toEqual({
      timing: false,
      snapshotInterval: false,
      keeping: false,
    });
  });

  it('asks for the snapshot interval only in snapshots mode', () => {
    for (const mode of RECORDING_MODES) {
      expect(recordingFields(mode).snapshotInterval).toBe(mode === 'snapshots');
    }
  });

  it('asks about timing and keeping in every mode that records', () => {
    for (const mode of RECORDING_MODES.filter((m) => m !== 'off')) {
      const fields = recordingFields(mode);
      expect([fields.timing, fields.keeping]).toEqual([true, true]);
    }
  });
});

describe('profile helpers', () => {
  it('takes digits only as a Codeforces contest number', () => {
    expect(isCodeforcesId(' 1985 ')).toBe(true);
    expect([isCodeforcesId('12x4'), isCodeforcesId(''), isCodeforcesId('-5')]).toEqual([
      false,
      false,
      false,
    ]);
  });

  it('turns lines into a list', () => {
    expect(linesToList(' a.org \n\n b.org\n')).toEqual(['a.org', 'b.org']);
  });

  it('leaves blank optional fields out', () => {
    expect(optionalText('  ')).toBeUndefined();
    expect(optionalText(' x ')).toBe('x');
    expect(optionalCount('')).toBeUndefined();
    expect(optionalCount('0')).toBeUndefined();
    expect(optionalCount('10')).toBe(10);
  });

  it('keeps required numbers whole and at least their minimum', () => {
    expect(wholeNumber('15', 1)).toBe(15);
    expect(wholeNumber(-3, 0)).toBe(0);
    expect(wholeNumber(2.7, 1)).toBe(2);
    expect(wholeNumber('', 1)).toBe(1);
    expect(wholeNumber(Number.NaN, 0)).toBe(0);
  });
});

describe('newestOnly', () => {
  it('keeps only the newest request current', () => {
    const begin = newestOnly();
    const first = begin();
    const second = begin();
    expect(first()).toBe(false);
    expect(second()).toBe(true);
  });
});
