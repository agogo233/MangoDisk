import {
  MAX_SCAN_EXCLUDED_FOLDERS,
  SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION,
  SCAN_EXCLUSION_SCOPES,
  type ScanExcludedFolder,
  type ScanExclusionPreferences,
  type ScanExclusionScope,
} from '@/lib/models/storage-scan';
import * as PathUtils from '@/lib/utils/path';

export type ScanExclusionPreferenceErrorCode = 'schemaVersionMismatch' | 'foldersInvalid';

export class ScanExclusionPreferenceError extends Error {
  constructor(readonly code: ScanExclusionPreferenceErrorCode) {
    super(`Invalid scan-exclusion preferences: ${code}`);
    this.name = 'ScanExclusionPreferenceError';
  }
}

const scopeOrder = Object.values(SCAN_EXCLUSION_SCOPES);

export function empty(): ScanExclusionPreferences {
  return { schemaVersion: SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION, folders: [] };
}

export function parse(value: unknown): ScanExclusionPreferences {
  if (!isRecord(value) || value.schemaVersion !== SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION) {
    throw new ScanExclusionPreferenceError('schemaVersionMismatch');
  }
  if (!Array.isArray(value.folders) || value.folders.length > MAX_SCAN_EXCLUDED_FOLDERS) {
    throw new ScanExclusionPreferenceError('foldersInvalid');
  }

  const seen = new Set<string>();
  const folders = value.folders.map(value => {
    if (!isRecord(value) || typeof value.path !== 'string' || !value.path.trim() || !Array.isArray(value.scopes)) {
      throw new ScanExclusionPreferenceError('foldersInvalid');
    }
    const path = PathUtils.display(value.path.trim());
    const key = PathUtils.comparisonKey(path);
    if (!key || seen.has(key) || !value.scopes.length || value.scopes.length > scopeOrder.length) {
      throw new ScanExclusionPreferenceError('foldersInvalid');
    }
    const requestedScopes = value.scopes;
    const scopes = scopeOrder.filter(scope => requestedScopes.includes(scope));
    if (scopes.length !== requestedScopes.length) throw new ScanExclusionPreferenceError('foldersInvalid');
    seen.add(key);
    return { path, scopes };
  });

  return { schemaVersion: SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION, folders };
}

/** The first shared schema affected both file scans, not space analysis. */
export function fromSharedV1(value: unknown): ScanExclusionPreferences {
  return fromLegacyList(value, [SCAN_EXCLUSION_SCOPES.largeFiles, SCAN_EXCLUSION_SCOPES.duplicateFiles]);
}

/** The original large-file key must not silently change another module's behavior. */
export function fromLargeFileV1(value: unknown): ScanExclusionPreferences {
  return fromLegacyList(value, [SCAN_EXCLUSION_SCOPES.largeFiles]);
}

function fromLegacyList(value: unknown, scopes: ScanExclusionScope[]): ScanExclusionPreferences {
  if (!isRecord(value) || value.schemaVersion !== 1 || !Array.isArray(value.excludedFolders)) {
    throw new ScanExclusionPreferenceError('schemaVersionMismatch');
  }
  return parse({
    schemaVersion: SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION,
    folders: value.excludedFolders.map(path => ({ path, scopes })),
  });
}

export function pathsForScope(folders: readonly ScanExcludedFolder[], scope: ScanExclusionScope): string[] {
  return PathUtils.collapseOverlappingRoots(
    folders.filter(folder => folder.scopes.includes(scope)).map(folder => folder.path)
  );
}

export function sameExcludedFolders(left: readonly string[], right: readonly string[]): boolean {
  const leftKeys = left.map(PathUtils.comparisonKey).sort();
  const rightKeys = right.map(PathUtils.comparisonKey).sort();
  return leftKeys.length === rightKeys.length && leftKeys.every((key, index) => key === rightKeys[index]);
}

/** A result hint describes possible scan coverage, not whether files were actually skipped. */
export function hasExcludedFolderInScanRoots(roots: readonly string[], excludedFolders: readonly string[]): boolean {
  const rootKeys = roots.map(PathUtils.comparisonKey);
  return excludedFolders.some(folder => {
    const folderKey = PathUtils.comparisonKey(folder);
    return rootKeys.some(
      rootKey => PathUtils.isSameOrChildKey(folderKey, rootKey) || PathUtils.isSameOrChildKey(rootKey, folderKey)
    );
  });
}

export function errorCode(error: unknown): ScanExclusionPreferenceErrorCode | 'unexpected' {
  return error instanceof ScanExclusionPreferenceError ? error.code : 'unexpected';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
