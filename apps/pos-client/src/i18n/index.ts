import { textDirection, type Locale } from '@pos/shared';
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { ar } from './locales/ar';
import { en } from './locales/en';
import { fillPluralForms } from './plurals';

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common';
    resources: { common: typeof en };
  }
}

void i18n.use(initReactI18next).init({
  resources: { en: { common: en }, ar: { common: fillPluralForms(ar, 'ar') } },
  lng: 'en',
  fallbackLng: 'en',
  defaultNS: 'common',
  interpolation: { escapeValue: false }, // React already escapes
  returnNull: false,
});

/** Switches language and flips the document direction for RTL locales. */
export function applyLocale(locale: Locale): void {
  void i18n.changeLanguage(locale);
  document.documentElement.lang = locale;
  document.documentElement.dir = textDirection(locale);
}

export { i18n };
