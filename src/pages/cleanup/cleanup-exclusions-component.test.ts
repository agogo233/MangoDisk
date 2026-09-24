// @vitest-environment happy-dom

import { flushPromises, shallowMount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import MdScanExclusionLink from '@/components/custom/md-scan-exclusion-link.vue';
import { i18n } from '@/i18n';
import type { PresentedCleanupScanResult } from '@/lib/models/cleanup';
import { useCleanupStore } from '@/stores/cleanup-store';
import { useCustomCleanupStore } from '@/stores/custom-cleanup-store';

import CleanupPage from './index.vue';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos' }));

const scan: PresentedCleanupScanResult = {
  schemaVersion: '1.9',
  customScanId: null,
  scannedAtMs: 1,
  disk: { name: 'Fixture', mountPoint: '/scan', totalBytes: 100, usedBytes: 50, availableBytes: 50 },
  rules: [],
  applicationIcons: [],
  warningCount: 0,
  safeBytes: 0,
  reclaimableBytes: 0,
  applicabilityElapsedMs: 0,
  applicableRuleCount: 0,
  filteredRuleCount: 0,
  inventoryApplicationCount: 0,
  inventoryProcessCount: 0,
  elapsedMs: 0,
};

beforeEach(() => {
  setActivePinia(createPinia());
  useCustomCleanupStore().initialized = true;
});

describe('deep cleanup exclusions', () => {
  it('shows the scan snapshot exclusion link and opens its settings', async () => {
    // Resolve the page's async result component before the test environment is torn down.
    await import('./components/md-cleanup-rule-groups.vue');
    const store = useCleanupStore();
    const wrapper = shallowMount(CleanupPage, {
      props: {
        busy: false,
        disk: scan.disk,
        disks: [],
        leftovers: null,
        leftoverResult: null,
        scanningLeftovers: false,
        deletingLeftovers: false,
        loadingMessage: '',
        operation: 'idle',
        progress: null,
        result: null,
        scan,
        scanScope: { mode: 'standard' },
        selectedBytes: 0,
        selectedRuleIds: [],
        sourceSelections: [],
        closingApplications: false,
        closeResult: null,
        privilegedScanRuleId: null,
      },
      global: {
        plugins: [i18n],
        stubs: {
          MdPageShell: { template: '<div><slot /></div>' },
          MdResultWorkspace: { template: '<div><slot name="summary" /><slot /></div>' },
          MdResultSummary: { template: '<div><slot name="status" /></div>' },
        },
      },
    });

    expect(wrapper.findComponent(MdScanExclusionLink).exists()).toBe(false);
    store.scanExcludedFolders = ['/scan/private'];
    await flushPromises();

    wrapper.getComponent(MdScanExclusionLink).vm.$emit('open');
    expect(wrapper.emitted('openExclusions')).toHaveLength(1);

    await wrapper.setProps({ scan: null });
    expect(wrapper.findComponent(MdScanExclusionLink).exists()).toBe(false);
    wrapper.unmount();
  });
});
