// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { createI18n } from 'vue-i18n';
import { expect, it, vi } from 'vitest';
import Row from './md-application-memory-row.vue';
import { useMemoryReleaseStore } from '@/stores/memory-release-store';
import { MemoryReleaseService } from '@/lib/services/memory-release-service';
import en from '@/locales/en-US.json';
vi.mock('@/lib/services/operating-system-service', () => ({ OperatingSystemService: { isWindows: () => true } }));
vi.mock('@/lib/services/memory-release-service', () => ({ MemoryReleaseService: { save: vi.fn() } }));
vi.mock('@/lib/services/byte-size-service', () => ({ ByteSizeService: { memory: (n: number) => `${n} B` } }));
it('excludes all matching image processes from the ranking without quitting or removing the row', async () => {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useMemoryReleaseStore();
  store.accept({
    schemaVersion: 1,
    revision: 0,
    automatic: false,
    intervalMinutes: 30,
    thresholdPercent: 80,
    skipForeground: true,
    exclusions: [],
  });
  vi.mocked(MemoryReleaseService.save).mockImplementation(async p => ({ ...p, revision: p.revision + 1 }));
  const page = mount(Row, {
    props: {
      application: {
        id: 'code',
        name: 'Code.exe',
        residentBytes: 100,
        processCount: 12,
        iconPath: 'C:\\Apps\\Code.exe',
        isBundle: false,
        canQuit: true,
      },
      expanded: true,
      share: 100,
    },
    global: {
      plugins: [pinia, createI18n({ legacy: false, locale: 'en', messages: { en } })],
      stubs: { MdNativeFileIcon: true, MdIcon: true },
    },
  });
  const action = page.get('button[aria-pressed]');
  await action.trigger('click');
  await flushPromises();
  expect(action.attributes('aria-pressed')).toBe('true');
  expect(page.get('.excluded-badge').text()).toBe('Excluded');
  expect(page.get('.application-name').text()).toBe('Code.exe');
  await action.trigger('click');
  await flushPromises();
  expect(page.find('.excluded-badge').exists()).toBe(false);
  page.unmount();
});
