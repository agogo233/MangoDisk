import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnalysisService } from '@/lib/services/analysis-service';
import { LoggerService } from '@/lib/services/logger-service';
import { OperatingSystemService } from '@/lib/services/operating-system-service';
import { PreferenceStorageService } from '@/lib/services/preference-storage-service';
import { useAnalysisStore } from './analysis-store';
import { useStorageScanPreferencesStore } from './storage-scan-preferences-store';

const { values } = vi.hoisted(() => ({ values: new Map<string, unknown>() }));
vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn(async () => ({
    get: async (key: string) => values.get(key),
    set: async (key: string, value: unknown) => {
      values.set(key, value);
    },
    save: async () => undefined,
  })),
}));

describe('analysis scan preference lifecycle', () => {
  beforeEach(() => {
    values.clear();
    setActivePinia(createPinia());
    useStorageScanPreferencesStore().initialized = true;
    vi.spyOn(OperatingSystemService, 'isWindows').mockReturnValue(true);
    vi.spyOn(LoggerService, 'warn').mockImplementation(() => undefined);
    vi.spyOn(AnalysisService, 'listenProgress').mockResolvedValue(vi.fn());
    vi.spyOn(AnalysisService, 'analyze').mockResolvedValue({
      scanId: 7,
      root: '/fixture',
      scannedAtMs: 1,
      totalBytes: 64,
      skippedCount: 0,
      truncated: false,
      entries: [],
      scanMode: 'fast',
    });
  });
  afterEach(() => vi.restoreAllMocks());

  it('persists the explicitly selected scan mode', async () => {
    await useAnalysisStore().analyze('/fixture', true, true, 'fast');
    await PreferenceStorageService.loadSettings();
    expect(values.get('analysisScanMode')).toBe('fast');
  });

  it('restores fast in a fresh session and saves a later standard selection', async () => {
    await useAnalysisStore().analyze('/fixture', true, true, 'fast');
    await PreferenceStorageService.loadSettings();
    setActivePinia(createPinia());
    useStorageScanPreferencesStore().initialized = true;
    const restarted = useAnalysisStore();
    await Promise.all([restarted.initializeScanMode(), restarted.initializeScanMode()]);
    expect(restarted.scanMode).toBe('fast');
    await restarted.analyze('/fixture');
    expect(AnalysisService.analyze).toHaveBeenLastCalledWith('/fixture', false, [], [], 'fast');
    await restarted.analyze('/fixture', true, true, 'standard');
    await PreferenceStorageService.loadSettings();
    expect(values.get('analysisScanMode')).toBe('standard');
  });

  it.each([null, 'quick', { mode: 'fast' }, 1])(
    'defaults to standard for an absent or invalid saved value %j',
    async saved => {
      values.set('analysisScanMode', saved);
      const store = useAnalysisStore();
      await store.initializeScanMode();
      expect(store.scanMode).toBe('standard');
      expect(store.scanModeInitialized).toBe(true);
    }
  );

  it('preserves a scan selection while a startup read is delayed', async () => {
    let finish: (value: unknown) => void = () => undefined;
    vi.spyOn(PreferenceStorageService, 'loadAnalysisScanMode').mockImplementation(
      () =>
        new Promise(resolve => {
          finish = resolve;
        })
    );
    const store = useAnalysisStore();
    const loading = store.initializeScanMode();
    await store.analyze('/fixture', true, true, 'fast');
    finish('standard');
    await loading;
    await PreferenceStorageService.loadSettings();
    expect(store.scanMode).toBe('fast');
    expect(values.get('analysisScanMode')).toBe('fast');
  });

  it.each(['macos', 'linux'])('does not restore or persist Windows modes on %s', async () => {
    vi.mocked(OperatingSystemService.isWindows).mockReturnValue(false);
    values.set('analysisScanMode', 'fast');
    const load = vi.spyOn(PreferenceStorageService, 'loadAnalysisScanMode');
    const save = vi.spyOn(PreferenceStorageService, 'saveAnalysisScanMode');
    const store = useAnalysisStore();
    await store.initializeScanMode();
    await store.analyze('/fixture', true, true, 'fast');
    expect(store.scanMode).toBe('standard');
    expect(load).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    expect(values.get('analysisScanMode')).toBe('fast');
  });

  it('logs storage errors while keeping scanning usable', async () => {
    vi.spyOn(PreferenceStorageService, 'loadAnalysisScanMode').mockRejectedValue(new Error('read failed'));
    vi.spyOn(PreferenceStorageService, 'saveAnalysisScanMode').mockRejectedValue(new Error('write failed'));
    const store = useAnalysisStore();
    await store.initializeScanMode();
    await store.analyze('/fixture', true, true, 'fast');
    await Promise.resolve();
    expect(store.scanMode).toBe('fast');
    expect(LoggerService.warn).toHaveBeenCalledTimes(2);
  });
});
