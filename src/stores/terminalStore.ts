// src/stores/terminalStore.ts
import { create } from 'zustand';

interface TabInfo {
  id: string;
  label: string;
  sessionId: string | null;
  cwd?: string;
}

interface TerminalSettings {
  fontSize: number;
  fontFamily: string;
  theme: string;
  scrollback: number;
  cursorStyle: 'block' | 'underline' | 'bar';
  cursorBlink: boolean;
}

interface TerminalState {
  tabs: TabInfo[];
  activeTabId: string;

  settings: TerminalSettings;

  searchBarVisible: boolean;
  settingsPanelVisible: boolean;
  contextMenuVisible: boolean;
  contextMenuPosition: { x: number; y: number };

  addTab: (tab: TabInfo) => void;
  removeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  updateTabLabel: (id: string, label: string) => void;

  updateSettings: (settings: Partial<TerminalSettings>) => void;

  toggleSearchBar: () => void;
  toggleSettingsPanel: () => void;
  showContextMenu: (x: number, y: number) => void;
  hideContextMenu: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  tabs: [{ id: 'tab-0', label: '终端 1', sessionId: null }],
  activeTabId: 'tab-0',

  settings: {
    fontSize: 14,
    fontFamily: 'Consolas',
    theme: 'gnome-dark',
    scrollback: 5000,
    cursorStyle: 'block',
    cursorBlink: true,
  },

  searchBarVisible: false,
  settingsPanelVisible: false,
  contextMenuVisible: false,
  contextMenuPosition: { x: 0, y: 0 },

  addTab: (tab) => set((state) => ({ tabs: [...state.tabs, tab] })),
  removeTab: (id) => set((state) => ({
    tabs: state.tabs.filter(t => t.id !== id),
    activeTabId: state.activeTabId === id ? state.tabs[0]?.id || 'tab-0' : state.activeTabId,
  })),
  setActiveTab: (id) => set({ activeTabId: id }),
  updateTabLabel: (id, label) => set((state) => ({
    tabs: state.tabs.map(t => t.id === id ? { ...t, label } : t),
  })),

  updateSettings: (newSettings) => set((state) => ({
    settings: { ...state.settings, ...newSettings },
  })),

  toggleSearchBar: () => set((state) => ({ searchBarVisible: !state.searchBarVisible })),
  toggleSettingsPanel: () => set((state) => ({ settingsPanelVisible: !state.settingsPanelVisible })),
  showContextMenu: (x, y) => set({ contextMenuVisible: true, contextMenuPosition: { x, y } }),
  hideContextMenu: () => set({ contextMenuVisible: false }),
}));