<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdIconAction from '@/components/custom/md-icon-action.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import {
  CUSTOM_CLEANUP_PREFERENCES_SCHEMA_VERSION,
  MAX_CUSTOM_CLEANUP_ROOTS_PER_RULE,
  MAX_CUSTOM_CLEANUP_RULES,
  MAX_CUSTOM_CLEANUP_TEXT_LENGTH,
  type CustomCleanupRule,
} from '@/lib/models/cleanup/custom-cleanup';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import { FileManagerService } from '@/lib/services/filesystem/file-manager-service';
import { FolderSelectionService } from '@/lib/services/storage/folder-selection-service';
import { NativeDragDropService, type NativeDragDropEvent } from '@/lib/services/shell/native-drag-drop-service';
import * as PathUtils from '@/lib/utils/filesystem/path';
import * as CustomCleanupPreferenceUtils from '@/lib/utils/cleanup/custom-cleanup-preference';
import { useAppStore } from '@/stores/app-store';
import { useCustomCleanupStore } from '@/stores/custom-cleanup-store';
import { customCleanupDraftFingerprint, customCleanupPersistedState } from '../custom-cleanup-dialog-state';

import MdCustomCleanupRuleEditor from './md-custom-cleanup-rule-editor.vue';
import { cloneCustomCleanupRule } from '@/lib/utils/cleanup/custom-cleanup-draft';

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{
  scan: [rules: CustomCleanupRule[], includeStandardRules: boolean];
  'update:modelValue': [open: boolean];
}>();
const { t } = useI18n({ useScope: 'global' });
const appStore = useAppStore();
const store = useCustomCleanupStore();
const drafts = ref<CustomCleanupRule[]>([]);
const activeRuleId = ref('');
const saving = ref(false);
const includeStandardRules = ref(true);
const validationRequested = ref(false);
const patternExamplesOpen = ref(false);
const directoryDropActive = ref(false);
const savedFingerprint = ref('');
let stopDirectoryDropListener: (() => void) | null = null;
let directoryDropListenerMounted = false;
const activeRule = computed(() => drafts.value.find(rule => rule.id === activeRuleId.value) ?? null);
const parsedRules = computed(() => {
  try {
    return CustomCleanupPreferenceUtils.parse({
      schemaVersion: CUSTOM_CLEANUP_PREFERENCES_SCHEMA_VERSION,
      includeStandardRules: includeStandardRules.value,
      rules: drafts.value,
    }).rules;
  } catch {
    return null;
  }
});
const canSave = computed(() => drafts.value.length === 0 || parsedRules.value !== null);
const draftFingerprint = computed(() => customCleanupDraftFingerprint(drafts.value, includeStandardRules.value));
const isDirty = computed(() => draftFingerprint.value !== savedFingerprint.value);
const invalidRuleIds = computed(() => {
  const ids = new Set<string>();
  for (const rule of drafts.value) {
    try {
      CustomCleanupPreferenceUtils.parse({
        schemaVersion: CUSTOM_CLEANUP_PREFERENCES_SCHEMA_VERSION,
        includeStandardRules: includeStandardRules.value,
        rules: [rule],
      });
    } catch {
      ids.add(rule.id);
    }
  }
  return ids;
});
function replaceDraftRule(rule: CustomCleanupRule) {
  const index = drafts.value.findIndex(draft => draft.id === rule.id);
  if (index >= 0) drafts.value.splice(index, 1, rule);
}

async function reset() {
  await store.initialize();
  includeStandardRules.value = store.includeStandardRules;
  savedFingerprint.value = customCleanupDraftFingerprint(store.rules, store.includeStandardRules);
  drafts.value = store.rules.map(cloneCustomCleanupRule);
  if (!drafts.value.length) drafts.value = [CustomCleanupPreferenceUtils.create()];
  activeRuleId.value = drafts.value[0]?.id ?? '';
  validationRequested.value = false;
  patternExamplesOpen.value = false;
}

