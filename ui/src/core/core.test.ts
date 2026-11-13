import { describe, expect, it } from 'vitest';
import { describeError, eraseConfirmation, formatBytes, percent } from './format';
import { linesToList, optionalCount, optionalText } from './profile';

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

  it('describes whatever was thrown', () => {
    expect(describeError('plain')).toBe('plain');
    expect(describeError(new Error('boom'))).toBe('boom');
    expect(describeError(42)).toBe('something went wrong');
  });
});

describe('profile helpers', () => {
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
});
