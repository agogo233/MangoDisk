// @vitest-environment happy-dom
import { mount } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { afterEach, describe, expect, it } from 'vitest';
import { Select } from '@/components/ui/select';
import type { ResidentReading } from '@/lib/models/resident/resident';
import MdFanDisplaySettings from './md-fan-display-settings.vue';

const wrappers: ReturnType<typeof mount>[] = [];
function reading(ids: number[]): ResidentReading['fanSpeed'] {
  return { status: 'ready', sampledAtMs: 1, value: { fans: ids.map(id => ({ id, rpm: id === 0 ? 0 : 1200 })) } };
}
function render(overrides: Partial<InstanceType<typeof MdFanDisplaySettings>['$props']> = {}) {
  const wrapper = mount(MdFanDisplaySettings, {
    props: { mode: 'highest', ids: [0], reading: reading([0]), disabled: false, open: false, ...overrides },
    global: {
      plugins: [
        createI18n({ legacy: false, locale: 'en', messages: { en: {} }, missingWarn: false, fallbackWarn: false }),
      ],
    },
  });
  wrappers.push(wrapper);
  return wrapper;
}
afterEach(() => wrappers.splice(0).forEach(wrapper => wrapper.unmount()));

describe('fan display choice visibility', () => {
  it('hides inactive Windows aggregation without changing the saved mode', async () => {
    const wrapper = render({ aggregateOnly: true, disabled: true, mode: 'average', reading: reading([0, 1]) });
    expect(wrapper.find('.selection-toggle').exists()).toBe(false);
    await wrapper.setProps({ disabled: false });
    expect(wrapper.getComponent(Select).props('modelValue')).toBe('average');
    expect(wrapper.emitted('change')).toBeUndefined();
  });
  it('limits Windows GPU fans to aggregation without persisting enumeration positions', async () => {
    const wrapper = render({ aggregateOnly: true, mode: 'selected', reading: reading([0, 1]), open: true });
    expect(wrapper.find('.fan-options').exists()).toBe(false);
    expect(wrapper.get('.selection-toggle').text()).toBe('systemStatus.fanAutomatic');
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'selected');
    expect(wrapper.emitted('change')).toBeUndefined();
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'average');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ fanDisplay: 'average', fanIds: [0] }]);
  });
  it('does not reopen a removed selector when a second fan returns', async () => {
    const wrapper = render({ reading: reading([0, 1]) });
    await wrapper.setProps({ 'onUpdate:open': open => void wrapper.setProps({ open }), open: true });
    await wrapper.setProps({ reading: reading([0]) });
    await wrapper.setProps({ reading: reading([0, 1]) });
    expect(wrapper.getComponent(Select).props('open')).toBe(false);
    expect(wrapper.emitted('change')).toBeUndefined();
  });
  it.each(['highest', 'average', 'selected'] as const)(
    'hides redundant single-fan controls in %s mode without saving preferences',
    async mode => {
      const wrapper = render({ mode });
      expect(wrapper.find('.selection-toggle').exists()).toBe(false);
      expect(wrapper.find('.fan-options').exists()).toBe(false);
      await wrapper.setProps({ reading: reading([0, 1]) });
      expect(wrapper.find('.selection-toggle').exists()).toBe(true);
      expect(wrapper.find('.fan-options').exists()).toBe(mode === 'selected');
      await wrapper.setProps({ reading: reading([0]) });
      expect(wrapper.find('.selection-toggle').exists()).toBe(false);
      expect(wrapper.emitted('change')).toBeUndefined();
    }
  );

  it('retains a recovery control for a disconnected selected fan', async () => {
    const wrapper = render({ mode: 'selected', ids: [1] });
    expect(wrapper.find('.selection-toggle').exists()).toBe(true);
    expect(wrapper.get('#fan-choice-1').attributes('aria-checked')).toBe('true');
    expect(wrapper.text()).toContain('systemStatus.disconnected');
    await wrapper.get('#fan-choice-0').trigger('click');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ fanDisplay: 'selected', fanIds: [0, 1] }]);
    await wrapper.setProps({ reading: reading([]) });
    expect(wrapper.find('.selection-toggle').exists()).toBe(true);
    expect(wrapper.find('.fan-options').exists()).toBe(true);
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'highest');
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ fanDisplay: 'highest', fanIds: [1] }]);
    await wrapper.setProps({ mode: 'highest', reading: reading([0]) });
    expect(wrapper.find('.selection-toggle').exists()).toBe(false);
  });

  it.each([1, 2])(
    'keeps %s known fan identities through unavailable samples without showing stale speeds',
    async count => {
      const wrapper = render({ mode: 'selected', reading: reading(count === 1 ? [0] : [0, 1]) });
      for (const status of ['loading', 'failed', 'stale', 'disconnected', 'unsupported'] as const) {
        await wrapper.setProps({ reading: { status, sampledAtMs: null, value: null } });
        expect(wrapper.find('.selection-toggle').exists()).toBe(count > 1);
        expect(wrapper.findAll('.fan-option')).toHaveLength(count > 1 ? count : 0);
        expect(wrapper.text()).not.toContain('1200');
        expect(wrapper.text()).not.toContain('systemStatus.disconnected');
      }
      await wrapper.setProps({ reading: { status: 'ready', sampledAtMs: 2, value: null } });
      expect(wrapper.find('.selection-toggle').exists()).toBe(count > 1);
      expect(wrapper.emitted('change')).toBeUndefined();
    }
  );
});
