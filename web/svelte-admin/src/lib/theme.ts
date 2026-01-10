import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

type Theme = 'light' | 'dark' | 'system';

function applyTheme(theme: Theme) {
  if (!browser) return;

  const root = document.documentElement;
  const isDark =
    theme === 'dark' ||
    (theme === 'system' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches);

  if (isDark) {
    root.classList.add('dark');
  } else {
    root.classList.remove('dark');
  }
}

function createThemeStore() {
  const defaultTheme: Theme = 'system';

  const { subscribe, set } = writable<Theme>(defaultTheme);

  return {
    subscribe,
    set: (theme: Theme) => {
      set(theme);
      if (browser) {
        localStorage.setItem('theme', theme);
        applyTheme(theme);
      }
    },
    init: () => {
      if (browser) {
        const stored = localStorage.getItem('theme') as Theme | null;
        const theme = stored || defaultTheme;
        set(theme);
        applyTheme(theme);
      }
    },
  };
}

export const theme = createThemeStore();

// Listen to system theme changes
if (browser) {
  window
    .matchMedia('(prefers-color-scheme: dark)')
    .addEventListener('change', () => {
      const currentTheme = get(theme);
      if (currentTheme === 'system') {
        applyTheme('system');
      }
    });
}

