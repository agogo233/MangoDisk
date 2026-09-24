<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdIconAction from '@/components/custom/md-icon-action.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import {
  MAX_SCAN_EXCLUDED_FOLDERS,
  SCAN_EXCLUSION_SCOPES,
  type ScanExcludedFolder,
  type ScanExclusionScope,
} from '@/lib/models/storage-scan';
import { ICON_NAMES, TOOLTIP_OPEN_DELAY_MS } from '@/lib/models/ui';
import { FileManagerService } from '@/lib/services/file-manager-service';
import { FolderSelectionService } from '@/lib/services/folder-selection-service';
import { NativeDragDropService, type NativeDragDropEvent } from '@/lib/services/native-drag-drop-service';
import * as PathUtils from '@/lib/utils/path';

const props = defineProps<{
  modelValue: boolean;
  folders: ScanExcludedFolder[];
  saving: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [open: boolean];
  save: [folders: ScanExcludedFolder[]];
  error: [error: unknown];
}>();

const { t } = useI18n({ useScope: 'global' });
const draftFolders = ref<ScanExcludedFolder[]>([]);
const scopes = Object.values(SCAN_EXCLUSION_SCOPES);
// Space analysis starts as a complete view unless the user explicitly opts out of a folder.
const defaultScopes = [
  SCAN_EXCLUSION_SCOPES.cleanup,
  SCAN_EXCLUSION_SCOPES.largeFiles,
  SCAN_EXCLUSION_SCOPES.duplicateFiles,
];
const scopeLabels = {
  cleanup: 'navigation.cleanup',
  largeFiles: 'navigation.large-files',
  duplicateFiles: 'navigation.duplicate-files',
  analysis: 'navigation.analysis',
} as const;
const selecting = ref(false);
const nativeDropActive = ref(false);
const openHelpPath = ref<string | null>(null);
const dropZoneElement = ref<HTMLElement | null>(null);
let stopNativeDropListener: (() => void) | null = null;
let nativeDropListenerMounted = false;
const addDisabled = computed(
  () => props.saving || selecting.value || draftFolders.value.length >= MAX_SCAN_EXCLUDED_FOLDERS
);

watch(
  () => props.modelValue,
  open => {
    if (open) draftFolders.value = props.folders.map(folder => ({ path: folder.path, scopes: [...folder.scopes] }));
    else {
      nativeDropActive.value = false;
      openHelpPath.value = null;
    }
  },
  { immediate: true }
);

