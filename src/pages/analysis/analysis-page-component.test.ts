// @vitest-environment happy-dom

import { shallowMount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import MdDelayedOperationWorkspace from '@/components/custom/md-delayed-operation-workspace.vue';
import MdAnalysisBrowserToolbar from './components/md-analysis-browser-toolbar.vue';
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
  entries: [],
};

beforeEach(() => {
  setActivePinia(createPinia());
});

describe('analysis page', () => {
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
});
