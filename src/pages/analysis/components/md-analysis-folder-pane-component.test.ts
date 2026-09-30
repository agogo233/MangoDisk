// @vitest-environment happy-dom

import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';

import { i18n } from '@/i18n';

import MdAnalysisFolderPane from './md-analysis-folder-pane.vue';

describe('analysis folder pane', () => {
  it('places the limit help immediately after the visible counts', async () => {
    const wrapper = mount(MdAnalysisFolderPane, {
      props: {
        entries: [],
        totalBytes: 0,
        folderCount: 100,
        fileCount: 4997,
        truncated: false,
        openDisabled: false,
        deleteDisabled: false,
      },
      global: {
        plugins: [i18n],
        stubs: {
          MdIcon: true,
          MdTooltip: { template: '<span><slot /></span>', props: ['text'] },
        },
      },
    });

    expect(wrapper.find('.md-help-action').exists()).toBe(false);
    await wrapper.setProps({ truncated: true });
    const header = wrapper.get('header');
    expect(header.get('p').text()).toBe('100 folders · 4,997 files');
    expect(header.get('.md-help-action').attributes('aria-label')).toBe('Showing up to 100 largest items');
    expect(header.element.children[1]?.contains(header.get('.md-help-action').element)).toBe(true);
  });
});
