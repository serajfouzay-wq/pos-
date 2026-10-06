import { LOCALES, type Locale } from '@pos/shared';
import { create } from 'zustand';
import { applyLocale } from '../i18n';

const LOCALE_KEY = 'generator.locale';

/** The language chosen last time (this PC only); English when unknown. */
function savedLocale(): Locale {
  try {
    const value = localStorage.getItem(LOCALE_KEY);
    return LOCALES.find((l) => l === value) ?? 'en';
  } catch {
    return 'en';
  }
}

interface UiState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
}

const initial = savedLocale();
applyLocale(initial);

export const useUiStore = create<UiState>()((set) => ({
  locale: initial,
  setLocale: (locale) => {
    applyLocale(locale);
    try {
      localStorage.setItem(LOCALE_KEY, locale);
    } catch {
      // Storage unavailable: the choice lasts until the app closes.
    }
    set({ locale });
  },
}));
