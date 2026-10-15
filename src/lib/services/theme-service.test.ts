// @vitest-environment happy-dom

import { afterEach, describe, expect, it } from 'vitest';
import { COLOR_THEME_IDS, THEME_IDS } from '@/lib/models/settings';
import { ThemeService } from './theme-service';

afterEach(() => {
  ThemeService.stopSystemListener();
  delete document.documentElement.dataset.theme;
  delete document.documentElement.dataset.colorTheme;
});

describe('independent color and brightness preferences', () => {
  it('keeps the selected palette when the system brightness changes', () => {
    ThemeService.applyColorTheme(COLOR_THEME_IDS.warmGray);
    ThemeService.apply(THEME_IDS.system);
    const systemPreference = ThemeService.mediaQuery!;
    systemPreference.dispatchEvent(new MediaQueryListEvent('change', { matches: true }));
    expect(document.documentElement.dataset.theme).toBe(THEME_IDS.dark);
    expect(document.documentElement.dataset.colorTheme).toBe(COLOR_THEME_IDS.warmGray);
    systemPreference.dispatchEvent(new MediaQueryListEvent('change', { matches: false }));
    expect(document.documentElement.dataset.theme).toBe(THEME_IDS.light);
    expect(document.documentElement.dataset.colorTheme).toBe(COLOR_THEME_IDS.warmGray);
  });

  it('ignores system brightness events after selecting a fixed mode', () => {
    ThemeService.apply(THEME_IDS.system);
    const previousPreference = ThemeService.mediaQuery!;
    ThemeService.apply(THEME_IDS.light);
    previousPreference.dispatchEvent(new MediaQueryListEvent('change', { matches: true }));
    expect(document.documentElement.dataset.theme).toBe(THEME_IDS.light);
    expect(ThemeService.mediaQuery).toBeNull();
  });

  it('switches back to classic colors without changing a fixed dark preference', () => {
    ThemeService.apply(THEME_IDS.dark);
    ThemeService.applyColorTheme(COLOR_THEME_IDS.warmGray);
    ThemeService.applyColorTheme(COLOR_THEME_IDS.mango);
    expect(document.documentElement.dataset.theme).toBe(THEME_IDS.dark);
    expect(document.documentElement.dataset.colorTheme).toBe(COLOR_THEME_IDS.mango);
    expect(ThemeService.mediaQuery).toBeNull();
  });
});