function addRule() {
  if (drafts.value.length >= MAX_CUSTOM_CLEANUP_RULES) return;
  const rule = CustomCleanupPreferenceUtils.create();
  drafts.value = [...drafts.value, rule];
  activeRuleId.value = rule.id;
  validationRequested.value = false;
}

function copyRule(ruleId: string, index: number) {
  if (drafts.value.length >= MAX_CUSTOM_CLEANUP_RULES) return;
  const sourceIndex = drafts.value.findIndex(rule => rule.id === ruleId);
  const source = drafts.value[sourceIndex];
  if (!source) return;
  const displayName = source.name.trim() || t('cleanup.customCleanup.untitledRule', { index: index + 1 });
  const copy = cloneCustomCleanupRule(source);
  copy.id = CustomCleanupPreferenceUtils.create().id;
  copy.name = t('cleanup.customCleanup.ruleCopyName', { name: displayName }).slice(0, MAX_CUSTOM_CLEANUP_TEXT_LENGTH);
  drafts.value.splice(sourceIndex + 1, 0, copy);
  activeRuleId.value = copy.id;
}

function removeRule(ruleId: string) {
  const index = drafts.value.findIndex(rule => rule.id === ruleId);
  if (index < 0) return;
  const removingActiveRule = activeRuleId.value === ruleId;
  drafts.value = drafts.value.filter(rule => rule.id !== ruleId);
  if (removingActiveRule) {
    activeRuleId.value = drafts.value[Math.min(index, drafts.value.length - 1)]?.id ?? '';
  }
}

async function addDirectories() {
  const rule = activeRule.value;
  if (!rule) return;
  try {
    const selected = await FolderSelectionService.select(true, t('cleanup.customCleanup.chooseDirectories'));
    await appendDirectories(rule.id, selected);
  } catch (error) {
    appStore.reportError(error);
  }
}

async function appendDirectories(ruleId: string, paths: string[]) {
  const directories = await FolderSelectionService.filterExistingDirectories(paths);
  const rule = drafts.value.find(item => item.id === ruleId);
  if (!rule) return;
  rule.roots = PathUtils.collapseOverlappingRoots([...rule.roots, ...directories.map(PathUtils.display)]).slice(
    0,
    MAX_CUSTOM_CLEANUP_ROOTS_PER_RULE
  );
}

function handleDirectoryDrop(event: NativeDragDropEvent) {
  if (!props.modelValue || !activeRule.value) {
    directoryDropActive.value = false;
    return;
  }
  if (event.type === 'leave') {
    directoryDropActive.value = false;
    return;
  }
  directoryDropActive.value = event.type !== 'drop';
  if (event.type === 'drop') {
    const ruleId = activeRule.value.id;
    void appendDirectories(ruleId, event.paths).catch(error => appStore.reportError(error));
  }
}

async function openDirectory(path: string) {
  try {
    await FileManagerService.reveal(path);
  } catch (error) {
    appStore.reportError(error);
  }
}

function removeDirectory(path: string) {
  const rule = activeRule.value;
  if (!rule) return;
  const key = PathUtils.comparisonKey(path);
  rule.roots = rule.roots.filter(item => PathUtils.comparisonKey(item) !== key);
}

async function persist(scan: boolean) {
  if (!canSave.value) {
    validationRequested.value = true;
    activeRuleId.value = drafts.value.find(rule => invalidRuleIds.value.has(rule.id))?.id ?? drafts.value[0]?.id ?? '';
    return;
  }
  if (scan && !parsedRules.value?.length) return;
  saving.value = true;
  try {
    const rules = parsedRules.value ?? [];
    await store.save(rules, includeStandardRules.value);
    const persisted = customCleanupPersistedState(rules, includeStandardRules.value, scan);
    drafts.value = rules.map(cloneCustomCleanupRule);
    savedFingerprint.value = persisted.fingerprint;
    validationRequested.value = false;
    if (persisted.startScan) {
      emit('scan', rules.map(cloneCustomCleanupRule), includeStandardRules.value);
    }
    if (persisted.closeDialog) {
      emit('update:modelValue', false);
    }
  } catch (error) {
    appStore.reportError(error);
  } finally {
    saving.value = false;
  }
}

