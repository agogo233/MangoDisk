<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import MdIconAction from '@/components/custom/md-icon-action.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  MAX_CUSTOM_CLEANUP_FILTER_DAYS,
  MAX_CUSTOM_CLEANUP_TEXT_LENGTH,
  type CustomCleanupRule,
} from '@/lib/models/cleanup/custom-cleanup';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import {
  customCleanupRuleValidation,
  customCleanupPatternsFromText,
  customCleanupBytesFromMegabytes,
  customCleanupMegabytesFromBytes,
} from '@/lib/utils/cleanup/custom-cleanup-draft';

const props = defineProps<{ rule: CustomCleanupRule; validationRequested: boolean; directoryDropActive: boolean }>();
const patternExamplesOpen = defineModel<boolean>('patternExamplesOpen', { required: true });
const emit = defineEmits<{
  'update:rule': [rule: CustomCleanupRule];
  addDirectories: [];
  openDirectory: [path: string];
  removeDirectory: [path: string];
}>();
const { t } = useI18n({ useScope: 'global' });
const activeRule = computed(() => props.rule);
const validation = computed(() => customCleanupRuleValidation(props.rule));

function patchRule(patch: Partial<CustomCleanupRule>) {
  emit('update:rule', { ...props.rule, ...patch });
}
function setPatterns(value: string | undefined) {
  patchRule({ namePatterns: customCleanupPatternsFromText(value) });
}
function sizeInMb(bytes: number | null): string {
  return customCleanupMegabytesFromBytes(bytes);
}
function setSize(kind: 'minimumBytes' | 'maximumBytes', value: string | undefined) {
  patchRule({ [kind]: customCleanupBytesFromMegabytes(value) });
}
function setModifiedMode(value: unknown) {
  if (!['any', 'olderThan', 'newerThan'].includes(String(value))) return;
  const mode = String(value) as 'any' | 'olderThan' | 'newerThan';
  patchRule({ modifiedTime: mode === 'any' ? { mode } : { mode, days: 30 } });
}
function setModifiedDays(value: string | undefined) {
  if (props.rule.modifiedTime.mode === 'any') return;
  const days = Number(value ?? '');
  patchRule({ modifiedTime: { ...props.rule.modifiedTime, days: Number.isSafeInteger(days) ? days : 0 } });
}
function modifiedDays(rule: CustomCleanupRule): string {
  return rule.modifiedTime.mode === 'any' ? '' : String(rule.modifiedTime.days);
}
</script>

