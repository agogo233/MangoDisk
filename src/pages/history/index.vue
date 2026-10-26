<script setup lang="ts">
import { FormatService } from '@/lib/services/presentation/format-service';
import { useI18n } from 'vue-i18n';
import { ref } from 'vue';

import MdEmptyState from '@/components/custom/md-empty-state.vue';
import MdDestructiveActionDialog from '@/components/custom/md-destructive-action-dialog.vue';
import MdPageShell from '@/components/custom/md-page-shell.vue';
import MdResultTable from '@/components/custom/md-result-table.vue';
import MdResultTableRow from '@/components/custom/md-result-table-row.vue';
import MdResultWorkspace from '@/components/custom/md-result-workspace.vue';
import MdStatusBadge from '@/components/custom/md-status-badge.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import { ICON_NAMES, type IconName } from '@/lib/models/presentation/ui';
import type { PresentedOperationRecord } from '@/lib/models/history/history';
import { ByteSizeService } from '@/lib/services/settings/byte-size-service';

import MdHistoryDetailDialog from './components/md-history-detail-dialog.vue';
import { usesItemCounts, displayedReleasedBytes } from '@/lib/utils/history/operation-record';

const { locale, t } = useI18n({ useScope: 'global' });

defineProps<{ history: PresentedOperationRecord[]; busy: boolean }>();
const emit = defineEmits<{ clear: [] }>();

const detailOpen = ref(false);
const clearConfirmOpen = ref(false);
const selectedRecord = ref<PresentedOperationRecord | null>(null);

function openDetails(record: PresentedOperationRecord) {
  selectedRecord.value = record;
  detailOpen.value = true;
}

function operationTitle(record: PresentedOperationRecord): string {
  return t(`history.categories.${record.category}`);
}

function operationIcon(record: PresentedOperationRecord): IconName {
  if (record.category === 'deepCleanup') return ICON_NAMES.deepCleanup;
  if (record.category === 'largeFileCleanup') return ICON_NAMES.largeFiles;
  if (record.category === 'duplicateFileCleanup') return ICON_NAMES.duplicateFiles;
  if (record.category === 'startupManagement') return ICON_NAMES.startup;
  if (record.category === 'systemOptimization') return ICON_NAMES.systemOptimization;
  if (record.category === 'privacyCleanup') return ICON_NAMES.shield;
  return ICON_NAMES.uninstall;
}

function confirmClearHistory() {
  clearConfirmOpen.value = false;
  emit('clear');
}

function selectedItemCount(record: PresentedOperationRecord): number {
  return record.selectedItemCount;
}

function recordSummary(record: PresentedOperationRecord): string {
  if (record.category === 'startupManagement') {
    return t('history.startupRecordSummary', {
      selected: FormatService.integer(record.selectedItemCount),
      changed: FormatService.integer(record.affectedItemCount),
    });
  }
  if (record.category === 'systemOptimization') {
    return t('history.systemOptimizationRecordSummary', {
      selected: FormatService.integer(record.selectedItemCount),
      changed: FormatService.integer(record.affectedItemCount),
    });
  }
  if (record.category === 'privacyCleanup') {
    return t('history.privacyRecordSummary', {
      selected: FormatService.integer(record.selectedItemCount),
      changed: FormatService.integer(record.affectedItemCount),
    });
  }
  const key = record.category === 'applicationUninstall' ? 'history.uninstallRecordSummary' : 'history.recordSummary';
  return t(
    key,
    {
      items: FormatService.integer(selectedItemCount(record)),
      files: FormatService.integer(record.affectedItemCount),
    },
    record.category === 'applicationUninstall' ? record.selectedItemCount : record.affectedItemCount
  );
}
</script>

