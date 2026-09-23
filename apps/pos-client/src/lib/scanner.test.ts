import { describe, expect, it } from 'vitest';
import { ScanDetector } from './scanner';

function typeAll(detector: ScanDetector, keys: string, start: number, step: number) {
  let t = start;
  const results = [];
  for (const key of keys) {
    results.push(detector.feed(key, t));
    t += step;
  }
  return { results, t };
}

describe('ScanDetector', () => {
  it('recognises a scanner burst terminated by Enter', () => {
    const d = new ScanDetector();
    const { t } = typeAll(d, '6281000000017', 0, 8);
    expect(d.feed('Enter', t)).toEqual({ kind: 'scan', code: '6281000000017' });
  });

  it('ignores human typing (≥ 100 ms between keys)', () => {
    const d = new ScanDetector();
    const { t } = typeAll(d, '12345678', 0, 150);
    expect(d.feed('Enter', t)).toEqual({ kind: 'ignored' });
  });

  it('treats exactly 100 ms as keyboard speed', () => {
    const d = new ScanDetector();
    const { t } = typeAll(d, '12345678', 0, 100);
    expect(d.feed('Enter', t)).toEqual({ kind: 'ignored' });
  });

  it('a slow key restarts the burst so typed prefixes do not pollute scans', () => {
    const d = new ScanDetector();
    d.feed('x', 0);
    const { t } = typeAll(d, '99887766', 500, 5);
    expect(d.feed('Enter', t)).toEqual({ kind: 'scan', code: '99887766' });
  });

  it('clears the buffer after a timeout and after Enter', () => {
    const d = new ScanDetector();
    typeAll(d, '1234', 0, 5);
    // A lone Enter long after the burst is not a scan.
    expect(d.feed('Enter', 5_000)).toEqual({ kind: 'ignored' });
    const { t } = typeAll(d, '5678', 6_000, 5);
    expect(d.feed('Enter', t)).toEqual({ kind: 'scan', code: '5678' });
    expect(d.feed('Enter', t + 5)).toEqual({ kind: 'ignored' });
  });

  it('rejects codes shorter than the minimum and non-character keys', () => {
    const d = new ScanDetector();
    const { t } = typeAll(d, '12', 0, 5);
    expect(d.feed('Enter', t)).toEqual({ kind: 'ignored' });
    expect(d.feed('Shift', t + 10)).toEqual({ kind: 'ignored' });
  });
});