<template>
  <div class="rule-editor scrollbar-stable-end">
    <div class="field">
      <span class="field-heading">
        <label :for="`custom-rule-name-${activeRule.id}`">{{ t('cleanup.customCleanup.ruleName') }}</label>
        <small v-if="validationRequested && validation.missingName" class="field-error">
          {{ t('cleanup.customCleanup.ruleNameRequired') }}
        </small>
      </span>
      <Input
        :id="`custom-rule-name-${activeRule.id}`"
        :model-value="activeRule.name"
        class="compact-control"
        :class="{ invalid: validationRequested && validation.missingName }"
        :maxlength="MAX_CUSTOM_CLEANUP_TEXT_LENGTH"
        :aria-invalid="validationRequested && validation.missingName"
        :placeholder="t('cleanup.customCleanup.ruleNamePlaceholder')"
        @update:model-value="patchRule({ name: $event ?? '' })"
      />
    </div>

    <div class="directory-section">
      <div class="directory-header">
        <span class="directory-title">
          <span class="field-label">{{ t('cleanup.customCleanup.directories') }}</span>
          <small v-if="validationRequested && validation.missingDirectories" class="field-error">
            {{ t('cleanup.customCleanup.directoriesRequired') }}
          </small>
          <small>{{ t('cleanup.customCleanup.directoryCount', { count: activeRule.roots.length }) }}</small>
        </span>
        <Button
          v-if="activeRule.roots.length"
          class="directory-header-action"
          variant="ghost"
          type="button"
          @click="emit('addDirectories')"
        >
          <MdIcon :name="ICON_NAMES.folderPlus" :size="14" />
          {{ t('cleanup.customCleanup.chooseDirectories') }}
        </Button>
      </div>
      <div
        class="directory-drop-zone"
        :class="{
          active: directoryDropActive,
          invalid: validationRequested && validation.missingDirectories,
        }"
      >
        <div v-if="activeRule.roots.length" class="directory-list scrollbar-stable-end">
          <div v-for="path in activeRule.roots" :key="path" class="directory-item">
            <span class="directory-path">{{ path }}</span>
            <span class="directory-item-actions">
              <MdIconAction
                class="directory-item-action"
                appearance="unstyled"
                :label="t('common.showInFileManager')"
                @click="emit('openDirectory', path)"
              >
                <MdIcon :name="ICON_NAMES.folderOpen" :size="14" />
              </MdIconAction>
              <MdIconAction
                class="directory-item-action"
                appearance="unstyled"
                destructive
                :label="t('cleanup.customCleanup.removeDirectory')"
                @click="emit('removeDirectory', path)"
              >
                <MdIcon :name="ICON_NAMES.close" :size="14" />
              </MdIconAction>
            </span>
          </div>
        </div>
        <div v-else class="empty-directories">
          <Button class="choose-directory-button" variant="ghost" type="button" @click="emit('addDirectories')">
            <MdIcon :name="ICON_NAMES.folderPlus" :size="15" />
            {{ t('cleanup.customCleanup.chooseDirectories') }}
          </Button>
          <span>{{ t('cleanup.customCleanup.dropDirectories') }}</span>
        </div>
      </div>
    </div>

    <div class="field">
      <span class="field-heading">
        <label :for="`custom-rule-pattern-${activeRule.id}`">{{ t('cleanup.customCleanup.patterns') }}</label>
        <small
          v-if="validationRequested && (validation.missingPatterns || validation.patternsInvalid)"
          class="field-error"
        >
          {{
            t(
              validation.missingPatterns
                ? 'cleanup.customCleanup.patternsRequired'
                : 'cleanup.customCleanup.patternsInvalid'
            )
          }}
        </small>
        <Tooltip v-model:open="patternExamplesOpen">
          <TooltipTrigger as-child>
            <button class="pattern-example-trigger" type="button" @click="patternExamplesOpen = !patternExamplesOpen">
              <MdIcon :name="ICON_NAMES.help" :size="13" />
              {{ t('cleanup.customCleanup.viewPatternExamples') }}
            </button>
          </TooltipTrigger>
          <TooltipContent class="grid w-72 gap-2 px-3 py-2 text-left" side="bottom" align="end">
            <strong class="font-semibold">{{ t('cleanup.customCleanup.patternExamplesTitle') }}</strong>
            <span class="flex items-baseline gap-2">
              <code class="w-24 shrink-0 font-mono font-semibold">*.log</code>
              <span>{{ t('cleanup.customCleanup.patternExampleExtension') }}</span>
            </span>
            <span class="flex items-baseline gap-2">
              <code class="w-24 shrink-0 font-mono font-semibold">cache-?.tmp</code>
              <span>{{ t('cleanup.customCleanup.patternExampleCharacter') }}</span>
            </span>
            <span class="flex items-baseline gap-2">
              <code class="w-24 shrink-0 font-mono font-semibold">*.log, *.tmp</code>
              <span>{{ t('cleanup.customCleanup.patternExampleMultiple') }}</span>
            </span>
          </TooltipContent>
        </Tooltip>
      </span>
      <Input
        :id="`custom-rule-pattern-${activeRule.id}`"
        class="compact-control"
        :class="{ invalid: validationRequested && (validation.missingPatterns || validation.patternsInvalid) }"
        :aria-invalid="validationRequested && (validation.missingPatterns || validation.patternsInvalid)"
        :model-value="activeRule.namePatterns.join(', ')"
        :placeholder="t('cleanup.customCleanup.patternsPlaceholder')"
        @update:model-value="setPatterns"
      />
      <small>{{ t('cleanup.customCleanup.patternsHint') }}</small>
    </div>

    <div class="filter-fields">
      <div class="field size-field">
        <label :for="`custom-rule-minimum-${activeRule.id}`">{{ t('cleanup.customCleanup.minimumSize') }}</label>
        <Input
          :id="`custom-rule-minimum-${activeRule.id}`"
          class="compact-control"
          :class="{ invalid: validationRequested && validation.sizeRangeInvalid }"
          type="number"
          min="0"
          :aria-invalid="validationRequested && validation.sizeRangeInvalid"
          :model-value="sizeInMb(activeRule.minimumBytes)"
          :placeholder="t('cleanup.customCleanup.noLimit')"
          @update:model-value="setSize('minimumBytes', $event)"
        />
      </div>
      <div class="field size-field">
        <span class="field-heading">
          <label :for="`custom-rule-maximum-${activeRule.id}`">{{ t('cleanup.customCleanup.maximumSize') }}</label>
          <small v-if="validationRequested && validation.sizeRangeInvalid" class="field-error">
            {{ t('cleanup.customCleanup.sizeRangeInvalid') }}
          </small>
        </span>
        <Input
          :id="`custom-rule-maximum-${activeRule.id}`"
          class="compact-control"
          :class="{ invalid: validationRequested && validation.sizeRangeInvalid }"
          type="number"
          min="0"
          :aria-invalid="validationRequested && validation.sizeRangeInvalid"
          :model-value="sizeInMb(activeRule.maximumBytes)"
          :placeholder="t('cleanup.customCleanup.noLimit')"
          @update:model-value="setSize('maximumBytes', $event)"
        />
      </div>
      <div class="field modified-field">
        <span class="modified-labels">
          <span class="field-heading">
            <span class="field-label">{{ t('cleanup.customCleanup.modifiedTime') }}</span>
            <small v-if="validationRequested && validation.modifiedTimeInvalid" class="field-error">
              {{ t('cleanup.customCleanup.modifiedTimeInvalid') }}
            </small>
          </span>
          <label
            :class="{ 'days-field-hidden': activeRule.modifiedTime.mode === 'any' }"
            :for="`custom-rule-days-${activeRule.id}`"
          >
            {{ t('cleanup.customCleanup.days') }}
          </label>
        </span>
        <div class="modified-controls">
          <Select :model-value="activeRule.modifiedTime.mode" @update:model-value="setModifiedMode">
            <SelectTrigger
              class="compact-control modified-select"
              :class="{ invalid: validationRequested && validation.modifiedTimeInvalid }"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="any">{{ t('cleanup.customCleanup.modifiedAny') }}</SelectItem>
              <SelectItem value="olderThan">{{ t('cleanup.customCleanup.modifiedOlder') }}</SelectItem>
              <SelectItem value="newerThan">{{ t('cleanup.customCleanup.modifiedNewer') }}</SelectItem>
            </SelectContent>
          </Select>
          <Input
            :id="`custom-rule-days-${activeRule.id}`"
            class="compact-control"
            :class="{
              'days-field-hidden': activeRule.modifiedTime.mode === 'any',
              invalid: validationRequested && validation.modifiedTimeInvalid,
            }"
            type="number"
            min="1"
            :max="MAX_CUSTOM_CLEANUP_FILTER_DAYS"
            :aria-invalid="validationRequested && validation.modifiedTimeInvalid"
            :disabled="activeRule.modifiedTime.mode === 'any'"
            :model-value="modifiedDays(activeRule)"
            @update:model-value="setModifiedDays"
          />
        </div>
      </div>
    </div>

    <div class="rule-options">
      <div class="rule-option">
        <MdCheckbox
          :id="`custom-rule-recursive-${activeRule.id}`"
          :model-value="activeRule.recursive"
          @update:model-value="patchRule({ recursive: Boolean($event) })"
        />
        <label :for="`custom-rule-recursive-${activeRule.id}`">
          {{ t('cleanup.customCleanup.includeSubdirectories') }}
        </label>
        <Tooltip>
          <TooltipTrigger as-child>
            <button class="md-help-action" type="button">
              <MdIcon :name="ICON_NAMES.help" :size="13" />
              <span class="sr-only">{{ t('cleanup.customCleanup.includeSubdirectoriesDescription') }}</span>
            </button>
          </TooltipTrigger>
          <TooltipContent class="max-w-64 text-left" side="top">
            {{ t('cleanup.customCleanup.includeSubdirectoriesDescription') }}
          </TooltipContent>
        </Tooltip>
      </div>
      <div class="rule-option">
        <MdCheckbox
          :id="`custom-rule-remove-empty-${activeRule.id}`"
          :model-value="activeRule.removeEmptyDirectories"
          @update:model-value="patchRule({ removeEmptyDirectories: Boolean($event) })"
        />
        <label :for="`custom-rule-remove-empty-${activeRule.id}`">
          {{ t('cleanup.customCleanup.removeEmptyDirectories') }}
        </label>
        <Tooltip>
          <TooltipTrigger as-child>
            <button class="md-help-action" type="button">
              <MdIcon :name="ICON_NAMES.help" :size="13" />
              <span class="sr-only">{{ t('cleanup.customCleanup.removeEmptyDirectoriesDescription') }}</span>
            </button>
          </TooltipTrigger>
          <TooltipContent class="max-w-64 text-left" side="top">
            {{ t('cleanup.customCleanup.removeEmptyDirectoriesDescription') }}
          </TooltipContent>
        </Tooltip>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";

