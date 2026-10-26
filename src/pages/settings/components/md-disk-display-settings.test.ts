// @vitest-environment happy-dom
import { mount, flushPromises } from '@vue/test-utils';
import { createI18n } from 'vue-i18n';
import { afterEach, describe, expect, it } from 'vitest';
import { Select } from '@/components/ui/select';
import type { ResourceVolume } from '@/lib/models/system-resources/system-resources';
import Settings from './md-disk-display-settings.vue';
import en from '@/locales/en-US.json';
import zh from '@/locales/zh-CN.json';

const locales = Object.fromEntries(
  Object.entries(import.meta.glob<typeof en>('/src/locales/*.json', { eager: true, import: 'default' })).map(
    ([path, messages]) => [path.split('/').at(-1)!.replace('.json', ''), messages]
  )
);

const diskA = { id: 'a', number: 0, model: 'Samsung SSD 990 PRO' };
const diskB = { id: 'b', number: 1, model: 'WD Blue SN580' };
const volumes: ResourceVolume[] = [
  { id: 'c', name: 'Windows (C:)', system: true, fileSystem: 'NTFS', mountPoints: ['C:'], devices: [diskA] },
  { id: 'd', name: 'D:', system: false, fileSystem: 'NTFS', mountPoints: ['D:'], devices: [diskA] },
  { id: 'f', name: 'Work (F:)', system: false, fileSystem: 'NTFS', mountPoints: ['F:'], devices: [diskB] },
];
const wrappers: ReturnType<typeof mount>[] = [];
function render(temperature: boolean, locale = 'en') {
  const wrapper = mount(Settings, {
    props: { temperature, volumeId: 'd', deviceId: null, volumes, disabled: false, open: true },
    attachTo: document.body,
    global: { plugins: [createI18n({ legacy: false, locale, messages: { en, zh, ...locales } })] },
  });
  wrappers.push(wrapper);
  return wrapper;
}
afterEach(() => {
  wrappers.forEach(wrapper => wrapper.unmount());
  wrappers.length = 0;
  document.body.innerHTML = '';
});

describe('disk settings choices', () => {
  it.each(Object.entries(locales))(
    'renders translated choices and readable disk identity in %s',
    async (locale, messages) => {
      render(true, locale);
      await flushPromises();
      expect(document.body.textContent).toContain(messages.systemStatus.diskTemperatureFollow);
      expect(document.body.textContent).toContain('Samsung SSD 990 PRO (C:, D:)');
      expect(document.body.textContent).not.toContain('systemStatus.');
    }
  );
  it('presents physical groups, volume labels and a named system-volume policy', async () => {
    render(false);
    await flushPromises();
    expect(document.body.textContent).toContain('Disk 0 · Samsung SSD 990 PRO');
    expect(document.body.textContent).toContain('Disk 1 · WD Blue SN580');
    expect(document.body.textContent).toContain('Local disk (D:)');
    expect(document.body.textContent).toContain('System disk (automatic)');
    expect(document.body.textContent).not.toContain('Samsung SSD 990 PRO (C:, D:)');
    expect(document.body.textContent).toContain('Windows (C:)');
  });
  it('shows the current system drive in the automatic selection and preserves named partitions', async () => {
    const wrapper = render(false, 'zh');
    await wrapper.setProps({ volumeId: null });
    expect(wrapper.get('.selection-toggle').text()).toBe('系统盘（C:）');
    expect(document.body.textContent).not.toContain('当前：');
    expect(document.body.textContent).toContain('本地磁盘 (D:)');
    await wrapper.setProps({ volumeId: 'f' });
    expect(wrapper.get('.selection-toggle').text()).toBe('Work (F:)');
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'd');
    expect(wrapper.emitted('change')?.[0]).toEqual([{ diskVolume: 'd' }]);
  });
  it('preserves macOS volume names in the system source and grouped choices', async () => {
    const wrapper = render(false, 'zh');
    await wrapper.setProps({
      volumeId: null,
      volumes: [
        { ...volumes[0]!, name: 'Macintosh HD', mountPoints: ['/'] },
        { ...volumes[1]!, name: 'SanDisk', mountPoints: ['/Volumes/SanDisk'] },
      ],
    });
    expect(wrapper.get('.selection-toggle').text()).toBe('系统盘（Macintosh HD）');
    expect(document.body.textContent).not.toContain('当前：');
    expect(document.body.textContent).toContain('SanDisk');
    expect(document.body.textContent).not.toContain('本地磁盘');
  });
  it('keeps the automatic choice concise and retains the system source in its trigger tooltip', async () => {
    const wrapper = render(false);
    await wrapper.setProps({ volumeId: 'f' });
    const follow = [...document.querySelectorAll('[role="option"]')].find(option =>
      option.textContent?.includes(en.systemStatus.systemDiskAutomatic)
    );
    expect(follow?.textContent?.trim()).toBe(en.systemStatus.systemDiskAutomatic);
    await wrapper.setProps({ volumeId: null });
    expect(wrapper.getComponent({ name: 'MdTooltip' }).props('text')).toContain('Windows (C:)');
  });
  it('changes only temperature device and restores follow mode without modifying capacity selection', async () => {
    const wrapper = render(true);
    await flushPromises();
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'b');
    expect(wrapper.emitted('change')?.[0]).toEqual([{ diskTemperatureDevice: 'b' }]);
    wrapper.getComponent(Select).vm.$emit('update:modelValue', 'follow');
    expect(wrapper.emitted('change')?.[1]).toEqual([{ diskTemperatureDevice: null }]);
  });
  it('keeps a disconnected explicit selection visible and offers recovery to follow mode', async () => {
    const wrapper = render(true);
    await wrapper.setProps({ deviceId: 'removed' });
    expect(wrapper.get('.selection-toggle').text()).toBe(en.systemStatus.savedDisconnected);
    expect(document.body.textContent).toContain(en.systemStatus.diskTemperatureFollow);
  });
  it('updates the current following disk when the volume changes, without persisting a guessed device', async () => {
    const wrapper = render(true, 'zh');
    await flushPromises();
    await wrapper.setProps({ volumeId: 'f' });
    expect(document.body.textContent).toContain('磁盘 1 · WD Blue SN580 (F:)');
    expect(wrapper.emitted('change')).toBeUndefined();
  });
  it('blocks a single temperature for a spanning-only disk while preserving volume selection', async () => {
    const wrapper = render(true);
    await wrapper.setProps({ volumes: [{ ...volumes[0]!, devices: [diskA, diskB] }] });
    expect(document.querySelectorAll('[role="option"][data-disabled]')).toHaveLength(2);
    expect(document.body.textContent).toContain(en.systemStatus.spanningVolume);
  });
});
