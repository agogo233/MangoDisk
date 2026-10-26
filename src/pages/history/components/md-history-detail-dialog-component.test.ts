// @vitest-environment happy-dom
import { mount } from '@vue/test-utils';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));
import { i18n } from '@/i18n';
import type { PresentedOperationRecord } from '@/lib/models/history/history';
import MdHistoryDetailDialog from './md-history-detail-dialog.vue';

const passthrough = { template: '<div><slot /></div>' };
const base = {
  schemaVersion: 1,
  operationId: 'operation',
  startedAtMs: 0,
  finishedAtMs: 1000,
  outcome: 'completed' as const,
  dryRun: false,
  selectedItemCount: 6,
  affectedItemCount: 1,
  expectedBytes: 4096,
  releasedBytes: 0,
  releasedBytesIsEstimate: true,
  failedItemCount: 0,
};
function render(record: PresentedOperationRecord) {
  return mount(MdHistoryDetailDialog, {
    props: { open: true, selectedRecord: record },
    global: {
      plugins: [i18n],
      stubs: {
        Dialog: passthrough,
        DialogDescription: passthrough,
        DialogTitle: passthrough,
        MdDialogContent: { props: ['height'], template: '<div :data-height="height"><slot /></div>' },
        MdDialogFooter: passthrough,
        MdDialogHeader: passthrough,
        MdResultTable: { template: '<div><slot name="header"/><slot/></div>' },
        MdTooltip: passthrough,
      },
    },
  });
}

describe('history detail dialog', () => {
  it('describes cleared privacy data consistently with the Spanish cleanup result', () => {
    const previousLocale = i18n.global.locale.value;
    i18n.global.locale.value = 'es-ES';
    const wrapper = render({
      ...base,
      category: 'privacyCleanup',
      details: {
        type: 'privacyCleanup',
        payload: {
          planId: 'plan',
          timeRange: 'allTime',
          items: [
            { sourceId: 'browser', kind: 'cookies', affectedItemCount: 2, status: 'cleared', failureReason: null },
          ],
        },
      },
    });
    try {
      expect(wrapper.get('.detail-action small').text()).toBe(i18n.global.t('privacy.result.statuses.cleared'));
      expect(wrapper.get('.detail-action').text()).toContain('2');
      expect(wrapper.get('.action-status').classes()).not.toContain('warning');
    } finally {
      wrapper.unmount();
      i18n.global.locale.value = previousLocale;
    }
  });

  it('keeps cancelled uninstall space uncounted and prioritizes failed actions in its explanation', async () => {
    const wrapper = render({
      ...base,
      category: 'applicationUninstall',
      details: {
        type: 'applicationUninstall',
        payload: {
          batchId: 'batch',
          restartRequired: true,
          applications: [
            {
              restartRequired: true,
              planId: 'plan',
              applicationId: 'app',
              applicationName: 'Example app',
              applicationIdentifier: 'org.example.app',
              applicationVersion: null,
              applicationPublisher: null,
              applicationPlatform: 'macosBundle',
              installerKind: null,
              componentIds: ['binary', 'cache'],
              actions: [
                {
                  componentId: 'binary',
                  kind: 'applicationBinary',
                  status: 'cancelled',
                  reason: null,
                  expectedBytes: 3072,
                  releasedBytes: 0,
                },
                {
                  componentId: 'cache',
                  kind: 'cache',
                  status: 'failed',
                  reason: 'verificationFailed',
                  expectedBytes: 1024,
                  releasedBytes: 256,
                },
              ],
            },
          ],
        },
      },
    });
    expect(wrapper.text()).toContain('Example app');
    expect(wrapper.find('.detail-action').text()).toContain(i18n.global.t('history.notCounted'));
    expect(wrapper.find('.detail-action').text()).toContain(
      i18n.global.t('history.applicationUninstallReasons.verificationFailed')
    );
    expect(wrapper.find('[data-height]').attributes('data-height')).toBe('tall');
    await wrapper.get('button').trigger('click');
    expect(wrapper.emitted('update:open')).toEqual([[false]]);
    wrapper.unmount();
  });

  it.each(['largeFileCleanup', 'duplicateFileCleanup'] as const)(
    'retains bounded %s evidence and failure status',
    category => {
      const payload = { items: [{ path: '/fixture/report.txt', status: 'failed' as const }], omittedItemCount: 3 };
      const record: PresentedOperationRecord =
        category === 'largeFileCleanup'
          ? { ...base, category, details: { type: 'largeFileCleanup', payload } }
          : { ...base, category, details: { type: 'duplicateFileCleanup', payload } };
      const wrapper = render(record);
      expect(wrapper.text()).toContain('report.txt');
      expect(wrapper.text()).toContain(i18n.global.t('history.deleteFailed'));
      expect(wrapper.find('.detail-omitted').text()).toContain('3');
      wrapper.unmount();
    }
  );

  it('keeps startup item counts and configured-state transitions', () => {
    const wrapper = render({
      ...base,
      category: 'startupManagement',
      details: {
        type: 'startupManagement',
        payload: {
          planId: null,
          items: [
            {
              itemId: 'item',
              displayName: 'Example startup',
              previousState: 'enabled',
              desiredState: 'disabled',
              status: 'changed',
              failureReason: null,
            },
          ],
        },
      },
    });
    expect(wrapper.find('.detail-summary').text()).toContain(i18n.global.t('history.changedItems'));
    expect(wrapper.find('.detail-action').text()).toContain('Example startup');
    expect(wrapper.find('.detail-action').text()).toContain('→');
    wrapper.unmount();
  });

  it('keeps cancelled privacy items visibly incomplete', () => {
    const wrapper = render({
      ...base,
      category: 'privacyCleanup',
      details: {
        type: 'privacyCleanup',
        payload: {
          planId: 'plan',
          timeRange: 'allTime',
          items: [
            { sourceId: 'browser', kind: 'cookies', affectedItemCount: 2, status: 'cancelled', failureReason: null },
          ],
        },
      },
    });
    expect(wrapper.find('.action-status').classes()).toContain('warning');
    expect(wrapper.find('.detail-action').text()).toContain(i18n.global.t('history.privacyStatuses.cancelled'));
    wrapper.unmount();
  });
});
