/**
 * Barcode-scanner detection from keystroke timing.
 *
 * USB/Bluetooth scanners "type" a code in a burst and finish with Enter.
 * Characters arriving < 100 ms apart are treated as a scanner; a gap of
 * ≥ 100 ms means a human is typing, and the buffer restarts from that key.
 * The buffer is cleared on Enter and after a timeout.
 */
export interface ScanDetectorOptions {
  /** Max gap between scanner keystrokes (spec: < 100 ms). */
  readonly maxInterKeyMs?: number;
  /** Shortest code accepted (EAN-8 is the shortest common symbology). */
  readonly minLength?: number;
  /** Drop a stale partial buffer after this long without keys. */
  readonly timeoutMs?: number;
}

export type ScanResult =
  | { readonly kind: 'buffering' }
  | { readonly kind: 'scan'; readonly code: string }
  | { readonly kind: 'ignored' };

export class ScanDetector {
  private buffer = '';
  private lastAt = Number.NEGATIVE_INFINITY;
  private readonly maxInterKeyMs: number;
  private readonly minLength: number;
  private readonly timeoutMs: number;

  constructor(options: ScanDetectorOptions = {}) {
    this.maxInterKeyMs = options.maxInterKeyMs ?? 100;
    this.minLength = options.minLength ?? 4;
    this.timeoutMs = options.timeoutMs ?? 300;
  }

  /** Feed one `KeyboardEvent.key` with its timestamp (ms). */
  feed(key: string, at: number): ScanResult {
    const gap = at - this.lastAt;
    if (gap >= this.timeoutMs) this.buffer = '';

    if (key === 'Enter') {
      const code = this.buffer;
      const fast = gap < this.maxInterKeyMs;
      this.reset();
      return fast && code.length >= this.minLength ? { kind: 'scan', code } : { kind: 'ignored' };
    }
    if (key.length !== 1) return { kind: 'ignored' };

    // A human-speed gap restarts the burst from this key.
    this.buffer = gap < this.maxInterKeyMs ? this.buffer + key : key;
    this.lastAt = at;
    return { kind: 'buffering' };
  }

  /** Whether the current buffer already looks like a scanner burst. */
  get inBurst(): boolean {
    return this.buffer.length >= 2;
  }

  reset(): void {
    this.buffer = '';
    this.lastAt = Number.NEGATIVE_INFINITY;
  }
}
