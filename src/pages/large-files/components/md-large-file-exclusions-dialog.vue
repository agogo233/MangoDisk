<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdIconAction from '@/components/custom/md-icon-action.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { MAX_LARGE_FILE_EXCLUDED_FOLDERS } from '@/lib/models/large-file';
import { ICON_NAMES } from '@/lib/models/ui';
import { FileManagerService } from '@/lib/services/file-manager-service';
import { FolderSelectionService } from '@/lib/services/folder-selection-service';
import { NativeDragDropService, type NativeDragDropEvent } from '@/lib/services/native-drag-drop-service';
import * as PathUtils from '@/lib/utils/path';

const props = defineProps<{
  modelValue: boolean;
  folders: string[];
  saving: boolean;
  rescanAfterSave: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [open: boolean];
  save: [folders: string[]];
  error: [error: unknown];
}>();

const { t } = useI18n({ useScope: 'global' });
const draftFolders = ref<string[]>([]);
const selecting = ref(false);
const nativeDropActive = ref(false);
const dropZoneElement = ref<HTMLElement | null>(null);
let stopNativeDropListener: (() => void) | null = null;
let nativeDropListenerMounted = false;
const addDisabled = computed(
  () => props.saving || selecting.value || draftFolders.value.length >= MAX_LARGE_FILE_EXCLUDED_FOLDERS
);

watch(
  () => props.modelValue,
  open => {
    if (open) draftFolders.value = [...props.folders];
    else nativeDropActive.value = false;
  },
  { immediate: true }
);

async function addFolders() {
  if (addDisabled.value) return;
  selecting.value = true;
  try {
    const selected = await FolderSelectionService.select(true, t('largeFiles.exclusions.chooseFolders'));
    await appendFolders(selected);
  } catch (error) {
    emit('error', error);
  } finally {
    selecting.value = false;
  }
}

async function appendFolders(paths: string[]) {
  if (!paths.length) return;
  const resolved = await FolderSelectionService.filterExistingDirectories(paths);
  const merged = PathUtils.collapseOverlappingRoots([...draftFolders.value, ...resolved]);
  if (merged.length > MAX_LARGE_FILE_EXCLUDED_FOLDERS) {
    emit('error', new Error(t('largeFiles.exclusions.limitReached', { count: MAX_LARGE_FILE_EXCLUDED_FOLDERS })));
    return;
  }
  draftFolders.value = merged;
}

function handleNativeDrop(event: NativeDragDropEvent) {
  if (!props.modelValue || addDisabled.value) {
    nativeDropActive.value = false;
    return;
  }
  if (event.type === 'leave') {
    nativeDropActive.value = false;
    return;
  }
  const dropZone = dropZoneElement.value;
  const scale = window.devicePixelRatio || 1;
  const rect = dropZone?.getBoundingClientRect();
  const x = event.position.x / scale;
  const y = event.position.y / scale;
  // Tauri reports physical window coordinates while the DOM uses logical CSS
  // pixels. Restricting the native event to this rectangle prevents a folder
  // dropped elsewhere in the modal from being accepted unexpectedly.
  const withinDropZone = Boolean(rect && x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom);
  nativeDropActive.value = withinDropZone && event.type !== 'drop';
  if (event.type !== 'drop' || !withinDropZone) return;
  selecting.value = true;
  void appendFolders(event.paths)
    .catch(error => emit('error', error))
    .finally(() => {
      selecting.value = false;
    });
}

function removeFolder(path: string) {
  if (props.saving) return;
  const key = PathUtils.comparisonKey(path);
  draftFolders.value = draftFolders.value.filter(folder => PathUtils.comparisonKey(folder) !== key);
}

async function openFolder(path: string) {
  try {
    await FileManagerService.reveal(path);
  } catch (error) {
    emit('error', error);
  }
}

function preventOutsideDismiss(event: Event) {
  // An exclusion remains a draft until the explicit footer action saves it.
  // Preventing overlay dismissal avoids silently losing several folder choices.
  event.preventDefault();
}

