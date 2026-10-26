// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { describe, expect, it } from 'vitest';
import MdSwitch from '@/components/custom/md-switch.vue';
import Mode from './md-resident-display-mode.vue';
import Appearance from './md-floating-appearance.vue';
import Behavior from './md-floating-behavior.vue';
import { Select } from '@/components/ui/select';
import { preferencesFixture } from '@/tests/fixtures/resident';
import en from '@/locales/en-US.json';

const locales = Object.fromEntries(
  Object.entries(
    import.meta.glob<typeof en>('/src/locales/*.json', {
      eager: true,
      import: 'default',
    })
  ).map(([path, messages]) => [path.split('/').at(-1)!.replace('.json', ''), messages])
);
describe('Windows display mode settings', () => {
  it.each(Object.entries(locales))(
    'localizes floating settings and preserves unrelated options in %s',
    async (locale, messages) => {
      const preferences = preferencesFixture();
      const wrapper = mount(Mode, {
        props: { preferences, platform: 'windows' },
        global: { plugins: [createI18n({ legacy: false, locale, messages: locales })] },
      });
      expect(wrapper.text()).toContain(messages.floatingMonitor.displayMode);
      expect(wrapper.find('#floating-layout').exists()).toBe(false);
      wrapper.findComponent(Select).vm.$emit('update:modelValue', 'floating');
      expect(wrapper.emitted('change')?.at(-1)).toEqual([{ windowsDisplayMode: 'floating' }]);
      await wrapper.setProps({ preferences: { ...preferences, windowsDisplayMode: 'floating' } });
      await flushPromises();
      expect(wrapper.find('#floating-layout').exists()).toBe(false);
      const behavior = mount(Behavior, {
        props: { preferences },
        global: { plugins: [createI18n({ legacy: false, locale, messages: locales })] },
      });
      const appearance = mount(Appearance, {
        props: { preferences },
        global: { plugins: [createI18n({ legacy: false, locale, messages: locales })] },
      });
      expect(behavior.text()).toContain(messages.floatingMonitor.alwaysOnTop);
      expect(behavior.text()).toContain(messages.floatingMonitor.snapToEdges);
      expect(behavior.text()).toContain(messages.floatingMonitor.clickThroughHint);
      behavior
        .findAllComponents(MdSwitch)
        .find(component => component.attributes('id') === 'floating-click-through')!
        .vm.$emit('update:modelValue', true);
      expect(behavior.emitted('change')?.at(-1)).toEqual([
        { floating: { ...preferences.floating, clickThrough: true } },
      ]);
      behavior
        .findAllComponents(MdSwitch)
        .find(component => component.attributes('id') === 'floating-snap')!
        .vm.$emit('update:modelValue', false);
      expect(behavior.emitted('change')?.at(-1)).toEqual([
        { floating: { ...preferences.floating, snapToEdges: false } },
      ]);
      appearance
        .findAllComponents(Select)
        .find(component => component.find('#floating-layout').exists())!
        .vm.$emit('update:modelValue', 'vertical');
      expect(appearance.emitted('change')?.at(-1)).toEqual([
        {
          floating: { ...preferences.floating, layout: 'vertical' },
        },
      ]);
      appearance
        .findAllComponents(Select)
        .find(component => component.find('#floating-opacity').exists())!
        .vm.$emit('update:modelValue', '60');
      expect(appearance.emitted('change')?.at(-1)).toEqual([
        { floating: { ...preferences.floating, backgroundOpacity: 60 } },
      ]);
      for (const [id, field, value] of [
        ['floating-font-size', 'fontSize', 'extraLarge'],
        ['floating-background', 'background', 'black'],
      ] as const) {
        appearance
          .findAllComponents(Select)
          .find(component => component.find(`#${id}`).exists())!
          .vm.$emit('update:modelValue', value);
        expect(appearance.emitted('change')?.at(-1)).toEqual([
          { floating: { ...preferences.floating, [field]: value } },
        ]);
      }
      appearance
        .findAllComponents(MdSwitch)
        .find(component => component.attributes('id') === 'floating-bold')!
        .vm.$emit('update:modelValue', true);
      expect(appearance.emitted('change')?.at(-1)).toEqual([{ floating: { ...preferences.floating, bold: true } }]);
      appearance
        .findAllComponents(MdSwitch)
        .find(component => component.attributes('id') === 'floating-dividers')!
        .vm.$emit('update:modelValue', false);
      expect(appearance.emitted('change')?.at(-1)).toEqual([
        { floating: { ...preferences.floating, dividers: false } },
      ]);
      expect(appearance.text()).toContain(messages.floatingMonitor.dividers);
      expect(appearance.text()).not.toMatch(/floatingMonitor\./u);
      behavior.unmount();
      appearance.unmount();
      wrapper.unmount();
    }
  );
  it('shows a valid saved percentage between the menu presets', async () => {
    const preferences = preferencesFixture();
    preferences.windowsDisplayMode = 'floating';
    preferences.floating.backgroundOpacity = 75;
    const wrapper = mount(Appearance, {
      props: { preferences },
      global: { plugins: [createI18n({ legacy: false, locale: 'en-US', messages: locales })] },
    });
    await flushPromises();
    expect(wrapper.get('#floating-opacity').text()).toContain('75%');
    wrapper.unmount();
  });
});

