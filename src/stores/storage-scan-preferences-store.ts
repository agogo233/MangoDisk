import { defineStore } from 'pinia';

import {
  SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION,
  type ScanExcludedFolder,
  type ScanExclusionScope,
  type ScanExclusionPreferences,
} from '@/lib/models/storage-scan';
import { LOG_DOMAINS, LOG_EVENTS } from '@/lib/models/telemetry';
import { LoggerService } from '@/lib/services/logger-service';
import { PreferenceStorageService } from '@/lib/services/preference-storage-service';
import * as PathUtils from '@/lib/utils/path';
import * as StorageScanPreferenceUtils from '@/lib/utils/storage-scan-preference';

interface StorageScanPreferencesState {
  folders: ScanExcludedFolder[];
  initialized: boolean;
}

const initializationByStore = new WeakMap<object, Promise<void>>();

function changedFolders(previous: readonly ScanExcludedFolder[], current: readonly ScanExcludedFolder[]) {
  const previousByPath = new Map(previous.map(folder => [PathUtils.comparisonKey(folder.path), folder]));
  const currentByPath = new Map(current.map(folder => [PathUtils.comparisonKey(folder.path), folder]));
  return [...new Set([...previousByPath.keys(), ...currentByPath.keys()])].flatMap(key => {
    const before = previousByPath.get(key);
    const after = currentByPath.get(key);
    const previousScopes = before?.scopes ?? [];
    const currentScopes = after?.scopes ?? [];
    if (
      previousScopes.length === currentScopes.length &&
      previousScopes.every((scope, index) => scope === currentScopes[index])
    ) {
      return [];
    }
    return [{ path: after?.path ?? before!.path, previousScopes, currentScopes }];
  });
}

export const useStorageScanPreferencesStore = defineStore('storage-scan-preferences', {
  state: (): StorageScanPreferencesState => ({
    folders: [],
    initialized: false,
  }),
  getters: {
    pathsForScope: state => (scope: ScanExclusionScope) =>
      StorageScanPreferenceUtils.pathsForScope(state.folders, scope),
  },
  actions: {
    async initialize() {
      if (this.initialized) return;
      const pending = initializationByStore.get(this);
      if (pending) return pending;

      const initialization = this.restore()
        .then(() => {
          this.initialized = true;
        })
        .finally(() => {
          initializationByStore.delete(this);
        });
      initializationByStore.set(this, initialization);
      return initialization;
    },
    async restore() {
      let saved: unknown | null;
      try {
        saved = await PreferenceStorageService.loadScanExclusionPreferences();
      } catch (error) {
        LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesLoadFailed, { error });
        throw error;
      }
      if (saved !== null) {
        try {
          this.folders = StorageScanPreferenceUtils.parse(saved).folders;
        } catch (error) {
          LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesInvalid, {
            source: 'scanExclusionPreferences',
            reason: StorageScanPreferenceUtils.errorCode(error),
          });
          throw error;
        }
        return;
      }
      await this.restoreLegacyPreferences();
    },
    async restoreLegacyPreferences() {
      let legacy: unknown | null;
      let source: 'storageScanPreferences' | 'largeFilePreferences' = 'storageScanPreferences';
      try {
        legacy = await PreferenceStorageService.loadLegacyStorageScanPreferences();
        if (legacy === null) {
          source = 'largeFilePreferences';
          legacy = await PreferenceStorageService.loadLegacyLargeFilePreferences();
        }
      } catch (error) {
        LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesLoadFailed, {
          source,
          error,
        });
        throw error;
      }
      if (legacy === null) return;

      let preferences: ScanExclusionPreferences;
      try {
        preferences =
          source === 'storageScanPreferences'
            ? StorageScanPreferenceUtils.fromSharedV1(legacy)
            : StorageScanPreferenceUtils.fromLargeFileV1(legacy);
      } catch (error) {
        LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesInvalid, {
          source,
          reason: StorageScanPreferenceUtils.errorCode(error),
        });
        throw error;
      }
      this.folders = preferences.folders;
      try {
        await PreferenceStorageService.migrateScanExclusionPreferences(preferences);
        LoggerService.info(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesMigrated, {
          source,
          excludedFolderCount: preferences.folders.length,
          fromSchemaVersion: 1,
          toSchemaVersion: SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION,
        });
      } catch (error) {
        // Keep valid values active. The legacy document remains available for a retry.
        LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesMigrationFailed, { error });
      }
    },
    async saveFolders(folders: ScanExcludedFolder[]) {
      await this.initialize();
      const preferences = StorageScanPreferenceUtils.parse({
        schemaVersion: SCAN_EXCLUSION_PREFERENCES_SCHEMA_VERSION,
        folders,
      });
      try {
        await PreferenceStorageService.saveScanExclusionPreferences(preferences);
        const changes = changedFolders(this.folders, preferences.folders);
        this.folders = preferences.folders;
        LoggerService.info(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanExclusionsUpdated, {
          excludedFolderCount: preferences.folders.length,
          changedFolderCount: changes.length,
          changedFolders: changes,
          cleanupCount: this.pathsForScope('cleanup').length,
          largeFileCount: this.pathsForScope('largeFiles').length,
          duplicateFileCount: this.pathsForScope('duplicateFiles').length,
          analysisCount: this.pathsForScope('analysis').length,
        });
      } catch (error) {
        LoggerService.warn(LOG_DOMAINS.storageScan, LOG_EVENTS.storageScanPreferencesSaveFailed, { error });
        throw error;
      }
    },
  },
});
