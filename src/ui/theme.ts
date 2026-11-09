import { useEffect, useState } from 'react';
import type { Theme } from '../render/palette';

const STORAGE_KEY = 'theme';
const THEME_COLOUR: Record<Theme, string> = { light: '#ffffff', dark: '#10161f' };

const systemQuery = () => window.matchMedia('(prefers-color-scheme: dark)');
const systemTheme = (): Theme => (systemQuery().matches ? 'dark' : 'light');

function hasStoredChoice() {
  try {
    return localStorage.getItem(STORAGE_KEY) !== null;
  } catch {
    return false;
  }
}

/** Remembers a choice only where it differs from the system, which is followed otherwise. */
function storeChoice(theme: Theme) {
  try {
    if (theme === systemTheme()) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Without storage the choice lasts until the page is closed.
  }
}

/** The script in index.html has already picked the first theme, before the first paint. */
export function useTheme() {
  const [theme, setTheme] = useState(() => document.documentElement.dataset.theme as Theme);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.querySelector('meta[name="theme-color"]')?.setAttribute('content', THEME_COLOUR[theme]);
  }, [theme]);

  useEffect(() => {
    const query = systemQuery();
    const follow = () => {
      if (!hasStoredChoice()) setTheme(systemTheme());
    };
    query.addEventListener('change', follow);
    return () => query.removeEventListener('change', follow);
  }, []);

  const toggle = () => {
    const next = theme === 'dark' ? 'light' : 'dark';
    storeChoice(next);
    setTheme(next);
  };
  return [theme, toggle] as const;
}