describe('shared resident display mode', () => {
  it.each(
    Object.keys(locales).flatMap(locale =>
      (['macos', 'windows', 'linux'] as const).map(platform => ({ locale, platform }))
    )
  )('keeps platform choices independent for $platform in $locale', async ({ locale, platform }) => {
    const preferences = preferencesFixture();
    const wrapper = mount(Mode, {
      props: { preferences, platform },
      global: { plugins: [createI18n({ legacy: false, locale, messages: locales })] },
    });
    const nativeMode = platform === 'macos' ? 'menuBar' : platform === 'linux' ? 'tray' : 'taskbar';
    const field =
      platform === 'macos' ? 'macosDisplayMode' : platform === 'linux' ? 'linuxDisplayMode' : 'windowsDisplayMode';
    expect(wrapper.get(`#${platform}-display-mode`).text()).toContain(locales[locale]!.floatingMonitor[nativeMode]);
    const select = wrapper.getComponent(Select);
    select.vm.$emit('update:modelValue', platform === 'macos' ? 'taskbar' : 'menuBar');
    select.vm.$emit('update:modelValue', null);
    expect(wrapper.emitted('change')).toBeUndefined();
    for (const mode of [nativeMode, 'floating', 'both'] as const) {
      select.vm.$emit('update:modelValue', mode);
      expect(wrapper.emitted('change')?.at(-1)).toEqual([{ [field]: mode }]);
      await wrapper.setProps({ preferences: { ...preferences, [field]: mode } });
      expect(wrapper.get(`#${platform}-display-mode`).text()).toContain(locales[locale]!.floatingMonitor[mode]);
    }
    expect(preferences.windowsDisplayMode).toBe('taskbar');
    expect(preferences.macosDisplayMode).toBe('menuBar');
    expect(preferences.linuxDisplayMode).toBe('tray');
    wrapper.unmount();
  });
});

describe('Windows fullscreen hiding settings', () => {
  it.each(Object.keys(locales))('preserves unrelated floating preferences in %s', async locale => {
    const preferences = preferencesFixture();
    const wrapper = mount(Behavior, {
      props: { preferences },
      global: { plugins: [createI18n({ legacy: false, locale, messages: locales })] },
    });
    expect(wrapper.text()).toContain(locales[locale]!.floatingMonitor.hideOnFullscreen);
    const toggle = wrapper.findAllComponents(MdSwitch).find(item => item.attributes('id') === 'floating-fullscreen')!;
    expect(toggle.props('modelValue')).toBe(true);
    toggle.vm.$emit('update:modelValue', false);
    expect(wrapper.emitted('change')?.at(-1)).toEqual([
      { floating: { ...preferences.floating, hideOnFullscreen: false } },
    ]);
    await wrapper.setProps({
      preferences: { ...preferences, floating: { ...preferences.floating, hideOnFullscreen: false } },
    });
    expect(toggle.props('modelValue')).toBe(false);
    toggle.vm.$emit('update:modelValue', true);
    expect(wrapper.emitted('change')?.at(-1)).toEqual([{ floating: preferences.floating }]);
    expect(preferences.floating.hideOnFullscreen).toBe(true);
    wrapper.unmount();
  });
});
