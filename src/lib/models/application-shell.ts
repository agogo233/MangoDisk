import { ICON_NAMES } from './ui';

export const APP_NAME = 'MangoDisk' as const;
export const APP_ICON_PATH = '/mangodisk.svg' as const;
export const APP_SHELL_EXPANDED_MIN_WIDTH_PX = 1100;

export function isAppShellExpanded(viewportWidth: number): boolean {
  return viewportWidth >= APP_SHELL_EXPANDED_MIN_WIDTH_PX;
}

export const PROJECT_LINKS = {
  website: 'https://mangodisk.app',
  repository: 'https://github.com/harry0703/mangodisk',
  issues: 'https://github.com/harry0703/mangodisk/issues',
  license: 'https://github.com/harry0703/mangodisk/blob/main/LICENSE',
} as const;

export const PAGE_IDS = {
  cleanup: 'cleanup',
  analysis: 'analysis',
  largeFiles: 'large-files',
  duplicateFiles: 'duplicate-files',
  applicationUninstall: 'application-uninstall',
  startup: 'startup',
  history: 'history',
  settings: 'settings',
} as const;

export type PageId = (typeof PAGE_IDS)[keyof typeof PAGE_IDS];

export const PRIMARY_NAV_ITEMS = [
  { id: PAGE_IDS.cleanup, icon: ICON_NAMES.deepCleanup },
  { id: PAGE_IDS.largeFiles, icon: ICON_NAMES.largeFiles },
  { id: PAGE_IDS.duplicateFiles, icon: ICON_NAMES.duplicateFiles },
  { id: PAGE_IDS.applicationUninstall, icon: ICON_NAMES.uninstall },
  { id: PAGE_IDS.startup, icon: ICON_NAMES.startup },
  { id: PAGE_IDS.analysis, icon: ICON_NAMES.analysis },
] as const;

export const SECONDARY_NAV_ITEMS = [
  { id: PAGE_IDS.history, icon: ICON_NAMES.history },
  { id: PAGE_IDS.settings, icon: ICON_NAMES.settings },
] as const;