<template>
  <MdPageShell class="@container/history" content-mode="workspace" content-width="wide" :title="t('history.title')">
    <template v-if="history.length" #actions>
      <Button
        class="clear-history-button"
        variant="ghost"
        type="button"
        :disabled="busy"
        @click="clearConfirmOpen = true"
      >
        <MdIcon :name="ICON_NAMES.trash" :size="14" />
        {{ t('history.clear') }}
      </Button>
    </template>

    <MdResultWorkspace v-if="!history.length" class="history-empty-workspace">
      <MdEmptyState
        :icon-name="ICON_NAMES.history"
        :title="t('history.emptyTitle')"
        :description="t('history.emptyDescription')"
      />
    </MdResultWorkspace>

    <MdResultWorkspace v-else>
      <MdResultTable class="history-list" header-variant="plain">
        <template #header>
          <div class="history-list-header" aria-hidden="true">
            <span>{{ t('history.operation') }}</span>
            <span>{{ t('history.planned') }}</span>
            <span>{{ t('history.resultSpace') }}</span>
          </div>
        </template>
        <MdResultTableRow v-for="record in history" :key="record.operationId" class="history-record-row">
          <button class="record" type="button" @click="openDetails(record)">
            <span class="record-icon">
              <MdIcon :name="record.dryRun ? ICON_NAMES.search : operationIcon(record)" :size="18" />
            </span>
            <span class="record-main">
              <span>
                <strong class="md-result-primary">{{ operationTitle(record) }}</strong>
                <MdStatusBadge v-if="record.failedItemCount" size="compact" tone="warning">
                  {{ t('history.statusWarnings') }}
                </MdStatusBadge>
              </span>
              <small>
                {{ FormatService.dateTime(record.startedAtMs, locale) }} ·
                {{ recordSummary(record) }}
              </small>
            </span>
            <strong class="record-byte">
              {{
                usesItemCounts(record)
                  ? FormatService.integer(record.selectedItemCount)
                  : ByteSizeService.bytes(record.expectedBytes)
              }}
            </strong>
            <strong class="record-byte">
              {{
                usesItemCounts(record)
                  ? FormatService.integer(record.affectedItemCount)
                  : ByteSizeService.bytes(displayedReleasedBytes(record))
              }}
            </strong>
            <MdIcon class="record-chevron" :name="ICON_NAMES.chevronRight" :size="17" />
          </button>
        </MdResultTableRow>
      </MdResultTable>
    </MdResultWorkspace>

    <MdHistoryDetailDialog v-model:open="detailOpen" :selected-record="selectedRecord" />

    <MdDestructiveActionDialog
      v-model:open="clearConfirmOpen"
      :title="t('history.clearConfirmTitle')"
      :description="t('history.clearConfirmDescription')"
      :summary-label="t('history.clearConfirmScope')"
      :summary-value="t('history.clearConfirmCount', { count: FormatService.integer(history.length) }, history.length)"
      :note="t('history.clearConfirmNote')"
      :cancel-label="t('common.cancel')"
      :confirm-label="t('history.clearConfirmAction')"
      :busy="busy"
      @confirm="confirmClearHistory"
    />
  </MdPageShell>
</template>

<style scoped>
@reference "@assets/main.css";

.clear-history-button {
  @apply border-0 bg-transparent text-muted-foreground shadow-none hover:text-destructive-text;
}

.clear-history-button:hover {
  background: var(--surface-destructive-subtle);
}

.history-list {
  --result-table-content-inline-padding: 0px;
}

.history-list-header,
.record {
  display: grid;
  grid-template-columns: 30px minmax(0, 1fr) 96px 96px 18px;
  align-items: center;
  gap: 12px;
}

.history-list-header {
  width: 100%;
  min-height: 36px;
  padding: 7px 14px;
  @apply text-muted-foreground;
  font-size: var(--font-content-secondary);
}

.history-list-header span:first-child {
  grid-column: 1 / 3;
}

.history-list-header span:nth-child(n + 2) {
  text-align: right;
}

.record {
  width: 100%;
  min-height: 52px;
  border: 0;
  padding: 5px 14px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  @apply focus-visible:outline-none;
}

.record-icon {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  @apply text-muted-foreground;
}

.record-main {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.record-main > span {
  display: flex;
  align-items: center;
  gap: 8px;
}

.record-main strong {
  font-size: var(--font-content-primary);
}

.record-main small {
  overflow: hidden;
  @apply text-muted-foreground;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--font-content-secondary);
}

.record-byte {
  min-width: 0;
  text-align: right;
  font-size: var(--font-content-primary);
}

.record-chevron {
  @apply text-muted-foreground;
}

@container history (max-width: 720px) {
  .record {
    grid-template-columns: 28px minmax(0, 1fr) 18px;
  }

  .record-byte {
    display: none;
  }

  .history-list :deep(.result-table-header) {
    display: none;
  }
}
</style>
