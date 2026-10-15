// @vitest-environment happy-dom

import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';
import { i18n } from '@/i18n';
import { OperatingSystemService } from '@/lib/services/operating-system-service';
import MdSplitActionButton from '@/components/custom/md-split-action-button.vue';
import MdAnalysisScanButton from './md-analysis-scan-button.vue';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos' }));

describe('analysis scan button', () => {
  it.each(['macos', 'linux'])('shows one allocation action on %s even with stale fast state', async () => {
    vi.spyOn(OperatingSystemService, 'isWindows').mockReturnValue(false);
    const wrapper = mount(MdAnalysisScanButton, {
      props: { mode: 'fast', disabled: false },
      global: { plugins: [i18n] },
    });
    try {
      expect(wrapper.findAll('button')).toHaveLength(1);
      await wrapper.get('button').trigger('click');
      expect(wrapper.emitted('scan')).toEqual([['standard']]);
    } finally {
      wrapper.unmount();
      vi.restoreAllMocks();
    }
  });

  it('preserves Windows mode selection and the primary action', async () => {
    vi.spyOn(OperatingSystemService, 'isWindows').mockReturnValue(true);
    const wrapper = mount(MdAnalysisScanButton, {
      props: { mode: 'fast', disabled: false },
      global: { plugins: [i18n] },
    });
    try {
      expect(wrapper.findAll('button')).toHaveLength(2);
      await wrapper.findAll('button')[0].trigger('click');
      wrapper.getComponent(MdSplitActionButton).vm.$emit('select', 'standard');
      expect(wrapper.emitted('scan')).toEqual([['fast'], ['standard']]);
    } finally {
      wrapper.unmount();
      vi.restoreAllMocks();
    }
  });
});