watch(
  () => props.modelValue,
  open => {
    if (open) void reset();
    else directoryDropActive.value = false;
  }
);

onMounted(() => {
  directoryDropListenerMounted = true;
  void NativeDragDropService.listen(handleDirectoryDrop)
    .then(stop => {
      if (directoryDropListenerMounted) stopDirectoryDropListener = stop;
      else stop();
    })
    .catch(error => appStore.reportError(error));
});

onBeforeUnmount(() => {
  directoryDropListenerMounted = false;
  stopDirectoryDropListener?.();
  stopDirectoryDropListener = null;
});
</script>

<template>
  <Dialog :open="modelValue" @update:open="emit('update:modelValue', $event)">
    <!-- Keep unsaved rule edits open when the backdrop is clicked accidentally. -->
    <MdDialogContent class="custom-dialog flex min-h-0 flex-col" size="large" @pointer-down-outside.prevent>
      <MdDialogHeader class="flex-none">
        <DialogTitle>{{ t('cleanup.customCleanup.title') }}</DialogTitle>
        <DialogDescription>{{ t('cleanup.customCleanup.description') }}</DialogDescription>
      </MdDialogHeader>

      <div class="custom-dialog-body">
        <aside class="rule-sidebar">
          <div class="rule-list">
            <div
              v-for="(rule, index) in drafts"
              :key="rule.id"
              class="rule-list-item"
              :class="{ active: rule.id === activeRuleId }"
            >
              <button class="rule-list-select" type="button" @click="activeRuleId = rule.id">
                <span>{{ rule.name.trim() || t('cleanup.customCleanup.untitledRule', { index: index + 1 }) }}</span>
                <small>{{
                  t('cleanup.customCleanup.directoryCount', { count: rule.roots.length }, rule.roots.length)
                }}</small>
              </button>
              <span class="rule-list-actions">
                <MdIconAction
                  class="rule-list-action"
                  appearance="unstyled"
                  :label="t('cleanup.customCleanup.copyRule')"
                  :disabled="drafts.length >= MAX_CUSTOM_CLEANUP_RULES"
                  @click="copyRule(rule.id, index)"
                >
                  <MdIcon :name="ICON_NAMES.copy" :size="13" />
                </MdIconAction>
                <MdIconAction
                  class="rule-list-action"
                  appearance="unstyled"
                  destructive
                  :label="t('cleanup.customCleanup.removeRule')"
                  @click="removeRule(rule.id)"
                >
                  <MdIcon :name="ICON_NAMES.trash" :size="13" />
                </MdIconAction>
              </span>
            </div>
          </div>
          <div class="rule-sidebar-footer">
            <Button
              class="add-rule-button"
              variant="ghost"
              type="button"
              :disabled="drafts.length >= MAX_CUSTOM_CLEANUP_RULES"
              @click="addRule"
            >
              <MdIcon :name="ICON_NAMES.folderPlus" :size="15" />
              {{ t('cleanup.customCleanup.addRule') }}
            </Button>
          </div>
        </aside>

        <MdCustomCleanupRuleEditor
          v-if="activeRule"
          v-model:pattern-examples-open="patternExamplesOpen"
          :rule="activeRule"
          :validation-requested="validationRequested"
          :directory-drop-active="directoryDropActive"
          @update:rule="replaceDraftRule"
          @add-directories="addDirectories"
          @open-directory="openDirectory"
          @remove-directory="removeDirectory"
        />

        <div v-else class="empty-rules">
          <p>{{ t('cleanup.customCleanup.empty') }}</p>
          <Button variant="outline" type="button" @click="addRule">{{ t('cleanup.customCleanup.addRule') }}</Button>
        </div>
      </div>

      <MdDialogFooter align="between">
        <label class="standard-scan-option">
          <MdCheckbox v-model="includeStandardRules" />
          <span>{{ t('cleanup.customCleanup.includeStandardRules') }}</span>
        </label>
        <span class="dialog-actions">
          <Button variant="outline" type="button" @click="emit('update:modelValue', false)">
            {{ t('common.cancel') }}
          </Button>
          <Button variant="outline" type="button" :disabled="saving || !isDirty" @click="persist(false)">
            {{ t('cleanup.customCleanup.save') }}
          </Button>
          <Button type="button" :disabled="saving || !drafts.length" @click="persist(true)">
            {{ t('cleanup.customCleanup.saveAndScan') }}
          </Button>
        </span>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
