<script setup lang="ts">
import { FormatService } from '@/lib/services/presentation/format-service';
import { computed, nextTick, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdTooltip from '@/components/custom/md-tooltip.vue';
import MdDestructiveActionDialog from '@/components/custom/md-destructive-action-dialog.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import type {
  ApplicationUninstallBatchPlan,
  ApplicationUninstallBatchResult,
  ApplicationUninstallCandidate,
  ApplicationUninstallExecutionProgress,
} from '@/lib/models/applications/application';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import { ByteSizeService } from '@/lib/services/settings/byte-size-service';
import { applicationUninstallExecutionItems } from '../application-uninstall-execution';

const props = defineProps<{
  open: boolean;
  plan: ApplicationUninstallBatchPlan | null;
  preview: ApplicationUninstallBatchResult | null;
  candidates: ApplicationUninstallCandidate[];
  selectedCandidates: ApplicationUninstallCandidate[];
  selectedBytes: number;
  includesUserData: boolean;
  preparing: boolean;
  executing: boolean;
  cancellingExecution: boolean;
  confirmationLoading: boolean;
  executionProgress: ApplicationUninstallExecutionProgress | null;
}>();
const emit = defineEmits<{ 'update:open': [open: boolean]; execute: []; cancelExecution: [] }>();
const { t } = useI18n({ useScope: 'global' });
const executionList = ref<HTMLElement | null>(null);
const nativeBatch = computed(
  () =>
    (Boolean(props.plan?.plans.length) &&
      props.plan?.plans.every(applicationPlan =>
        applicationPlan.items.some(item => item.kind === 'nativeInstaller')
      )) ||
    (props.selectedCandidates.length > 0 &&
      props.selectedCandidates.every(candidate =>
        candidate.components.some(component => component.kind === 'nativeInstaller')
      ))
);
const executionProgressValue = computed(() => {
  const completed = props.executionProgress?.completedApplicationCount ?? 0;
  const total = props.executionProgress?.totalApplicationCount ?? props.plan?.plans.length ?? 0;
  return t('applicationUninstall.executionProgressValue', {
    completed: FormatService.integer(Math.min(completed, total)),
    total: FormatService.integer(total),
  });
});
const executionItems = computed(() =>
  applicationUninstallExecutionItems(
    props.plan?.plans ?? [],
    props.candidates,
    props.preview?.results ?? [],
    props.executionProgress
  ).map(item => ({
    ...item,
    name: item.name ?? t('applicationUninstall.unknownApplication'),
    detail: t(item.detailKey),
    statusLabel: t(item.statusLabelKey),
  }))
);
const activeExecutionItem = computed(() => executionItems.value.find(item => item.state === 'active') ?? null);
const executionTitle = computed(() => {
  if (props.cancellingExecution) return t('applicationUninstall.cancellingBatch');
  if (props.executionProgress?.stage === 'finalizing') {
    return t('applicationUninstall.finalizingBatch');
  }
  if (activeExecutionItem.value) {
    return t('applicationUninstall.uninstallingApplicationTitle', { name: activeExecutionItem.value.name });
  }
  return t('applicationUninstall.validatingBatch');
});
const executionDescription = computed(() =>
  props.cancellingExecution
    ? t('applicationUninstall.cancellingBatchDescription')
    : t('applicationUninstall.executionProgressDescription', {
        completed: FormatService.integer(props.executionProgress?.completedApplicationCount ?? 0),
        total: FormatService.integer(props.executionProgress?.totalApplicationCount ?? props.plan?.plans.length ?? 0),
      })
);
const executionPercent = computed(() => {
  const total = props.executionProgress?.totalApplicationCount ?? props.plan?.plans.length ?? 0;
  if (!total) return 0;
  return Math.min(100, ((props.executionProgress?.completedApplicationCount ?? 0) / total) * 100);
});
watch(
  () => props.executionProgress?.currentApplicationId,
  async applicationId => {
    if (!applicationId) return;
    await nextTick();
    // Keep the current application visible in long batches without changing
    // the fixed execution order or stealing focus from a native uninstaller.
    executionList.value
      ?.querySelector<HTMLElement>('.uninstall-execution-item.is-active')
      ?.scrollIntoView({ block: 'nearest' });
  }
);
</script>

