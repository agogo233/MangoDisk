// @vitest-environment happy-dom

import { flushPromises, shallowMount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, expect, it, vi } from 'vitest';
import { Select } from '@/components/ui/select';
import { COLOR_THEME_IDS, THEME_IDS } from '@/lib/models/settings/settings';
import { i18n } from '@/i18n';
import { MacOsPermissionService } from '@/lib/services/settings/macos-permission-service';
import { OperatingSystemService } from '@/lib/services/settings/operating-system-service';
import { PreferenceStorageService } from '@/lib/services/settings/preference-storage-service';
import * as AppSettingsUtils from '@/lib/utils/settings/app-settings';
import SettingsPage from './index.vue';

afterEach(() => vi.restoreAllMocks());

it('saves a palette choice independently and rejects unknown selections', async () => {
  setActivePinia(createPinia());
  vi.spyOn(MacOsPermissionService, 'isMacOs').mockReturnValue(false);
  vi.spyOn(OperatingSystemService, 'isLinux').mockReturnValue(false);
  vi.spyOn(OperatingSystemService, 'isWindows').mockReturnValue(false);
  vi.spyOn(PreferenceStorageService, 'loadScanExclusionPreferences').mockResolvedValue({
    schemaVersion: 3,
    folders: [],
    names: [],
  });
  const settings = { ...AppSettingsUtils.defaults(), theme: THEME_IDS.dark };
  const wrapper = shallowMount(SettingsPage, {
    props: { settings, focusRevision: 0 },
    global: {
      plugins: [i18n],
      stubs: {
        MdPageShell: { template: '<div><slot /></div>' },
        MdSettingsGroup: { template: '<div><slot /></div>' },
        MdSettingsRow: { template: '<div><slot /></div>' },
      },
    },
  });
  const palette = wrapper
    .findAllComponents(Select)
    .find(select => select.props('modelValue') === COLOR_THEME_IDS.mango);
  expect(palette).toBeDefined();
  palette!.vm.$emit('update:modelValue', COLOR_THEME_IDS.warmGray);
  await flushPromises();
  expect(wrapper.emitted('save')).toEqual([[{ ...settings, colorTheme: COLOR_THEME_IDS.warmGray }]]);
  palette!.vm.$emit('update:modelValue', 'unsupported');
  await flushPromises();
  expect(wrapper.emitted('save')).toHaveLength(1);
  wrapper.unmount();
});
