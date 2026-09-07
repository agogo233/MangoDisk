import { describe, expect, it } from 'vitest';

import { LARGE_FILE_PREFERENCES_SCHEMA_VERSION, MAX_LARGE_FILE_EXCLUDED_FOLDERS } from '@/lib/models/large-file';
import * as LargeFilePreferenceUtils from '@/lib/utils/large-file-preference';

describe('LargeFilePreferenceUtils', () => {
  it('normalizes paths and collapses nested exclusions', () => {
    expect(
      LargeFilePreferenceUtils.parse({
        schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
        excludedFolders: ['/tmp/cache/nested', '/tmp/cache', '/tmp/downloads'],
      })
    ).toEqual({
      schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
      excludedFolders: ['/tmp/cache', '/tmp/downloads'],
    });
  });

  it('normalizes Windows extended path prefixes before persistence', () => {
    expect(
      LargeFilePreferenceUtils.parse({
        schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
        excludedFolders: ['\\\\?\\C:\\Users\\fixture\\Downloads'],
      }).excludedFolders
    ).toEqual(['C:\\Users\\fixture\\Downloads']);
  });

  it('rejects malformed and oversized preference documents', () => {
    expect(() => LargeFilePreferenceUtils.parse({ schemaVersion: 0, excludedFolders: [] })).toThrowError(
      'schemaVersionMismatch'
    );
    expect(() =>
      LargeFilePreferenceUtils.parse({
        schemaVersion: LARGE_FILE_PREFERENCES_SCHEMA_VERSION,
        excludedFolders: Array.from({ length: MAX_LARGE_FILE_EXCLUDED_FOLDERS + 1 }, (_, index) => `/tmp/${index}`),
      })
    ).toThrowError('excludedFoldersInvalid');
  });

  it('returns only a stable diagnostic reason', () => {
    expect(LargeFilePreferenceUtils.errorCode(new Error('/private/path'))).toBe('unexpected');
  });
});
