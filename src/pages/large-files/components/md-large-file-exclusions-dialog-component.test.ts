// @vitest-environment happy-dom

import { flushPromises, mount } from '@vue/test-utils';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { i18n } from '@/i18n';
import { FileManagerService } from '@/lib/services/file-manager-service';
import { FolderSelectionService } from '@/lib/services/folder-selection-service';

const nativeDropMock = vi.hoisted(() => ({
  listener: undefined as
    ((event: { type: 'drop'; paths: string[]; position: { x: number; y: number } }) => void) | undefined,
}));

vi.mock('@/lib/services/native-drag-drop-service', () => ({
  NativeDragDropService: {
    listen: vi.fn(
      (listener: (event: { type: 'drop'; paths: string[]; position: { x: number; y: number } }) => void) => {
        nativeDropMock.listener = listener;
        return Promise.resolve(() => undefined);
      }
    ),
  },
}));

import MdLargeFileExclusionsDialog from './md-large-file-exclusions-dialog.vue';
import exclusionsDialogSource from './md-large-file-exclusions-dialog.vue?raw';

const passthroughStub = { template: '<div><slot /></div>' };
const dialogContentStub = {
  name: 'MdDialogContent',
  emits: ['interactOutside'],
  template: '<div><slot /></div>',
};
const dialogFooterStub = {
  name: 'MdDialogFooter',
  props: { align: String },
  template: '<footer :data-align="align"><slot /></footer>',
};
const buttonStub = {
  inheritAttrs: false,
  props: { disabled: Boolean },
  emits: ['click'],
  template: '<button :class="$attrs.class" :disabled="disabled" @click="$emit(\'click\', $event)"><slot /></button>',
};
const iconActionStub = {
  props: { label: String, disabled: Boolean },
  emits: ['click'],
  template: '<button :aria-label="label" :disabled="disabled" @click="$emit(\'click\', $event)"><slot /></button>',
};

function mountDialog(folders = ['/fixture/cache', '/fixture/downloads']) {
  return mount(MdLargeFileExclusionsDialog, {
    props: {
      modelValue: true,
      folders,
      saving: false,
      rescanAfterSave: true,
    },
    global: {
      plugins: [i18n],
      stubs: {
        Button: buttonStub,
        Dialog: passthroughStub,
        DialogDescription: passthroughStub,
        DialogTitle: passthroughStub,
        MdDialogContent: dialogContentStub,
        MdDialogFooter: dialogFooterStub,
        MdDialogHeader: passthroughStub,
        MdIcon: true,
        MdIconAction: iconActionStub,
      },
    },
  });
}

describe('large-file exclusions dialog', () => {
  afterEach(() => {
    nativeDropMock.listener = undefined;
    vi.restoreAllMocks();
  });

  it('uses legacy WebKit-safe semantic surfaces for folder interactions', () => {
    expect(exclusionsDialogSource).toContain('background: var(--surface-primary-subtle)');
    expect(exclusionsDialogSource).toContain('background: var(--surface-muted-subtle)');
    expect(exclusionsDialogSource).not.toContain('bg-primary/10');
    expect(exclusionsDialogSource).not.toContain('ring-primary/20');
  });

  it('renders every excluded folder in one bounded scroll region', () => {
    const wrapper = mountDialog();

    expect(wrapper.findAll('.exclusion-row')).toHaveLength(2);
    expect(wrapper.get('.exclusion-list').classes()).toContain('scrollbar-stable');
    expect(wrapper.get('.exclusion-drop-zone').classes()).not.toContain('empty');
    expect(wrapper.text()).toContain('/fixture/cache');
  });

  it('keeps the protection note in the left side of the split footer', () => {
    const wrapper = mountDialog();
    const footer = wrapper.get('footer');

    expect(footer.attributes('data-align')).toBe('between');
    expect(footer.get('.exclusion-note').text()).toContain('System-protected folders are always skipped');
    expect(footer.get('.exclusion-footer-actions').findAll('button')).toHaveLength(2);
  });

  it('removes a folder from the draft and saves the remaining list', async () => {
    const wrapper = mountDialog();

    await wrapper.get('[aria-label="Remove this folder"]').trigger('click');
    await wrapper.findAll('button').at(-1)?.trigger('click');

    expect(wrapper.emitted('save')).toEqual([[['/fixture/downloads']]]);
    expect(wrapper.text()).toContain('Save and Scan Again');
  });

  it('keeps the empty folder region clickable without changing saved data until confirmation', async () => {
    const select = vi.spyOn(FolderSelectionService, 'select').mockResolvedValue([]);
    const wrapper = mountDialog([]);

    expect(wrapper.get('.exclusion-drop-zone').classes()).toContain('empty');
    expect(wrapper.get('.exclusion-empty-action').text()).toContain('Click to add folders, or drag them here');
    await wrapper.get('.exclusion-empty-action').trigger('click');

    expect(select).toHaveBeenCalledOnce();
    expect(wrapper.emitted('save')).toBeUndefined();
  });

  it('adds dropped directories to the same stable-height list region', async () => {
    vi.spyOn(FolderSelectionService, 'filterExistingDirectories').mockResolvedValue(['/fixture/dropped']);
    const wrapper = mountDialog([]);
    await flushPromises();
    vi.spyOn(wrapper.get('.exclusion-drop-zone').element, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      right: 300,
      top: 0,
      bottom: 150,
    } as DOMRect);

    nativeDropMock.listener?.({ type: 'drop', paths: ['/fixture/dropped'], position: { x: 40, y: 40 } });
    await flushPromises();

    expect(wrapper.get('.exclusion-drop-zone').classes()).not.toContain('empty');
    expect(wrapper.get('.exclusion-list').text()).toContain('/fixture/dropped');
  });

  it('ignores a native folder drop outside the list region', async () => {
    const filterDirectories = vi
      .spyOn(FolderSelectionService, 'filterExistingDirectories')
      .mockResolvedValue(['/fixture/dropped']);
    const wrapper = mountDialog([]);
    await flushPromises();
    vi.spyOn(wrapper.get('.exclusion-drop-zone').element, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      right: 300,
      top: 0,
      bottom: 150,
    } as DOMRect);

    nativeDropMock.listener?.({ type: 'drop', paths: ['/fixture/dropped'], position: { x: 400, y: 40 } });
    await flushPromises();

    expect(filterDirectories).not.toHaveBeenCalled();
    expect(wrapper.get('.exclusion-drop-zone').classes()).toContain('empty');
  });

  it('reveals an excluded folder through the shared file manager service', async () => {
    const reveal = vi.spyOn(FileManagerService, 'reveal').mockResolvedValue();
    const wrapper = mountDialog();

    await wrapper.get('[aria-label="Show in File Manager"]').trigger('click');

    expect(reveal).toHaveBeenCalledWith('/fixture/cache');
  });

  it('prevents an outside interaction from dismissing an unsaved draft', () => {
    const wrapper = mountDialog();
    const preventDefault = vi.fn();

    wrapper.getComponent({ name: 'MdDialogContent' }).vm.$emit('interactOutside', { preventDefault });

    expect(preventDefault).toHaveBeenCalledOnce();
    expect(wrapper.emitted('update:modelValue')).toBeUndefined();
  });
});
