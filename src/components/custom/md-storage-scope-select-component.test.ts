// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { defineComponent, h, ref } from 'vue';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { i18n } from '@/i18n';
import { FolderSelectionService } from '@/lib/services/folder-selection-service';
import MdStorageScopeSelect from './md-storage-scope-select.vue';

afterEach(() => vi.restoreAllMocks());

function mountSelector(multiple: boolean) {
  const selected = ref<string | string[]>(multiple ? [] : 'E:\\');
  const wrapper = mount(
    defineComponent({
      setup: () => () =>
        h(
          TooltipProvider,
          {},
          {
            default: () =>
              h(MdStorageScopeSelect, {
                multiple,
                modelValue: selected.value,
                disks: ['E:\\', 'F:\\'].map(mountPoint => ({
                  name: mountPoint,
                  mountPoint,
                  totalBytes: 100,
                  availableBytes: 50,
                  usedBytes: 50,
                })),
                recentFolders: ['E:\\Work', 'F:\\Chat'],
                'onUpdate:modelValue': value => {
                  selected.value = value;
                },
              }),
          }
        ),
    }),
    { attachTo: document.body, global: { plugins: [i18n], stubs: { MdNativeFileIcon: true } } }
  );
  async function open() {
    await wrapper.get('[role="combobox"]').trigger('keydown', { key: 'ArrowDown' });
    await flushPromises();
  }
  async function choose(text: string) {
    const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
      el => el.textContent?.trim() === text
    );
    expect(option, `option ${text} should be present`).toBeDefined();
    option!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flushPromises();
  }
  return { wrapper, selected, open, choose };
}

describe('storage scope selection', () => {
  it('keeps multi-select open while adding disks and folders and allows deselecting all', async () => {
    const view = mountSelector(true);
    try {
      await view.open();
      await view.choose('E:\\');
      await view.choose('Chat');
      expect(view.selected.value).toEqual(['E:\\', 'F:\\Chat']);
      expect(document.querySelector('[role="listbox"]')).not.toBeNull();
      expect(view.wrapper.get('[role="combobox"]').text()).toContain('2 locations');
      await view.choose('E:\\');
      await view.choose('Chat');
      expect(view.selected.value).toEqual([]);
    } finally {
      view.wrapper.unmount();
    }
  });

  it('appends native folder selections, preserves selection on cancel, and never emits the action value', async () => {
    const select = vi
      .spyOn(FolderSelectionService, 'select')
      .mockResolvedValueOnce(['E:\\Work', 'F:\\Chat'])
      .mockResolvedValueOnce([]);
    vi.spyOn(FolderSelectionService, 'filterExistingDirectories').mockResolvedValue(['E:\\Work', 'F:\\Chat']);
    const view = mountSelector(true);
    try {
      await view.open();
      await view.choose('E:\\');
      await view.choose('Choose folder…');
      expect(select).toHaveBeenCalledWith(true, 'Choose folder…', 'E:\\');
      expect(view.selected.value).toEqual(['E:\\', 'E:\\Work', 'F:\\Chat']);
      expect(document.querySelector('[role="listbox"]')).toBeNull();
      await view.open();
      await view.choose('Choose folder…');
      expect(view.selected.value).toEqual(['E:\\', 'E:\\Work', 'F:\\Chat']);
    } finally {
      view.wrapper.unmount();
    }
  });

  it('preserves single-selection behavior on other pages', async () => {
    const view = mountSelector(false);
    try {
      await view.open();
      await view.choose('F:\\');
      expect(view.selected.value).toBe('F:\\');
      expect(document.querySelector('[role="listbox"]')).toBeNull();
    } finally {
      view.wrapper.unmount();
    }
  });
});
