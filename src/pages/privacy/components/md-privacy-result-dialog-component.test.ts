// @vitest-environment happy-dom

import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';

import { i18n } from '@/i18n';
import type { PrivacyExecutionPlan, PrivacyExecutionResult } from '@/lib/models/privacy/privacy';

import MdPrivacyResultDialog from './md-privacy-result-dialog.vue';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos' }));

const passthroughStub = { template: '<div><slot /></div>' };
const dialogContentStub = {
  props: { height: { type: String, default: 'auto' } },
  template: '<section class="dialog-content-stub" :data-dialog-height="height"><slot /></section>',
};

function fixture(itemCount: number): { plan: PrivacyExecutionPlan; result: PrivacyExecutionResult } {
  const planItems = Array.from({ length: itemCount }, (_, index) => ({
    token: `token-${index}`,
    sourceId: 'browser',
    sourceName: 'Browser',
    profileName: 'Default',
    kind: 'browsingHistory' as const,
    impact: 'low' as const,
    itemCount: 1,
    estimatedBytes: 10,
    requiresBrowserClose: false,
    synchronizationMayPropagate: false,
  }));
  return {
    plan: {
      schemaVersion: 2,
      planId: 'plan-1',
      scanId: 'scan-1',
      createdAtMs: 1,
      expiresAtMs: 2,
      items: planItems,
      browserCloseRequirements: [],
      requiresConfirmation: true,
      requiresBrowserClose: false,
    },
    result: {
      planId: 'plan-1',
      affectedItemCount: itemCount,
      failedItemCount: 0,
      scan: null,
      items: planItems.map(item => ({
        token: item.token,
        status: 'cleared' as const,
        affectedItemCount: 1,
        verified: true,
        failureReason: null,
      })),
    },
  };
}

function mountDialog(itemCount: number, data = fixture(itemCount)) {
  return mount(MdPrivacyResultDialog, {
    props: { modelValue: true, ...data },
    global: {
      plugins: [i18n],
      stubs: {
        Button: passthroughStub,
        Dialog: passthroughStub,
        DialogDescription: passthroughStub,
        DialogTitle: passthroughStub,
        MdDialogContent: dialogContentStub,
        MdDialogFooter: passthroughStub,
        MdDialogHeader: passthroughStub,
        MdIcon: true,
      },
    },
  });
}

describe('privacy result dialog component', () => {
  it('explains a blocked plan and names the changed source without implying cleanup', () => {
    const data = fixture(2);
    data.plan.items[1]!.kind = 'jumpLists';
    data.result.affectedItemCount = 0;
    data.result.failedItemCount = 1;
    data.result.items[0] = {
      ...data.result.items[0]!,
      status: 'cancelled',
      affectedItemCount: 0,
      verified: false,
      failureReason: 'preflightBlocked',
    };
    data.result.items[1] = {
      ...data.result.items[1]!,
      status: 'failed',
      affectedItemCount: 0,
      verified: false,
      failureReason: 'sourceChanged',
    };
    const wrapper = mountDialog(2, data);
    expect(wrapper.text()).toContain('Nothing cleared');
    expect(wrapper.text()).toContain('Application jump lists');
    expect(wrapper.text()).toContain('another item failed safety checks');
    expect(wrapper.text()).toContain('Data changed');
    expect(wrapper.text()).not.toContain('Could not complete');
    wrapper.unmount();
  });

  it('keeps partial cleanup distinct and handles unknown reasons safely', () => {
    const data = fixture(2);
    data.result.items[1] = {
      ...data.result.items[1]!,
      status: 'failed',
      verified: false,
      failureReason: 'futureReason',
    };
    const wrapper = mountDialog(2, data);
    expect(wrapper.text()).not.toContain('Nothing cleared');
    expect(wrapper.text()).toContain('Check the log');
    expect(wrapper.text()).not.toContain('futureReason');
    wrapper.unmount();
  });

  it('uses the shared compact result structure without a rescan action', () => {
    const wrapper = mountDialog(1);

    expect(wrapper.findAll('.operation-result-stats > span')).toHaveLength(2);
    expect(wrapper.findAll('.operation-result-items > div')).toHaveLength(1);
    expect(wrapper.text()).not.toContain('重新扫描');
    expect(wrapper.get('.dialog-content-stub').attributes('data-dialog-height')).toBe('auto');
    wrapper.unmount();
  });

  it('bounds long result lists to the shared tall dialog layout', () => {
    const wrapper = mountDialog(8);

    expect(wrapper.get('.dialog-content-stub').attributes('data-dialog-height')).toBe('tall');
    wrapper.unmount();
  });
});
