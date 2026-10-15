// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { h } from 'vue';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from 'reka-ui';

import { i18n } from '@/i18n';
import { LANGUAGE_IDS } from '@/lib/models/settings';
import { ClipboardService } from '@/lib/services/clipboard-service';
import MdThirdPartyLicensesDialog from './md-third-party-licenses-dialog.vue';
import license from '../../../src-tauri/crates/mangodisk-privileged/vendors/pawnio/modules/0.2.11/COPYING?raw';

vi.mock('@/lib/models/third-party-licenses', async importOriginal => {
  const original = await importOriginal<typeof import('@/lib/models/third-party-licenses')>();
  return {
    ...original,
    THIRD_PARTY_COMPONENTS: [
      ...original.THIRD_PARTY_COMPONENTS,
      {
        id: 'additional-component',
        nameKey: 'common.copy',
        version: '3.0',
        author: 'Example author',
        licenseKey: 'common.close',
        descriptionKey: 'common.cancel',
        projectUrl: 'https://example.com/component',
        documents: [
          {
            id: 'license',
            titleKey: 'common.close',
            actionKey: 'thirdPartyLicenses.readLicense',
            format: 'text',
            content: 'Additional component license',
          },
        ],
      },
    ],
  };
});

const originalLocale = i18n.global.locale.value;
const wrappers: ReturnType<typeof mount>[] = [];
function mountDialog() {
  const wrapper = mount(
    { render: () => h(TooltipProvider, null, { default: () => h(MdThirdPartyLicensesDialog, { open: true }) }) },
    { attachTo: document.body, global: { plugins: [i18n] } }
  );
  wrappers.push(wrapper);
  return wrapper.findComponent(MdThirdPartyLicensesDialog);
}
function button(label: string): HTMLButtonElement {
  const found = [...document.body.querySelectorAll('button')].find(
    element => element.textContent?.trim() === label || element.getAttribute('aria-label') === label
  );
  expect(found, `button ${label} exists`).toBeDefined();
  return found!;
}
function modulesButton(label: string): HTMLButtonElement {
  return [
    ...document.body.querySelectorAll<HTMLButtonElement>('[aria-labelledby="pawnio-modules-heading"] button'),
  ].find(button => button.textContent?.trim() === label)!;
}
afterEach(() => {
  wrappers.splice(0).forEach(wrapper => wrapper.unmount());
  i18n.global.locale.value = originalLocale;
  vi.restoreAllMocks();
  document.body.innerHTML = '';
});

describe('third-party licenses dialog', () => {
  it('identifies each component before showing and copying its original license', async () => {
    const copy = vi.spyOn(ClipboardService, 'writeText').mockResolvedValue();
    mountDialog();
    await flushPromises();
    const modules = document.body.querySelector('[aria-labelledby="pawnio-modules-heading"]');
    expect(modules?.textContent).toContain('PawnIO Modules');
    expect(modules?.textContent).toContain('0.2.11');
    expect(modules?.textContent).toContain('LGPL 2.1');
    const installer = document.body.querySelector('[aria-labelledby="pawnio-installer-heading"]');
    expect(installer?.textContent).toContain('2.2.0');
    expect(installer?.textContent).toContain(i18n.global.t('thirdPartyLicenses.components.pawnioInstaller.license'));
    expect(installer?.textContent).not.toContain('LGPL');
    expect(installer?.textContent).toContain('namazso');
    expect(modules?.textContent).toContain('namazso');
    expect(document.body.querySelectorAll('[data-document-action]')).toHaveLength(2);
    expect(document.body.querySelector('[data-slot="dialog-description"]')?.classList.contains('sr-only')).toBe(true);

    modulesButton(i18n.global.t('thirdPartyLicenses.readLicense')).click();
    await flushPromises();
    expect(document.body.querySelector('[data-slot="dialog-title"]')?.textContent).toBe(
      i18n.global.t('thirdPartyLicenses.documentTitle', {
        component: 'PawnIO Modules',
        version: '0.2.11',
        document: 'LGPL 2.1',
      })
    );
    expect(document.body.querySelector('pre')?.textContent === license).toBe(true);
    button(i18n.global.t('common.copy')).click();
    await flushPromises();
    expect(copy).toHaveBeenLastCalledWith(license);

    button(i18n.global.t('thirdPartyLicenses.back')).click();
    await flushPromises();
    expect(document.activeElement).toBe(modulesButton(i18n.global.t('thirdPartyLicenses.readLicense')));
  });

  it('opens the upstream project from each component title', async () => {
    const dialog = mountDialog();
    await flushPromises();
    modulesButton('PawnIO Modules').click();
    [...document.body.querySelectorAll<HTMLButtonElement>('[aria-labelledby="pawnio-installer-heading"] button')]
      .find(
        button => button.textContent?.trim() === i18n.global.t('thirdPartyLicenses.components.pawnioInstaller.name')
      )!
      .click();
    expect(dialog.emitted('openLink')).toEqual([
      ['https://github.com/namazso/PawnIO.Modules'],
      ['https://github.com/namazso/PawnIO.Setup'],
    ]);
  });

  it('renders an additional catalog entry and its documents without component-specific viewer branches', async () => {
    const dialog = mountDialog();
    await flushPromises();
    const card = document.body.querySelector('[aria-labelledby="additional-component-heading"]')!;
    expect(card.textContent).toContain('3.0');
    card.querySelector<HTMLButtonElement>('[data-document-action]')!.click();
    await flushPromises();
    expect(document.body.querySelector('pre')?.textContent).toBe('Additional component license');
    expect(document.body.querySelector('[data-slot="dialog-title"]')?.textContent).toBe(
      i18n.global.t('thirdPartyLicenses.documentTitle', {
        component: i18n.global.t('common.copy'),
        version: '3.0',
        document: i18n.global.t('common.close'),
      })
    );
    button(i18n.global.t('thirdPartyLicenses.back')).click();
    await flushPromises();
    expect(document.activeElement?.getAttribute('data-document-action')).toBe('additional-component:license');
    document.body
      .querySelector<HTMLElement>('[aria-labelledby="additional-component-heading"]')!
      .querySelector<HTMLButtonElement>('h3 button')!
      .click();
    expect(dialog.emitted('openLink')?.at(-1)).toEqual(['https://example.com/component']);
  });

  it('names the license owner in every language while preserving the license original', async () => {
    mountDialog();
    for (const locale of Object.values(LANGUAGE_IDS)) {
      i18n.global.locale.value = locale;
      await flushPromises();
      expect(document.body.querySelector('[data-slot="dialog-title"]')?.textContent).toBe(
        i18n.global.t('thirdPartyLicenses.title')
      );
      modulesButton(i18n.global.t('thirdPartyLicenses.readLicense')).click();
      await flushPromises();
      expect(document.body.querySelector('[data-slot="dialog-title"]')?.textContent).toContain('PawnIO Modules');
      expect(document.body.querySelector('pre')?.textContent === license).toBe(true);
      button(i18n.global.t('thirdPartyLicenses.back')).click();
      await flushPromises();
      expect(document.body.querySelector('[aria-labelledby="pawnio-modules-heading"]')?.textContent).toContain(
        '0.2.11'
      );
    }
  });
});
