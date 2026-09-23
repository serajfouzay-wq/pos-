import { create } from 'zustand';

export const SECTIONS = ['clients', 'builds', 'licenses', 'settings'] as const;
export type Section = (typeof SECTIONS)[number];

export const CLIENT_TABS = ['details', 'receipt', 'licenses', 'builds'] as const;
export type ClientTab = (typeof CLIENT_TABS)[number];

interface NavigationState {
  section: Section;
  /** Client open in the editor (Clients section). */
  clientId: string | null;
  clientTab: ClientTab;
  navigate: (section: Section) => void;
  openClient: (clientId: string, tab?: ClientTab) => void;
  closeClient: () => void;
  setClientTab: (tab: ClientTab) => void;
}

export const useNavigationStore = create<NavigationState>()((set) => ({
  section: 'clients',
  clientId: null,
  clientTab: 'details',
  navigate: (section) => {
    set({ section });
  },
  openClient: (clientId, tab = 'details') => {
    set({ section: 'clients', clientId, clientTab: tab });
  },
  closeClient: () => {
    set({ clientId: null });
  },
  setClientTab: (clientTab) => {
    set({ clientTab });
  },
}));
