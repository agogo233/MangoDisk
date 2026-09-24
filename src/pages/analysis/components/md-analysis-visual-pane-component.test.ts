// @vitest-environment happy-dom

import { mount } from '@vue/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { i18n } from '@/i18n';
import type { AnalysisResult } from '@/lib/models/analysis';
import { ByteSizeService } from '@/lib/services/byte-size-service';

import MdAnalysisVisualPane from './md-analysis-visual-pane.vue';

const result: AnalysisResult = {
  scanId: 7,
  root: '/fixture',
  scannedAtMs: 1_000,
  totalBytes: 64,
  skippedCount: 0,
  entries: [],
};

function mountPane(exclusionsActive: boolean) {
  return mount(MdAnalysisVisualPane, {
    props: {
      result,
      entries: [],
      folderCount: 0,
      viewMode: 'treemap',
      exclusionsActive,
      openDisabled: false,
      deleteDisabled: false,
    },
    global: {
      plugins: [i18n],
      stubs: {
        MdAnalysisTreemap: true,
        MdAnalysisDetailsTable: true,
        MdIcon: true,
        MdTooltip: { template: '<span><slot /></span>' },
      },
    },
  });
}

describe('analysis result exclusions', () => {
  beforeEach(() => {
    vi.spyOn(ByteSizeService, 'bytes').mockReturnValue('64 B');
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('keeps the result toolbar clear when analysis includes all folders', () => {
    expect(mountPane(false).find('.exclusion-link').exists()).toBe(false);
  });

  it('explains a filtered result without changing the reported total', () => {
    const wrapper = mountPane(true);

    expect(wrapper.get('.exclusion-link').text()).toBe('Exclusions on');
    expect(wrapper.get('.space-summary').text()).toContain('64 B');
  });

  it('opens the shared exclusion settings from the filtered result hint', async () => {
    const wrapper = mountPane(true);

    expect(wrapper.get('.exclusion-link').element.tagName).toBe('BUTTON');
    await wrapper.get('.exclusion-link').trigger('click');

    expect(wrapper.emitted('openExclusions')).toHaveLength(1);
  });
});
