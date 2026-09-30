// @vitest-environment happy-dom

import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';

import { i18n } from '@/i18n';

import MdIncompleteScanGuidance from './md-incomplete-scan-guidance.vue';

const passthroughStub = { template: '<div><slot /></div>' };
const dialogStub = {
  props: ['open'],
  emits: ['update:open'],
  template: '<div v-if="open" class="dialog-stub"><slot /></div>',
};
const buttonStub = {
  props: ['disabled'],
  template: '<button type="button" :disabled="disabled"><slot /></button>',
};

function mountGuidance() {
  const wrapper = mount(MdIncompleteScanGuidance, {
    props: {
      modelValue: false,
      failureCount: 2,
      retryDisabled: false,
    },
    global: {
      plugins: [i18n],
      stubs: {
        Button: buttonStub,
        Dialog: dialogStub,
        DialogDescription: passthroughStub,
        DialogTitle: passthroughStub,
        MdDialogContent: passthroughStub,
        MdDialogFooter: passthroughStub,
        MdDialogHeader: passthroughStub,
        MdIcon: passthroughStub,
        MdInlineNotice: passthroughStub,
      },
    },
  });
  return { wrapper };
}

describe('incomplete cleanup scan guidance', () => {
  it('opens details from the summary and offers a retry', async () => {
    const { wrapper } = mountGuidance();

    await wrapper.get('.scan-warning-trigger').trigger('click');
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual([true]);
    await wrapper.setProps({ modelValue: true });
    expect(wrapper.get('.dialog-stub').text()).toContain(
      i18n.global.t('cleanup.permission.otherDescription', { count: 2 })
    );
    expect(wrapper.get('.dialog-stub').text()).not.toContain(i18n.global.t('fullDiskAccessGuidance.openSettings'));

    const buttons = wrapper.findAll('.dialog-stub button');
    expect(buttons[0]?.text()).toBe(i18n.global.t('settings.feedbackDialog.openLogFolder'));
    await buttons[0]!.trigger('click');
    expect(wrapper.emitted('openLogs')).toHaveLength(1);
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual([true]);

    await buttons[1]!.trigger('click');
    expect(wrapper.emitted('retry')).toHaveLength(1);
    expect(wrapper.emitted('update:modelValue')?.at(-1)).toEqual([false]);
    wrapper.unmount();
  });

  it('keeps retry disabled while another operation is running', async () => {
    const { wrapper } = mountGuidance();
    await wrapper.setProps({ modelValue: true, retryDisabled: true });
    const retryButton = wrapper.findAll('.dialog-stub button')[1]!;
    expect(retryButton.attributes('disabled')).toBeDefined();
    await retryButton.trigger('click');
    expect(wrapper.emitted('retry')).toBeUndefined();
    wrapper.unmount();
  });
});
