// @vitest-environment happy-dom
import { flushPromises, shallowMount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { createI18n } from 'vue-i18n';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ResidentPreferences } from '@/lib/models/resident/resident';
import { ResidentService } from '@/lib/services/resident/resident-service';
import { useResidentSettingsStore } from '@/stores/resident-settings-store';
import { preferencesFixture } from '@/tests/fixtures/resident';
import Settings from './md-status-display-settings.vue';

vi.mock('@/lib/services/resident/resident-service', () => ({
  ResidentService: { preferences: vi.fn(), savePreferences: vi.fn(), onPreferencesChanged: vi.fn() },
}));
vi.mock('@/lib/services/diagnostics/logger-service', () => ({
  LoggerService: { warn: vi.fn(), info: vi.fn() },
}));

function deferredPreferences() {
  let resolve!: (value: ResidentPreferences) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<ResidentPreferences>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let notify: () => void;
const stop = vi.fn();
const wrappers: ReturnType<typeof shallowMount>[] = [];
function mountSettings() {
  const pinia = createPinia();
  const wrapper = shallowMount(Settings, {
    props: { isMacOs: false },
    global: {
      plugins: [
        pinia,
        createI18n({ legacy: false, locale: 'en', messages: {}, missingWarn: false, fallbackWarn: false }),
      ],
    },
  });
  wrappers.push(wrapper);
  return { wrapper, store: useResidentSettingsStore(pinia) };
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(ResidentService.preferences).mockResolvedValue(preferencesFixture());
  vi.mocked(ResidentService.onPreferencesChanged).mockImplementation(async callback => {
    notify = callback;
    return stop;
  });
});
afterEach(() => wrappers.splice(0).forEach(wrapper => wrapper.unmount()));

describe('native display preference synchronization', () => {
  it('reloads a notification received while the initial read is pending', async () => {
    const pending = deferredPreferences();
    const latest = { ...preferencesFixture(), revision: 1, showIcon: false };
    vi.mocked(ResidentService.preferences).mockReturnValueOnce(pending.promise).mockResolvedValueOnce(latest);
    const { store } = mountSettings();
    await flushPromises();
    expect(store.loading).toBe(true);
    notify();
    pending.resolve(preferencesFixture());
    await flushPromises();
    expect(ResidentService.preferences).toHaveBeenCalledTimes(2);
    expect(store.preferences).toEqual(latest);
  });

  it('coalesces native changes during a save and reads the newer committed state', async () => {
    const { store } = mountSettings();
    await flushPromises();
    const pending = deferredPreferences();
    vi.mocked(ResidentService.savePreferences).mockReturnValueOnce(pending.promise);
    const saved = { ...preferencesFixture(), revision: 1, showIcon: false };
    const latest: ResidentPreferences = { ...saved, revision: 2, floating: { ...saved.floating, snapToEdges: false } };
    vi.mocked(ResidentService.preferences).mockResolvedValueOnce(latest);
    const saving = store.change({ showIcon: false });
    expect(store.saving).toBe(true);
    notify();
    notify();
    notify();
    pending.resolve(saved);
    await saving;
    await flushPromises();
    expect(ResidentService.preferences).toHaveBeenCalledTimes(2);
    expect(store.preferences).toEqual(latest);
    expect(store.draft).toEqual(latest);
  });

  it('does not retry a failed deferred refresh without a new notification', async () => {
    const pending = deferredPreferences();
    vi.mocked(ResidentService.preferences)
      .mockReturnValueOnce(pending.promise)
      .mockRejectedValueOnce(new Error('injected preference read failure'));
    const { store } = mountSettings();
    await flushPromises();
    notify();
    pending.resolve(preferencesFixture());
    await flushPromises();
    await flushPromises();
    expect(ResidentService.preferences).toHaveBeenCalledTimes(2);
    expect(store.error).toBe(true);
    expect(store.loading).toBe(false);
    notify();
    await flushPromises();
    expect(ResidentService.preferences).toHaveBeenCalledTimes(3);
    expect(store.error).toBe(false);
  });

  it('discards pending refreshes when the settings page is unmounted', async () => {
    const pending = deferredPreferences();
    vi.mocked(ResidentService.preferences).mockReturnValueOnce(pending.promise);
    const { wrapper } = mountSettings();
    await flushPromises();
    notify();
    wrapper.unmount();
    pending.resolve(preferencesFixture());
    await flushPromises();
    expect(stop).toHaveBeenCalledOnce();
    expect(ResidentService.preferences).toHaveBeenCalledOnce();
  });

  it('establishes the event subscription before reading the initial snapshot', async () => {
    let subscribe!: () => void;
    vi.mocked(ResidentService.onPreferencesChanged).mockReturnValueOnce(
      new Promise(resolve => {
        subscribe = () => resolve(stop);
      })
    );
    mountSettings();
    await flushPromises();
    expect(ResidentService.preferences).not.toHaveBeenCalled();
    subscribe();
    await flushPromises();
    expect(ResidentService.preferences).toHaveBeenCalledOnce();
  });

  it('still loads preferences when event registration fails', async () => {
    vi.mocked(ResidentService.onPreferencesChanged).mockRejectedValueOnce(new Error('injected subscription failure'));
    const { store } = mountSettings();
    await flushPromises();
    expect(store.preferences).toEqual(preferencesFixture());
  });
});
