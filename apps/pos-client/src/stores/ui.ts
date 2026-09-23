import type { Locale } from '@pos/shared';
import { create } from 'zustand';
import { applyLocale } from '../i18n';

interface UiState {
  locale: Locale;
  setLocale: (locale: Locale) => void;
}

export const useUiStore = create<UiState>()((set) => ({
  locale: 'en',
  setLocale: (locale) => {
    applyLocale(locale);
    set({ locale });
  },
}));
