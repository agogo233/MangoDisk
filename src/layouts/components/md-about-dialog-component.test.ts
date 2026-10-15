// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { h } from 'vue';
import { afterEach, expect, it, vi } from 'vitest';
import { TooltipProvider } from 'reka-ui';
import { i18n } from '@/i18n';
import MdAboutDialog from './md-about-dialog.vue';

let wrapper: ReturnType<typeof mount> | undefined;
afterEach(() => {
  wrapper?.unmount();
  document.body.innerHTML = '';
});
it('opens third-party licenses from About and closes the child without dismissing About', async () => {
  wrapper = mount(
    {
      render: () =>
        h(TooltipProvider, null, {
          default: () =>
            h(MdAboutDialog, {
              open: true,
              status: 'idle',
              action: null,
              currentVersion: '1.1.8',
              version: '',
              notes: '',
              checkError: '',
              downloadedBytes: 0,
              totalBytes: null,
              actionError: '',
              failureStage: null,
            }),
        }),
    },
    { attachTo: document.body, global: { plugins: [i18n] } }
  );
  await flushPromises();
  expect(document.body.textContent).not.toContain('Bundled PawnIO components');
  const entry = [...document.body.querySelectorAll('button')].find(
    button => button.textContent?.trim() === i18n.global.t('thirdPartyLicenses.title')
  );
  expect(entry).toBeDefined();
  entry!.click();
  await vi.dynamicImportSettled();
  await flushPromises();
  await flushPromises();
  expect(document.body.textContent).toContain('PawnIO Modules');
  expect(document.body.textContent).toContain(i18n.global.t('thirdPartyLicenses.readLicense'));
  const dialogs = document.body.querySelectorAll<HTMLElement>('[data-slot="dialog-content"]');
  expect(dialogs).toHaveLength(2);
  const childClose = [...dialogs[1]!.querySelectorAll('button')].find(
    button => button.textContent?.trim() === i18n.global.t('common.close')
  );
  childClose!.click();
  await flushPromises();
  expect(document.body.querySelectorAll('[data-slot="dialog-content"]')).toHaveLength(1);
  expect(wrapper.findComponent(MdAboutDialog).emitted('close')).toBeUndefined();
});
