// @vitest-environment happy-dom

import { flushPromises, mount, shallowMount } from '@vue/test-utils';
import { defineComponent, h } from 'vue';
import { AnalysisService } from '@/lib/services/analysis-service';
import { useAnalysisStore } from '@/stores/analysis-store';
import { useStorageScanPreferencesStore } from '@/stores/storage-scan-preferences-store';
import MdAnalysisDetailsTable from './components/md-analysis-details-table.vue';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import MdDelayedOperationWorkspace from '@/components/custom/md-delayed-operation-workspace.vue';
import MdAnalysisBrowserToolbar from './components/md-analysis-browser-toolbar.vue';
import MdAnalysisFolderPane from './components/md-analysis-folder-pane.vue';
import MdAnalysisVisualPane from './components/md-analysis-visual-pane.vue';
import { Button } from '@/components/ui/button';
import { i18n } from '@/i18n';
import type { AnalysisResult } from '@/lib/models/analysis';

import AnalysisPage from './index.vue';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos' }));

const result: AnalysisResult = {
  scanId: 7,
  root: '/fixture',
  scannedAtMs: 1,
  totalBytes: 64,
  skippedCount: 0,
  truncated: false,
  entries: [],
};

beforeEach(() => {
  setActivePinia(createPinia());
  vi.restoreAllMocks();
});

describe('analysis page', () => {
  it('preserves details sorting while an unvisited child is resolved from the backend cache', async () => {
    const store = useAnalysisStore();
    store.result = result;
    useStorageScanPreferencesStore().initialized = true;
    const unlisten = vi.fn();
    vi.spyOn(AnalysisService, 'listenProgress').mockResolvedValue(unlisten);
    let complete: (value: AnalysisResult) => void = () => undefined;
    const analyze = vi.spyOn(AnalysisService, 'analyze').mockImplementation(
      () =>
        new Promise(resolve => {
          complete = resolve;
        })
    );
    const wrapper = mount(
      defineComponent({
        setup: () => () =>
          h(AnalysisPage, {
            result: store.result,
            excludedFolders: [],
            homePath: '/fixture',
            disk: null,
            disks: [],
            progress: store.progress,
            busy: store.pending,
            cancelling: store.cancelling,
            deleting: false,
          }),
      }),
      {
        global: {
          plugins: [i18n],
          stubs: {
            MdPageShell: { template: '<div><slot /></div>' },
            MdAnalysisBrowserToolbar: true,
            MdAnalysisFolderPane: true,
            MdAnalysisTreemap: true,
            MdDestructiveActionDialog: true,
            MdTooltip: { template: '<span><slot /></span>' },
          },
        },
      }
    );
    try {
      wrapper.getComponent(MdAnalysisVisualPane).vm.$emit('update:viewMode', 'details');
      await flushPromises();
      const table = wrapper.getComponent(MdAnalysisDetailsTable);
      const nameSort = '.details-head-grid button';
      await table.get(nameSort).trigger('click');
      expect(table.get(nameSort).attributes('data-active')).toBe('true');
      const request = store.analyze('/fixture/child');
      await flushPromises();
      expect(analyze).toHaveBeenCalledOnce();
      expect(store.scanStarted).toBe(true);
      expect(store.progress).toBeNull();
      const retainedWhilePending = wrapper.findComponent(MdAnalysisDetailsTable).exists();
      const fullProgressWhilePending = wrapper.find('.analysis-overlay--full').exists();
      complete({ ...result, root: '/fixture/child' });
      await request;
      await flushPromises();
      expect(unlisten).toHaveBeenCalledOnce();
      expect.soft(retainedWhilePending).toBe(true);
      expect.soft(fullProgressWhilePending).toBe(false);
      expect.soft(wrapper.getComponent(MdAnalysisDetailsTable).vm.$.uid).toBe(table.vm.$.uid);
      expect(wrapper.get(nameSort).attributes('data-active')).toBe('true');
    } finally {
      wrapper.unmount();
    }
  });

  it('exposes the entry limit help only for a truncated result', async () => {
    const wrapper = shallowMount(AnalysisPage, {
      props: {
        result,
        excludedFolders: [],
        homePath: '/fixture',
        disk: null,
        disks: [],
        progress: null,
        busy: false,
        cancelling: false,
        deleting: false,
      },
      global: {
        plugins: [i18n],
        stubs: { MdPageShell: { template: '<div><slot /></div>' } },
      },
    });
    expect(wrapper.getComponent(MdAnalysisFolderPane).props('truncated')).toBe(false);
    await wrapper.setProps({ result: { ...result, truncated: true } });
    expect(wrapper.getComponent(MdAnalysisFolderPane).props('truncated')).toBe(true);
  });

  it('replaces stale browser data immediately during a primary rescan', async () => {
    const wrapper = shallowMount(AnalysisPage, {
      props: {
        result,
        excludedFolders: [],
        homePath: '/fixture',
        disk: null,
        disks: [],
        progress: null,
        busy: false,
        cancelling: false,
        deleting: false,
      },
      global: {
        plugins: [i18n],
        stubs: {
          MdPageShell: { template: '<div><slot name="actions" /><slot /></div>' },
        },
      },
    });

    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(true);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(true);

    wrapper.findComponent(Button).vm.$emit('click');
    await wrapper.setProps({ busy: true });

    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(false);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(false);
    const progress = wrapper.getComponent(MdDelayedOperationWorkspace);
    expect(progress.props('delay')).toBe(0);
    expect(progress.classes()).toContain('analysis-overlay--full');

    await wrapper.setProps({ busy: false });
    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(true);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(true);
  });

  it('hides stale path and results when parent navigation starts a real scan', async () => {
    const wrapper = shallowMount(AnalysisPage, {
      props: {
        result,
        excludedFolders: [],
        homePath: '/fixture',
        disk: null,
        disks: [],
        progress: null,
        busy: false,
        cancelling: false,
        deleting: false,
      },
      global: {
        plugins: [i18n],
        stubs: { MdPageShell: { template: '<div><slot /></div>' } },
      },
    });

    wrapper.getComponent(MdAnalysisBrowserToolbar).vm.$emit('navigate', '/');
    expect(wrapper.emitted('analyze')?.at(-1)).toEqual(['/', false, false]);
    await wrapper.setProps({ busy: true });
    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(true);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(true);
    const waitingOverlay = wrapper.getComponent(MdDelayedOperationWorkspace).vm.$.uid;

    await wrapper.setProps({
      progress: {
        operationId: 8,
        currentStage: 'analyzing',
        currentPath: '/',
        itemsScanned: 0,
        bytesScanned: 0,
        completedSteps: 0,
        totalSteps: 0,
        foundItems: 0,
        foundBytes: 0,
        elapsedMs: 0,
      },
    });
    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(false);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(false);
    const progress = wrapper.getComponent(MdDelayedOperationWorkspace);
    expect(progress.vm.$.uid).not.toBe(waitingOverlay);
    expect(progress.classes()).toContain('analysis-overlay--full');
    expect(progress.props('delay')).toBe(0);

    await wrapper.setProps({ busy: false, progress: null });
    expect(wrapper.findComponent(MdAnalysisBrowserToolbar).exists()).toBe(true);
    expect(wrapper.findComponent(MdAnalysisVisualPane).exists()).toBe(true);
  });
});
