// @vitest-environment happy-dom
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { afterEach, describe, expect, it } from 'vitest';
import { Select } from '@/components/ui/select';
import MdGpuDisplaySettings from './md-gpu-display-settings.vue';
import type { GpuDisplaySelection } from '@/lib/models/resident/resident';

const adapters = [
  { id: 'a', name: 'Radeon' },
  { id: 'b', name: 'Intel' },
];
const wrappers: ReturnType<typeof mount>[] = [];
function render(overrides: Partial<InstanceType<typeof MdGpuDisplaySettings>['$props']> = {}) {
  const wrapper = mount(MdGpuDisplaySettings, {
    props: {
      temperature: false,
      selection: { mode: 'automatic' } as GpuDisplaySelection,
      adapters,
      devices: [],
      observedAtMs: 1000,
      disabled: false,
      open: false,
      ...overrides,
    },
    attachTo: document.body,
    global: {
      plugins: [
        createI18n({ legacy: false, locale: 'en', missingWarn: false, fallbackWarn: false, messages: { en: {} } }),
      ],
    },
  });
  wrappers.push(wrapper);
  return wrapper;
}
afterEach(() => {
  wrappers.splice(0).forEach(wrapper => wrapper.unmount());
});

describe('GPU display choice visibility', () => {
  it('does not reopen a removed selector when a second GPU returns', async () => {
    const wrapper = render();
    await wrapper.setProps({ 'onUpdate:open': open => void wrapper.setProps({ open }), open: true });
    await flushPromises();
    expect(wrapper.getComponent(Select).props('open')).toBe(true);
    await wrapper.setProps({ adapters: adapters.slice(0, 1) });
    await wrapper.setProps({ adapters });
    await flushPromises();
    expect(wrapper.getComponent(Select).props('open')).toBe(false);
    expect(wrapper.emitted('change')).toBeUndefined();
  });
  it.each([false, true])(
    'collapses automatic choices and offers only two modes with temperature=%s',
    async temperature => {
      const wrapper = render({ temperature });
      expect(wrapper.find('.selection-toggle').exists()).toBe(true);
      expect(wrapper.find('.gpu-options').exists()).toBe(false);
      await wrapper.setProps({ open: true });
      await flushPromises();
      expect(
        Array.from(document.querySelectorAll('[role="option"]')).map(option => option.textContent?.trim())
      ).toEqual([temperature ? 'gpuTemperature.followUsage' : 'systemStatus.automatic', 'gpuDetails.selectedAdapters']);
      expect(wrapper.emitted('change')).toBeUndefined();
    }
  );

  it('expands explicit choices, keeps at least one device and collapses when automatic resumes', async () => {
    const wrapper = render();
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'selected');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ gpuDisplay: { mode: 'selected', adapterIds: ['a'] } }]);
    await wrapper.setProps({ selection: { mode: 'selected', adapterIds: ['a'] } });
    expect(wrapper.findAll('.gpu-option')).toHaveLength(2);
    expect(wrapper.get('#gpu-choice-usage-0').attributes('disabled')).toBeDefined();
    await wrapper.get('#gpu-choice-usage-1').trigger('click');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ gpuDisplay: { mode: 'selected', adapterIds: ['a', 'b'] } }]);
    await wrapper.setProps({ selection: { mode: 'selected', adapterIds: ['a', 'b'] } });
    expect(wrapper.get('#gpu-choice-usage-0').attributes('disabled')).toBeUndefined();
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'automatic');
    await wrapper.setProps({ selection: { mode: 'automatic' } });
    expect(wrapper.find('.gpu-options').exists()).toBe(false);
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ gpuDisplay: { mode: 'automatic' } }]);
  });

  it.each(['automatic', 'selected'] as const)(
    'hides redundant single-GPU controls in %s mode without changing configuration',
    async mode => {
      const selection: GpuDisplaySelection = mode === 'automatic' ? { mode } : { mode, adapterIds: ['a'] };
      const wrapper = render({ adapters: adapters.slice(0, 1), selection });
      expect(wrapper.find('.selection-toggle').exists()).toBe(false);
      expect(wrapper.find('.gpu-options').exists()).toBe(false);
      await wrapper.setProps({ adapters });
      expect(wrapper.find('.selection-toggle').exists()).toBe(true);
      expect(wrapper.find('.gpu-options').exists()).toBe(mode === 'selected');
      expect(wrapper.emitted('change')).toBeUndefined();
    }
  );

  it('retains disconnected choices and a recovery control when only one GPU remains', async () => {
    const wrapper = render({
      adapters: adapters.slice(0, 1),
      temperature: true,
      selection: { mode: 'selected', adapterIds: ['b'] },
    });
    expect(wrapper.find('.selection-toggle').exists()).toBe(true);
    expect(wrapper.text()).toContain('systemStatus.savedDisconnected');
    expect(wrapper.get('#gpu-choice-temperature-1').attributes('aria-checked')).toBe('true');
    await wrapper.get('#gpu-choice-temperature-0').trigger('click');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([
      { gpuTemperatureDisplay: { mode: 'selected', adapterIds: ['b', 'a'] } },
    ]);
    await wrapper.setProps({ adapters: [] });
    expect(wrapper.find('.selection-toggle').exists()).toBe(true);
    expect(wrapper.findAll('.gpu-option')).toHaveLength(1);
  });
});
