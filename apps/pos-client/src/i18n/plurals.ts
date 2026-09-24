/**
 * The locale files only spell out `_one` / `_other` (the English plural
 * forms). Languages with more forms (Arabic: zero, one, two, few, many,
 * other) would otherwise fall back to English for "2 seats" or "4 guests";
 * the missing forms reuse the locale's own `_other` text instead.
 */
interface Tree {
  readonly [key: string]: string | Tree;
}

export function fillPluralForms<T extends Tree>(tree: T, locale: string): T {
  const categories = new Intl.PluralRules(locale).resolvedOptions().pluralCategories;
  const out: Record<string, string | Tree> = {};
  for (const [key, value] of Object.entries(tree)) {
    out[key] = typeof value === 'string' ? value : fillPluralForms(value, locale);
  }
  for (const [key, value] of Object.entries(tree)) {
    if (typeof value !== 'string' || !key.endsWith('_other')) continue;
    const base = key.slice(0, -'_other'.length);
    for (const category of categories) {
      out[`${base}_${category}`] ??= value;
    }
  }
  return out as T;
}