onMounted(() => {
  nativeDropListenerMounted = true;
  void NativeDragDropService.listen(handleNativeDrop)
    .then(stop => {
      if (nativeDropListenerMounted) stopNativeDropListener = stop;
      else stop();
    })
    .catch(error => emit('error', error));
});

onBeforeUnmount(() => {
  nativeDropListenerMounted = false;
  stopNativeDropListener?.();
  stopNativeDropListener = null;
});
</script>

<template>
  <Dialog :open="modelValue" @update:open="emit('update:modelValue', $event)">
    <MdDialogContent class="flex min-h-0 flex-col" size="standard" @interact-outside="preventOutsideDismiss">
      <MdDialogHeader class="flex-none">
        <DialogTitle>{{ t('largeFiles.exclusions.title') }}</DialogTitle>
        <DialogDescription>{{ t('largeFiles.exclusions.description') }}</DialogDescription>
      </MdDialogHeader>

      <div class="exclusion-dialog-body">
        <div class="exclusion-toolbar">
          <p>
            {{ t('largeFiles.exclusions.folderCount', { count: draftFolders.length }, draftFolders.length) }}
          </p>
          <Button
            class="exclusion-add-button"
            variant="ghost"
            size="sm"
            type="button"
            :disabled="addDisabled"
            @click="addFolders"
          >
            <MdIcon :name="ICON_NAMES.folderPlus" :size="15" />
            {{ selecting ? t('largeFiles.exclusions.addingFolder') : t('largeFiles.exclusions.addFolder') }}
          </Button>
        </div>

        <div
          ref="dropZoneElement"
          class="exclusion-drop-zone"
          :class="{ empty: draftFolders.length === 0, active: nativeDropActive }"
          @dragover.prevent
          @dragenter.prevent
        >
          <div v-if="draftFolders.length" class="exclusion-list scrollbar-stable">
            <div v-for="folder in draftFolders" :key="PathUtils.comparisonKey(folder)" class="exclusion-row">
              <MdIcon class="exclusion-folder-icon" :name="ICON_NAMES.folder" :size="18" />
              <span class="exclusion-path">{{ PathUtils.display(folder) }}</span>
              <span class="exclusion-row-actions">
                <MdIconAction
                  appearance="unstyled"
                  :disabled="saving"
                  :label="t('common.showInFileManager')"
                  @click="openFolder(folder)"
                >
                  <MdIcon :name="ICON_NAMES.folderOpen" :size="16" />
                </MdIconAction>
                <MdIconAction
                  appearance="unstyled"
                  destructive
                  :disabled="saving"
                  :label="t('largeFiles.exclusions.removeFolder')"
                  @click="removeFolder(folder)"
                >
                  <MdIcon :name="ICON_NAMES.trash" :size="16" />
                </MdIconAction>
              </span>
            </div>
          </div>
          <button v-else class="exclusion-empty-action" type="button" :disabled="addDisabled" @click="addFolders">
            <MdIcon :name="ICON_NAMES.folderPlus" :size="28" />
            <strong>{{ t('largeFiles.exclusions.emptyTitle') }}</strong>
            <span>{{ t('largeFiles.exclusions.emptyDescription') }}</span>
          </button>
        </div>
      </div>

      <MdDialogFooter class="exclusion-footer" align="between">
        <p class="exclusion-note">
          <MdIcon :name="ICON_NAMES.shield" :size="15" />
          <span>{{ t('largeFiles.exclusions.systemProtectionNote') }}</span>
        </p>
        <span class="exclusion-footer-actions">
          <Button variant="outline" type="button" :disabled="saving" @click="emit('update:modelValue', false)">
            {{ t('common.cancel') }}
          </Button>
          <Button type="button" :disabled="saving" @click="emit('save', [...draftFolders])">
            {{
              saving
                ? t('largeFiles.exclusions.saving')
                : rescanAfterSave
                  ? t('largeFiles.exclusions.saveAndScan')
                  : t('largeFiles.exclusions.save')
            }}
          </Button>
        </span>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
@reference "@assets/main.css";

