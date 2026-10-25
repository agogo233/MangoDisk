// @vitest-environment happy-dom
import { flushPromises, shallowMount } from '@vue/test-utils';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { i18n } from '@/i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import { ExplorerIntegrationService } from '@/lib/services/settings/explorer-integration-service';
import Settings from './md-explorer-integration-settings.vue';

vi.mock('@/lib/services/settings/explorer-integration-service', () => ({
  ExplorerIntegrationService: { status: vi.fn(), setEnabled: vi.fn() },
}));
vi.mock('@/lib/services/diagnostics/logger-service', () => ({ LoggerService: { warn: vi.fn() } }));

beforeEach(() => {
  i18n.global.locale.value = 'en-US';
  vi.mocked(ExplorerIntegrationService.setEnabled).mockReset();
  vi.mocked(ExplorerIntegrationService.status).mockResolvedValue({ enabled: false });
});
afterEach(() => vi.clearAllMocks());

function open() {
  return shallowMount(Settings, {
    global: {
      plugins: [i18n],
      stubs: { MdSettingsRow: { template: '<div><slot /></div>' } },
    },
  });
}

it('does not register on mount and waits for native success before showing enabled', async () => {
  const wrapper = open();
  await flushPromises();
  expect(ExplorerIntegrationService.setEnabled).not.toHaveBeenCalled();
  let finish!: (value: { enabled: boolean }) => void;
  vi.mocked(ExplorerIntegrationService.setEnabled).mockReturnValueOnce(
    new Promise(resolve => {
      finish = resolve;
    })
  );
  const toggle = wrapper.findComponent(MdSwitch);
  toggle.vm.$emit('update:modelValue', true);
  await flushPromises();
  expect(toggle.props('disabled')).toBe(true);
  expect(toggle.props('modelValue')).toBe(false);
  finish({ enabled: true });
  await flushPromises();
  expect(toggle.props('modelValue')).toBe(true);
  expect(toggle.props('disabled')).toBe(false);
  wrapper.unmount();
});

it('reconciles a failed registration and keeps disabling available', async () => {
  vi.mocked(ExplorerIntegrationService.status).mockResolvedValue({ enabled: true });
  const wrapper = open();
  await flushPromises();
  vi.mocked(ExplorerIntegrationService.setEnabled).mockRejectedValueOnce(new Error('registration failed'));
  vi.mocked(ExplorerIntegrationService.status).mockResolvedValueOnce({ enabled: false });
  wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', true);
  await flushPromises();
  expect(wrapper.find('[role="alert"]').exists()).toBe(true);
  expect(wrapper.findComponent(MdSwitch).props('modelValue')).toBe(false);
  expect(wrapper.findComponent(MdSwitch).props('disabled')).toBe(false);
  wrapper.unmount();
});

it('keeps the latest language when it changes while enabling the menu', async () => {
  const wrapper = open();
  try {
    await flushPromises();
    let finish!: (value: { enabled: boolean }) => void;
    vi.mocked(ExplorerIntegrationService.setEnabled)
      .mockResolvedValue({ enabled: true })
      .mockReturnValueOnce(new Promise(resolve => (finish = resolve)));
    wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', true);
    await flushPromises();
    i18n.global.locale.value = 'zh-CN';
    await flushPromises();
    finish({ enabled: true });
    await flushPromises();
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenLastCalledWith(
      true,
      i18n.global.t('settings.explorerIntegration.command')
    );
  } finally {
    wrapper.unmount();
    i18n.global.locale.value = 'en-US';
  }
});

it('does not re-enable the menu after a language change while disabling', async () => {
  vi.mocked(ExplorerIntegrationService.status).mockResolvedValue({ enabled: true });
  const wrapper = open();
  try {
    await flushPromises();
    let finish!: (value: { enabled: boolean }) => void;
    vi.mocked(ExplorerIntegrationService.setEnabled).mockReturnValueOnce(new Promise(resolve => (finish = resolve)));
    wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', false);
    await flushPromises();
    i18n.global.locale.value = 'zh-CN';
    await flushPromises();
    finish({ enabled: false });
    await flushPromises();
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenCalledOnce();
    expect(wrapper.findComponent(MdSwitch).props('modelValue')).toBe(false);
  } finally {
    wrapper.unmount();
    i18n.global.locale.value = 'en-US';
  }
});

it('does not send a deferred title update after leaving settings', async () => {
  const wrapper = open();
  try {
    await flushPromises();
    let finish!: (value: { enabled: boolean }) => void;
    vi.mocked(ExplorerIntegrationService.setEnabled)
      .mockResolvedValue({ enabled: true })
      .mockReturnValueOnce(new Promise(resolve => (finish = resolve)));
    wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', true);
    await flushPromises();
    i18n.global.locale.value = 'zh-CN';
    await flushPromises();
    wrapper.unmount();
    finish({ enabled: true });
    await flushPromises();
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenCalledOnce();
  } finally {
    wrapper.unmount();
    i18n.global.locale.value = 'en-US';
  }
});

it('retries the failed registration instead of only clearing its error', async () => {
  const wrapper = open();
  try {
    await flushPromises();
    vi.mocked(ExplorerIntegrationService.setEnabled).mockRejectedValueOnce(new Error('certificate not trusted'));
    wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', true);
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    vi.mocked(ExplorerIntegrationService.setEnabled).mockResolvedValueOnce({ enabled: true });
    await wrapper.find('button').trigger('click');
    await flushPromises();
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenCalledTimes(2);
    expect(wrapper.findComponent(MdSwitch).props('modelValue')).toBe(true);
  } finally {
    wrapper.unmount();
  }
});

it('retries a failed disable even when reconciliation could not read the status', async () => {
  vi.mocked(ExplorerIntegrationService.status).mockResolvedValue({ enabled: true });
  const wrapper = open();
  try {
    await flushPromises();
    vi.mocked(ExplorerIntegrationService.setEnabled).mockRejectedValueOnce(new Error('temporarily in use'));
    vi.mocked(ExplorerIntegrationService.status).mockRejectedValueOnce(new Error('status unavailable'));
    wrapper.findComponent(MdSwitch).vm.$emit('update:modelValue', false);
    await flushPromises();
    expect(wrapper.findComponent(MdSwitch).props('disabled')).toBe(true);
    vi.mocked(ExplorerIntegrationService.setEnabled).mockResolvedValueOnce({ enabled: false });
    await wrapper.find('button').trigger('click');
    await flushPromises();
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenCalledTimes(2);
    expect(ExplorerIntegrationService.setEnabled).toHaveBeenLastCalledWith(
      false,
      i18n.global.t('settings.explorerIntegration.command')
    );
    expect(wrapper.findComponent(MdSwitch).props('modelValue')).toBe(false);
  } finally {
    wrapper.unmount();
  }
});

it('retries only the read after the initial status request failed', async () => {
  vi.mocked(ExplorerIntegrationService.status).mockRejectedValueOnce(new Error('status unavailable'));
  const wrapper = open();
  try {
    await flushPromises();
    expect(wrapper.find('[role="alert"]').exists()).toBe(true);
    await wrapper.find('button').trigger('click');
    await flushPromises();
    expect(ExplorerIntegrationService.status).toHaveBeenCalledTimes(2);
    expect(ExplorerIntegrationService.setEnabled).not.toHaveBeenCalled();
    expect(wrapper.find('[role="alert"]').exists()).toBe(false);
  } finally {
    wrapper.unmount();
  }
});