async function addFolders() {
  if (addDisabled.value) return;
  selecting.value = true;
  try {
    const selected = await FolderSelectionService.select(true, t('storageScanExclusions.chooseFolders'));
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
  const seen = new Set(draftFolders.value.map(folder => PathUtils.comparisonKey(folder.path)));
  const additions = resolved.filter(path => {
    const key = PathUtils.comparisonKey(path);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  if (draftFolders.value.length + additions.length > MAX_SCAN_EXCLUDED_FOLDERS) {
    emit('error', new Error(t('storageScanExclusions.limitReached', { count: MAX_SCAN_EXCLUDED_FOLDERS })));
    return;
  }
  draftFolders.value = [...draftFolders.value, ...additions.map(path => ({ path, scopes: [...defaultScopes] }))];
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
  draftFolders.value = draftFolders.value.filter(folder => PathUtils.comparisonKey(folder.path) !== key);
  if (openHelpPath.value === path) openHelpPath.value = null;
}

function toggleScope(path: string, scope: ScanExclusionScope, checked: boolean) {
  if (props.saving) return;
  draftFolders.value = draftFolders.value.map(folder => {
    if (PathUtils.comparisonKey(folder.path) !== PathUtils.comparisonKey(path)) return folder;
    const next = checked ? [...folder.scopes, scope] : folder.scopes.filter(value => value !== scope);
    // Each configured folder must affect at least one scan. Removing a folder
    // entirely is a separate, explicit action beside the folder path.
    return next.length ? { ...folder, scopes: scopes.filter(value => next.includes(value)) } : folder;
  });
}

function setHelpOpen(path: string, open: boolean) {
  if (open) openHelpPath.value = path;
  else if (openHelpPath.value === path) openHelpPath.value = null;
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
    <MdDialogContent class="flex min-h-0 flex-col" size="large" @interact-outside="preventOutsideDismiss">
      <MdDialogHeader class="flex-none">
        <DialogTitle>{{ t('storageScanExclusions.title') }}</DialogTitle>
        <DialogDescription>{{ t('storageScanExclusions.description') }}</DialogDescription>
      </MdDialogHeader>

      <div class="exclusion-dialog-body">
        <div class="exclusion-toolbar">
          <p>
            {{ t('storageScanExclusions.folderCount', { count: draftFolders.length }, draftFolders.length) }}
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
            {{ selecting ? t('storageScanExclusions.addingFolder') : t('storageScanExclusions.addFolder') }}
          </Button>
        </div>

        <div
          ref="dropZoneElement"
          class="exclusion-drop-zone"
          :class="{ empty: draftFolders.length === 0, active: nativeDropActive }"
          @dragover.prevent
          @dragenter.prevent
        >
          <div v-if="draftFolders.length" class="exclusion-list">
            <div v-for="folder in draftFolders" :key="PathUtils.comparisonKey(folder.path)" class="exclusion-row">
              <span class="exclusion-folder-mark" aria-hidden="true">
                <MdIcon :name="ICON_NAMES.folder" :size="18" />
              </span>
              <span class="exclusion-path">{{ PathUtils.display(folder.path) }}</span>
              <span class="exclusion-row-actions">
                <MdIconAction
                  appearance="unstyled"
                  :disabled="saving"
                  :label="t('common.showInFileManager')"
                  @click="openFolder(folder.path)"
                >
                  <MdIcon :name="ICON_NAMES.folderOpen" :size="16" />
                </MdIconAction>
                <MdIconAction
                  appearance="unstyled"
                  destructive
                  :disabled="saving"
                  :label="t('storageScanExclusions.removeFolder')"
                  @click="removeFolder(folder.path)"
                >
                  <MdIcon :name="ICON_NAMES.trash" :size="16" />
                </MdIconAction>
              </span>
              <div class="exclusion-scopes">
                <span v-for="scope in scopes" :key="scope" class="exclusion-scope-item">
                  <label class="exclusion-scope">
                    <MdCheckbox
                      :model-value="folder.scopes.includes(scope)"
                      :disabled="saving || (folder.scopes.length === 1 && folder.scopes.includes(scope))"
                      @update:model-value="toggleScope(folder.path, scope, $event === true)"
                    />
                    {{ t(scopeLabels[scope]) }}
                  </label>
                  <TooltipProvider
                    v-if="scope === SCAN_EXCLUSION_SCOPES.cleanup"
                    :delay-duration="TOOLTIP_OPEN_DELAY_MS"
                    :disable-hoverable-content="true"
                    :ignore-non-keyboard-focus="true"
                  >
                    <Tooltip :open="openHelpPath === folder.path" @update:open="setHelpOpen(folder.path, $event)">
                      <TooltipTrigger as-child>
                        <button
                          class="md-help-action"
                          type="button"
                          :aria-label="t('storageScanExclusions.cleanupScopeHint')"
                          @click="setHelpOpen(folder.path, true)"
                        >
                          <MdIcon :name="ICON_NAMES.help" :size="13" aria-hidden="true" />
                        </button>
                      </TooltipTrigger>
                      <TooltipContent
                        class="max-w-[min(24rem,calc(100vw-24px))] text-left whitespace-normal text-wrap [overflow-wrap:anywhere]"
                      >
                        {{ t('storageScanExclusions.cleanupScopeHint') }}
                      </TooltipContent>
                    </Tooltip>
                  </TooltipProvider>
                </span>
              </div>
            </div>
          </div>
          <button v-else class="exclusion-empty-action" type="button" :disabled="addDisabled" @click="addFolders">
            <MdIcon :name="ICON_NAMES.folderPlus" :size="28" />
            <strong>{{ t('storageScanExclusions.emptyTitle') }}</strong>
          </button>
        </div>
      </div>

      <MdDialogFooter class="exclusion-footer">
        <span class="exclusion-footer-actions">
          <Button variant="outline" type="button" :disabled="saving" @click="emit('update:modelValue', false)">
            {{ t('common.cancel') }}
          </Button>
          <Button
            type="button"
            :disabled="saving || selecting"
            @click="
              emit(
                'save',
                draftFolders.map(folder => ({ path: folder.path, scopes: [...folder.scopes] }))
              )
            "
          >
            {{ saving ? t('storageScanExclusions.saving') : t('storageScanExclusions.save') }}
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
  flex: 0 1 auto;
  flex-direction: column;
  gap: 8px;
  padding: 10px 18px 12px;
  border-top: 1px solid var(--border-subtle);
}

.exclusion-toolbar {
  display: flex;
  min-height: 30px;
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
  height: 30px;
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
  height: 240px;
  flex: none;
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
  height: 100%;
  overflow-y: auto;
}

.exclusion-row {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  column-gap: 10px;
  row-gap: 6px;
  padding: 10px 14px;
  transition: background-color 150ms ease;
}

@media (hover: hover) {
  .exclusion-row:hover {
    background: var(--surface-muted-subtle);
  }
}

.exclusion-row:focus-within {
  background: var(--surface-muted-subtle);
}

.exclusion-folder-mark {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: 8px;
  background: var(--surface-primary-subtle);
  color: var(--primary);
}

.exclusion-row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.exclusion-row-actions :deep(.icon-action) {
  display: grid;
  width: 28px;
  height: 28px;
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

.exclusion-path {
  display: block;
  min-width: 0;
  overflow: hidden;
  color: var(--foreground);
  font-size: var(--font-content-body);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.exclusion-scopes {
  grid-column: 2 / -1;
  display: flex;
  flex-wrap: wrap;
  gap: 6px 10px;
  min-width: 0;
}

.exclusion-scope-item {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 2px;
  white-space: nowrap;
}

.exclusion-scope {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 5px;
  color: var(--muted-foreground);
  font-size: var(--font-content-meta);
  cursor: pointer;
  white-space: nowrap;
}

.exclusion-empty-action {
  display: flex;
  width: 100%;
  height: 100%;
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

.exclusion-footer-actions {
  display: flex;
  flex: none;
  gap: 8px;
}
</style>
