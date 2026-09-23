import { z } from 'zod';

export const LOCALES = ['en', 'ar'] as const;
export const LocaleSchema = z.enum(LOCALES);
export type Locale = z.infer<typeof LocaleSchema>;

const RTL_LANGUAGES: ReadonlySet<string> = new Set(['ar', 'fa', 'he', 'ur']);

export type TextDirection = 'ltr' | 'rtl';

/** `'ar'`, `'ar-KW'` → `'rtl'`. Works for any BCP-47 tag, not only supported locales. */
export function textDirection(locale: string): TextDirection {
  const language = locale.split('-')[0]?.toLowerCase() ?? '';
  return RTL_LANGUAGES.has(language) ? 'rtl' : 'ltr';
}
