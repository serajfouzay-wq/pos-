/** Quantities are integer thousandths of a unit (1000 = 1, 250 = 0.25 kg). */

/** `2000` → `"2"`, `250` → `"0.25"`, `-250` → `"-0.25"` (display only). */
export function formatQuantity(quantityMilli: number): string {
  const sign = quantityMilli < 0 ? '-' : '';
  const abs = Math.abs(quantityMilli);
  const whole = Math.trunc(abs / 1000);
  const frac = abs % 1000;
  if (frac === 0) return `${sign}${String(whole)}`;
  return `${sign}${String(whole)}.${String(frac).padStart(3, '0').replace(/0+$/, '')}`;
}

/**
 * `"2.5"` → 2500 thousandths (integer, no floats); `""` → null; anything
 * else (or a sign when not allowed) → undefined.
 */
export function parseQuantity(text: string, allowNegative = false): number | null | undefined {
  const trimmed = text.trim();
  if (!trimmed) return null;
  const match = /^(-)?(\d{1,9})(?:\.(\d{1,3}))?$/.exec(trimmed);
  if (!match || (match[1] && !allowNegative)) return undefined;
  const milli = Number(match[2]) * 1000 + Number((match[3] ?? '').padEnd(3, '0'));
  return match[1] ? -milli : milli;
}