.exclusion-dialog-body {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 12px;
  padding: 16px 18px;
  border-top: 1px solid var(--border-subtle);
}

.exclusion-toolbar {
  display: flex;
  min-height: 32px;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.exclusion-toolbar p {
  margin: 0;
  color: var(--muted-foreground);
  font-size: var(--font-content-meta);
}

.exclusion-toolbar :deep(.exclusion-add-button) {
  flex: none;
  height: 32px;
  gap: 6px;
  padding: 0 9px;
  border: 1px solid transparent;
  border-radius: 6px;
  box-shadow: none;
  color: var(--muted-foreground);
  font-size: 12px;
  font-weight: 500;
  transition:
    color 150ms ease,
    background-color 150ms ease,
    border-color 150ms ease;
}

@media (hover: hover) {
  .exclusion-toolbar :deep(.exclusion-add-button:hover) {
    border-color: var(--border-subtle);
    background: var(--surface-muted-subtle);
    color: var(--foreground);
  }
}

.exclusion-drop-zone {
  min-height: 150px;
  max-height: min(320px, 42dvh);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  overflow: hidden;
  transition:
    border-color 150ms ease,
    background-color 150ms ease,
    box-shadow 150ms ease;
}

.exclusion-drop-zone.empty {
  border-style: dashed;
}

.exclusion-drop-zone.active {
  border-color: var(--primary);
  background: var(--surface-primary-subtle);
  box-shadow: 0 0 0 2px var(--border-primary-subtle);
}

.exclusion-list {
  min-height: 148px;
  max-height: min(318px, calc(42dvh - 2px));
  overflow-y: auto;
}

.exclusion-row {
  display: grid;
  grid-template-columns: 24px minmax(0, 1fr) auto;
  min-height: 48px;
  align-items: center;
  gap: 8px;
  padding: 8px 10px 8px 12px;
}

.exclusion-row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.exclusion-row-actions :deep(.icon-action) {
  display: grid;
  width: 30px;
  height: 30px;
  place-items: center;
  border-radius: 6px;
  color: var(--muted-foreground);
}

.exclusion-row-actions :deep(.icon-action:not([aria-disabled='true']):hover) {
  background: var(--muted);
  color: var(--foreground);
}

.exclusion-row-actions :deep(.icon-action.destructive:not([aria-disabled='true']):hover) {
  color: var(--destructive);
}

.exclusion-row + .exclusion-row {
  border-top: 1px solid var(--border-subtle);
}

.exclusion-folder-icon,
.exclusion-note {
  color: var(--muted-foreground);
}

.exclusion-path {
  min-width: 0;
  overflow: hidden;
  color: var(--foreground);
  font-size: var(--font-content-body);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.exclusion-empty-action {
  display: flex;
  width: 100%;
  min-height: 148px;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  cursor: pointer;
  border-radius: inherit;
  color: var(--muted-foreground);
  text-align: center;
  transition: background-color 150ms ease;
}

/*
 * Safari 15.6 cannot evaluate Tailwind's color-mix() opacity output. Using the
 * shared semantic surface prevents the whole empty action from becoming a
 * solid primary-color block while preserving the same hover affordance.
 */
@media (hover: hover) {
  .exclusion-empty-action:hover:not(:disabled) {
    background: var(--surface-primary-subtle);
  }
}

.exclusion-empty-action:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -3px;
}

.exclusion-empty-action:disabled {
  cursor: default;
  opacity: 0.5;
}

.exclusion-empty-action strong {
  color: var(--primary);
  font-size: var(--font-content-body);
  font-weight: 500;
}

.exclusion-empty-action span,
.exclusion-note {
  font-size: var(--font-content-meta);
}

.exclusion-note {
  display: flex;
  min-width: 0;
  max-width: 58%;
  flex: 1 1 auto;
  align-items: center;
  gap: 6px;
  margin: 0;
  line-height: 1.4;
}

.exclusion-note > :first-child {
  flex: none;
}

.exclusion-footer-actions {
  display: flex;
  flex: none;
  gap: 8px;
}
</style>
