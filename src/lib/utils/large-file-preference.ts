import {
  LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
  MAX_LARGE_FILE_EXCLUDED_FOLDERS,
  type LargeFilePreferences,
} from '@/lib/models/large-file';
import * as PathUtils from '@/lib/utils/path';

export type LargeFilePreferenceErrorCode = 'schemaVersionMismatch' | 'excludedFoldersInvalid';

export class LargeFilePreferenceError extends Error {
  constructor(readonly code: LargeFilePreferenceErrorCode) {
    super(`Invalid large-file preferences: ${code}`);
    this.name = 'LargeFilePreferenceError';
  }
}

export function empty(): LargeFilePreferences {
  return {
    schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
    excludedFolders: [],
  };
}

export function parse(value: unknown): LargeFilePreferences {
  if (!isRecord(value) || value.schemaVersion !== LARGE_FILE_PREFERENCES_SCHEMA_VERSION) {
    throw new LargeFilePreferenceError('schemaVersionMismatch');
  }
  if (!Array.isArray(value.excludedFolders) || value.excludedFolders.length > MAX_LARGE_FILE_EXCLUDED_FOLDERS) {
    throw new LargeFilePreferenceError('excludedFoldersInvalid');
  }

  const seen = new Set<string>();
  const folders = value.excludedFolders.map(value => {
    if (typeof value !== 'string' || !value.trim()) {
      throw new LargeFilePreferenceError('excludedFoldersInvalid');
    }
    const path = PathUtils.display(value.trim());
    const key = PathUtils.comparisonKey(path);
    if (!key || seen.has(key)) throw new LargeFilePreferenceError('excludedFoldersInvalid');
    seen.add(key);
    return path;
  });

  return {
    schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
    excludedFolders: PathUtils.collapseOverlappingRoots(folders),
  };
}

export function errorCode(error: unknown): LargeFilePreferenceErrorCode | 'unexpected' {
  return error instanceof LargeFilePreferenceError ? error.code : 'unexpected';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