@reference "@assets/main.css";

.custom-dialog {
  min-height: 0;
}

.custom-dialog-body {
  display: grid;
  min-height: 0;
  /*
   * The editor may shrink and scroll when the viewport is short, but it must
   * not grow into unused dialog space. Growing here creates the detached
   * footer divider that made the form look vertically unbalanced.
   */
  flex: 0 1 auto;
  grid-template-columns: 170px minmax(0, 1fr);
  overflow: hidden;
  border-top: 1px solid var(--border-subtle);
}

.rule-sidebar {
  display: flex;
  min-height: 0;
  flex-direction: column;
  overflow: hidden;
  border-right: 1px solid var(--border-subtle);
}

.rule-list {
  display: flex;
  min-height: 0;
  flex: 1 1 0;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
  padding: 7px;
}

.rule-sidebar-footer {
  flex: none;
  border-top: 1px solid var(--border-subtle);
  padding: 4px 7px;
}

.rule-list-item {
  position: relative;
  display: flex;
  min-width: 0;
  align-items: center;
  border-radius: 6px;
  transition: background-color 140ms ease;
}

.rule-list-item:hover {
  background: var(--surface-accent-subtle);
}

.rule-list-item.active {
  @apply bg-accent text-accent-foreground;
}

.rule-list-select {
  display: grid;
  min-width: 0;
  flex: 1;
  cursor: pointer;
  gap: 1px;
  padding: 7px 8px;
  text-align: left;
}

.rule-list-select span {
  overflow: hidden;
  font-size: var(--font-content-primary);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rule-list-select small {
  @apply text-muted-foreground;
  font-size: var(--font-content-secondary);
}

.rule-list-select small {
  padding-right: 52px;
}

.rule-list-actions {
  position: absolute;
  right: 5px;
  bottom: 5px;
  display: flex;
  gap: 1px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 120ms ease;
}

.rule-list-item:hover .rule-list-actions,
.rule-list-item:focus-within .rule-list-actions {
  opacity: 1;
  pointer-events: auto;
}

:deep(.rule-list-action) {
  display: grid;
  width: 24px;
  height: 24px;
  cursor: pointer;
  place-items: center;
  border-radius: 5px;
  color: var(--muted-foreground);
}

:deep(.rule-list-action:hover) {
  @apply text-foreground;
  background: var(--card);
}

:deep(.rule-list-action[aria-disabled='true']) {
  cursor: not-allowed;
  opacity: 0.4;
}

:deep(.rule-list-action.destructive:hover) {
  color: var(--destructive-text);
}

.add-rule-button {
  width: 100%;
  height: 32px;
  justify-content: flex-start;
}

.empty-rules {
  display: grid;
  place-content: center;
  justify-items: center;
  gap: 10px;
  padding: 32px;
  color: var(--muted-foreground);
}

.standard-scan-option {
  display: flex;
  flex: none;
  cursor: pointer;
  align-items: center;
  gap: 7px;
  font-size: var(--font-content-secondary);
  font-weight: var(--font-weight-label);
}

.dialog-actions {
  display: flex;
  margin-left: auto;
  gap: 8px;
}

@container (max-width: 500px) {
  .custom-dialog-body {
    grid-template-columns: 150px minmax(0, 1fr);
  }
}
</style>