.field small {
  @apply text-muted-foreground;
  font-size: var(--font-content-secondary);
}

.rule-editor {
  display: grid;
  min-height: 0;
  align-content: start;
  container-type: inline-size;
  grid-template-columns: minmax(0, 1fr);
  gap: 10px;
  overflow-y: auto;
  padding: 10px 14px;
}

.field {
  display: grid;
  gap: 4px;
}

.field-heading {
  display: flex;
  min-width: 0;
  min-height: 17px;
  align-items: baseline;
  gap: 7px;
}

.field label,
.field-label {
  font-size: var(--font-content-secondary);
  font-weight: var(--font-weight-label);
}

.pattern-example-trigger {
  @apply text-muted-foreground;
  display: inline-flex;
  flex: none;
  margin-left: auto;
  cursor: pointer;
  align-items: center;
  gap: 4px;
  font-size: var(--font-content-secondary);
  transition: color 140ms ease;
}

.pattern-example-trigger:hover,
.pattern-example-trigger:focus-visible {
  color: var(--foreground);
}

.directory-section {
  display: grid;
  gap: 4px;
}

.directory-header {
  display: flex;
  min-height: 26px;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.directory-title {
  display: flex;
  min-width: 0;
  align-items: baseline;
  gap: 8px;
}

.directory-header small,
.empty-directories {
  @apply text-muted-foreground;
  font-size: var(--font-content-secondary);
}

.directory-header-action {
  height: 28px;
  padding-inline: 7px;
  font-size: var(--font-content-secondary);
}

.directory-drop-zone {
  display: flex;
  height: 126px;
  min-height: 126px;
  flex-direction: column;
  overflow: hidden;
  border-color: var(--border-subtle);
  border-width: 1px;
  border-radius: 7px;
  background: transparent;
  transition:
    border-color 140ms ease,
    background-color 140ms ease;
}

.directory-drop-zone.active {
  border-color: var(--primary);
  background: var(--surface-primary-subtle);
}

.directory-drop-zone.invalid {
  border-color: var(--destructive-border);
}

.directory-list {
  display: grid;
  min-height: 0;
  flex: 1;
  align-content: start;
  overflow-y: auto;
  padding: 4px 8px;
}

.directory-item {
  @apply text-muted-foreground;
  display: grid;
  min-height: 28px;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 6px;
  border-color: var(--border-subtle);
  border-bottom-width: 1px;
  padding: 3px 2px 3px 5px;
  font-size: var(--font-content-secondary);
}

.directory-item:last-child {
  border-bottom-width: 0;
}

.directory-path {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.directory-item-actions {
  display: flex;
  gap: 1px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 120ms ease;
}

:deep(.directory-item-action) {
  display: grid;
  width: 24px;
  height: 24px;
  cursor: pointer;
  place-items: center;
  border-radius: 5px;
}

:deep(.directory-item-action:hover) {
  @apply bg-accent text-foreground;
}

:deep(.directory-item-action.destructive:hover) {
  color: var(--destructive-text);
}

.directory-item:hover .directory-item-actions,
.directory-item:focus-within .directory-item-actions {
  opacity: 1;
  pointer-events: auto;
}

.choose-directory-button {
  width: fit-content;
  height: 32px;
  padding-inline: 8px;
}

.filter-fields {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.compact-control {
  width: 100%;
  min-width: 0;
  height: 32px;
}

.compact-control.invalid {
  border-color: var(--destructive-border);
}

/*
 * Older WebKit exposes platform steppers that do not follow the compact field
 * geometry. Numeric values remain keyboard-editable, while hiding the native
 * controls keeps the editor consistent across supported macOS versions.
 */
.compact-control[type='number'] {
  -webkit-appearance: none;
  appearance: textfield;
}

.compact-control[type='number']::-webkit-inner-spin-button,
.compact-control[type='number']::-webkit-outer-spin-button {
  margin: 0;
  -webkit-appearance: none;
  appearance: none;
}

.modified-select {
  background: transparent;
}

.modified-field {
  grid-column: 1 / -1;
}

.modified-labels,
.modified-controls {
  display: grid;
  min-width: 0;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.days-field-hidden {
  visibility: hidden;
}

.modified-labels label {
  font-size: var(--font-content-secondary);
  font-weight: var(--font-weight-label);
}

.empty-directories {
  display: flex;
  min-height: 0;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 12px;
}

.rule-options {
  display: flex;
  min-height: 24px;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 24px;
}

.rule-option {
  display: inline-flex;
  min-width: 0;
  align-items: center;
  gap: 7px;
}

.rule-option label {
  cursor: pointer;
  font-size: var(--font-content-secondary);
  font-weight: var(--font-weight-label);
}

.field-error {
  overflow: hidden;
  color: var(--destructive-text);
  font-size: var(--font-content-secondary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field-heading .field-error,
.directory-title .field-error {
  color: var(--destructive-text);
}

@container (max-width: 500px) {
  .filter-fields {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
