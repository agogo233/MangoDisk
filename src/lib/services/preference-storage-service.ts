import { load, type Store } from '@tauri-apps/plugin-store';

import type { AppSettings } from '@/lib/models/settings';
import type { StorageScopePreferences } from '@/lib/models/storage-scope';
import type { CustomCleanupPreferences } from '@/lib/models/custom-cleanup';
import type { ScanExclusionPreferences } from '@/lib/models/storage-scan';

const SETTINGS_FILE_NAME = 'settings.json';
const SETTINGS_KEYS = {
  settings: 'settings',
  storageScopePreferences: 'storageScopePreferences',
  customCleanupPreferences: 'customCleanupPreferences',
  scanExclusionPreferences: 'scanExclusionPreferences',
  /** Read only during one-way migration to scanExclusionPreferences. */
  storageScanPreferences: 'storageScanPreferences',
  largeFilePreferences: 'largeFilePreferences',
} as const;

type SettingsKey = (typeof SETTINGS_KEYS)[keyof typeof SETTINGS_KEYS];

/**
 * Owns the single frontend settings document.
 *
 * Values remain unknown on reads so their owning domain parser validates every
 * persisted value before it enters application state.
 */
export class PreferenceStorageService {
  private static storePromise: Promise<Store> | null = null;
  private static mutationQueue: Promise<void> = Promise.resolve();

  static loadSettings(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.settings);
  }

  static saveSettings(settings: AppSettings): Promise<void> {
    return this.write(SETTINGS_KEYS.settings, settings);
  }

  static clearSettings(): Promise<void> {
    return this.remove(SETTINGS_KEYS.settings);
  }

  static loadStorageScopePreferences(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.storageScopePreferences);
  }

  static saveStorageScopePreferences(preferences: StorageScopePreferences): Promise<void> {
    return this.write(SETTINGS_KEYS.storageScopePreferences, preferences);
  }

  static clearStorageScopePreferences(): Promise<void> {
    return this.remove(SETTINGS_KEYS.storageScopePreferences);
  }

  static loadCustomCleanupPreferences(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.customCleanupPreferences);
  }

  static saveCustomCleanupPreferences(preferences: CustomCleanupPreferences): Promise<void> {
    return this.write(SETTINGS_KEYS.customCleanupPreferences, preferences);
  }

  static clearCustomCleanupPreferences(): Promise<void> {
    return this.remove(SETTINGS_KEYS.customCleanupPreferences);
  }

  static loadScanExclusionPreferences(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.scanExclusionPreferences);
  }

  static saveScanExclusionPreferences(preferences: ScanExclusionPreferences): Promise<void> {
    return this.write(SETTINGS_KEYS.scanExclusionPreferences, preferences);
  }

  static loadLegacyStorageScanPreferences(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.storageScanPreferences);
  }

  static loadLegacyLargeFilePreferences(): Promise<unknown | null> {
    return this.read(SETTINGS_KEYS.largeFilePreferences);
  }

  /** Publishes the scoped document and removes older keys in one serialized save. */
  static migrateScanExclusionPreferences(preferences: ScanExclusionPreferences): Promise<void> {
    return this.enqueueMutation(async store => {
      await store.set(SETTINGS_KEYS.scanExclusionPreferences, preferences);
      await store.delete(SETTINGS_KEYS.storageScanPreferences);
      await store.delete(SETTINGS_KEYS.largeFilePreferences);
      await store.save();
    });
  }

  static clearScanExclusionPreferences(): Promise<void> {
    return this.remove(SETTINGS_KEYS.scanExclusionPreferences);
  }

  private static store(): Promise<Store> {
    this.storePromise ??= load(SETTINGS_FILE_NAME, { autoSave: false }).catch(error => {
      this.storePromise = null;
      throw error;
    });
    return this.storePromise;
  }

  private static async read(key: SettingsKey): Promise<unknown | null> {
    await this.mutationQueue.catch(() => undefined);
    return (await this.store()).get<unknown>(key).then(value => value ?? null);
  }

  private static write(key: SettingsKey, value: unknown): Promise<void> {
    return this.enqueueMutation(async store => {
      await store.set(key, value);
      await store.save();
    });
  }

  private static remove(key: SettingsKey): Promise<void> {
    return this.enqueueMutation(async store => {
      await store.delete(key);
      await store.save();
    });
  }

  private static enqueueMutation(operation: (store: Store) => Promise<void>): Promise<void> {
    const mutation = this.mutationQueue.catch(() => undefined).then(async () => operation(await this.store()));
    this.mutationQueue = mutation;
    return mutation;
  }
}