<template>
  <MdDestructiveActionDialog
    :open="open"
    :title="
      executing
        ? executionTitle
        : t(
            nativeBatch ? 'applicationUninstall.confirmNativeBatchTitle' : 'applicationUninstall.confirmBatchTitle',
            {
              count: FormatService.integer(preview?.selectedApplicationCount ?? selectedCandidates.length),
            },
            preview?.selectedApplicationCount ?? selectedCandidates.length
          )
    "
    :description="
      executing
        ? executionDescription
        : t(
            nativeBatch
              ? 'applicationUninstall.confirmNativeBatchDescription'
              : includesUserData
                ? 'applicationUninstall.confirmBatchWithDataDescription'
                : 'applicationUninstall.confirmBatchDescription'
          )
    "
    :summary-label="
      executing
        ? ''
        : preparing
          ? t('applicationUninstall.preparingBatchSummary')
          : t(
              'applicationUninstall.confirmBatchSummary',
              {
                count: FormatService.integer(preview?.previewedApplicationCount ?? 0),
              },
              preview?.previewedApplicationCount ?? 0
            )
    "
    :summary-value="
      executing ? '' : ByteSizeService.bytes(preview?.previewedBytes ?? plan?.expectedBytes ?? selectedBytes)
    "
    :cancel-label="t('common.cancel')"
    :confirm-label="t('applicationUninstall.confirmBatchAction')"
    :busy="executing"
    :loading="confirmationLoading"
    :loading-label="t('applicationUninstall.preparingBatchSummary')"
    :show-details="executing"
    @update:open="emit('update:open', $event)"
    @confirm="emit('execute')"
  >
    <div ref="executionList" class="uninstall-execution-list" :aria-label="t('applicationUninstall.uninstallItemList')">
      <div
        v-for="item in executionItems"
        :key="item.applicationId"
        class="uninstall-execution-item"
        :class="`is-${item.state}`"
      >
        <span class="uninstall-execution-status" aria-hidden="true">
          <MdIcon v-if="item.state === 'completed'" :name="ICON_NAMES.check" :size="14" />
          <MdIcon v-else-if="item.state === 'cancelled'" :name="ICON_NAMES.minus" :size="13" />
          <b v-else-if="item.state === 'failed'">!</b>
          <i v-else-if="item.state === 'active'" class="md-operational-motion" />
          <i v-else />
        </span>
        <span class="uninstall-execution-copy">
          <MdTooltip :text="item.name"
            ><strong>{{ item.name }}</strong></MdTooltip
          >
          <small>{{ item.detail }}</small>
        </span>
        <small class="uninstall-execution-item-label">{{ item.statusLabel }}</small>
      </div>
    </div>
    <div
      class="uninstall-execution-progress"
      role="progressbar"
      :aria-label="executionProgressValue"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="Math.round(executionPercent)"
    >
      <span :style="{ width: `${executionPercent}%` }" />
    </div>
    <div class="uninstall-execution-actions">
      <Button
        class="uninstall-execution-cancel"
        variant="ghost"
        size="sm"
        type="button"
        :disabled="cancellingExecution"
        @click="emit('cancelExecution')"
      >
        {{
          cancellingExecution
            ? t('applicationUninstall.cancellingAction')
            : t('applicationUninstall.cancelExecutionAction')
        }}
      </Button>
    </div>
  </MdDestructiveActionDialog>
</template>

<style scoped>
@reference "@assets/main.css";

.uninstall-execution-list {
  max-height: 230px;
  overflow-y: auto;
  overscroll-behavior: contain;
  border-width: 1px;
  border-radius: 11px;
  @apply border-border/80 bg-muted/20;
}
.uninstall-execution-item {
  display: grid;
  min-width: 0;
  grid-template-columns: 24px minmax(0, 1fr) auto;
  align-items: center;
  gap: 9px;
  border-top-width: 1px;
  padding: 9px 11px;
  @apply border-border/60;
}
.uninstall-execution-item:first-child {
  border-top: 0;
}
.uninstall-execution-item.is-active {
  background: var(--surface-primary-subtle);
}
.uninstall-execution-status {
  display: grid;
  width: 22px;
  height: 22px;
  place-items: center;
  border-radius: 50%;
  @apply bg-muted text-muted-foreground;
}
.uninstall-execution-item.is-completed .uninstall-execution-status {
  @apply text-success;
  background: var(--surface-success-subtle);
}
.uninstall-execution-item.is-cancelled .uninstall-execution-status {
  @apply bg-muted text-muted-foreground;
}
.uninstall-execution-item.is-failed .uninstall-execution-status {
  @apply text-warning-foreground;
  background: var(--surface-warning-subtle);
}
.uninstall-execution-item.is-active .uninstall-execution-status {
  @apply text-primary-text;
  background: var(--surface-primary-subtle);
}
.uninstall-execution-status i {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  @apply bg-muted-foreground/45;
}
.uninstall-execution-status b {
  font-size: 12px;
  line-height: 1;
}
.uninstall-execution-item.is-active .uninstall-execution-status i {
  width: 13px;
  height: 13px;
  border-width: 2px;
  @apply border-primary/20 border-t-primary bg-transparent;
  animation: spin 0.8s linear infinite;
}
.uninstall-execution-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
}
.uninstall-execution-copy strong,
.uninstall-execution-copy small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.uninstall-execution-progress {
  height: 4px;
  margin-top: 14px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--surface-primary-subtle);
}
.uninstall-execution-progress > span {
  display: block;
  min-width: 2%;
  height: 100%;
  border-radius: inherit;
  @apply bg-primary transition-[width] duration-300 ease-out;
}
.uninstall-execution-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 10px;
}
.uninstall-execution-cancel {
  @apply text-muted-foreground hover:text-foreground;
}
.uninstall-execution-copy strong {
  font-size: 12.5px;
  line-height: 1.4;
}
.uninstall-execution-item-label {
  @apply text-muted-foreground;
  font-size: 10.5px;
  white-space: nowrap;
}
.uninstall-execution-copy small {
  margin-top: 1px;
  @apply text-muted-foreground;
  font-size: 10.5px;
  line-height: 1.4;
}
</style>
