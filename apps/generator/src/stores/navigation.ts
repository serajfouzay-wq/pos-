import { create } from 'zustand';

export const SECTIONS = ['clients', 'builds', 'licenses', 'settings'] as const;
export type Section = (typeof SECTIONS)[number];

interface NavigationState {
  section: Section;
  navigate: (section: Section) => void;
}

export const useNavigationStore = create<NavigationState>()((set) => ({
  section: 'clients',
  navigate: (section) => {
    set({ section });
  },
}));
