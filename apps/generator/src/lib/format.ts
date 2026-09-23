/**
 * Pure helpers for the generator forms. Rates are integer basis points end to
 * end: the percentage the operator types is converted by string arithmetic,
 * never through a float.
 */

/** "5" → 500, "5.25" → 525, "" → 0; `null` for anything else (or > 100 %). */
export function parseBps(input: string): number | null {
  const text = input.trim();
  if (text === '') return 0;
  const match = /^(\d{1,3})(?:\.(\d{1,2}))?$/.exec(text);
  if (!match) return null;
  const whole = Number(match[1]);
  const fraction = Number((match[2] ?? '').padEnd(2, '0'));
  const bps = whole * 100 + fraction;
  return bps <= 10_000 ? bps : null;
}

/** 525 → "5.25", 500 → "5", 0 → "0". */
export function formatBps(bps: number): string {
  const whole = Math.trunc(bps / 100);
  const fraction = bps % 100;
  if (fraction === 0) return String(whole);
  return `${String(whole)}.${String(fraction).padStart(2, '0').replace(/0$/, '')}`;
}

/** "Al-Noor Café & Bakery" → "al-noor-cafe-bakery" (≤ 40 chars, kebab-case). */
export function slugify(name: string): string {
  return name
    .normalize('NFKD')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 40)
    .replace(/-+$/g, '');
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} B`;
  if (bytes < 1024 * 1024) return `${String(Math.round(bytes / 1024))} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatDateTime(iso: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(
    new Date(iso),
  );
}

/** Contents of an uploaded file as plain base64 (no data-URL prefix). */
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => {
      reject(reader.error ?? new Error('could not read the file'));
    };
    reader.onload = () => {
      const result = typeof reader.result === 'string' ? reader.result : '';
      resolve(result.slice(result.indexOf(',') + 1));
    };
    reader.readAsDataURL(file);
  });
}

/**
 * Header lines are edited as one text block, one line per receipt line.
 * Nothing is trimmed: the operator is mid-typing (a trailing space is the
 * start of the next word).
 */
export function linesFromText(text: string): string[] {
  return text.split('\n');
}
