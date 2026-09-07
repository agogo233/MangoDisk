<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdConfirmDialog from '@/components/custom/md-confirm-dialog.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import type { PrivacyExecutionItemResult } from '@/lib/models/privacy';
import { ICON_NAMES } from '@/lib/models/ui';
import * as FormatUtils from '@/lib/utils/format';
import { usePrivacyStore } from '@/stores/privacy-store';

const emit = defineEmits<{ cancel: [] }>();
const { t } = useI18n({ useScope: 'global' });
const privacyStore = usePrivacyStore();
const cancellationConfirmOpen = ref(false);
const clockMs = ref(Date.now());
const itemListElement = ref<HTMLElement | null>(null);
let clockTimer: ReturnType<typeof setInterval> | null = null;
type ExecutionItemState = PrivacyExecutionItemResult['status'] | 'active' | 'pending';

const progress = computed(() => privacyStore.executionProgress);
const planItems = computed(() => privacyStore.executionItems);
const total = computed(() => progress.value?.totalItemCount ?? planItems.value.length);
const progressDivisor = computed(() => Math.max(total.value, 1));
const completed = computed(() => Math.min(progress.value?.completedItemCount ?? 0, total.value));
const elapsedMs = computed(() => {
  const reported = progress.value?.elapsedMs ?? 0;
  const startedAt = privacyStore.executionStartedAtMs;
  const live = startedAt === null ? 0 : Math.max(0, clockMs.value - startedAt);
  return Math.max(reported, live);
});
const elapsedSeconds = computed(() => Math.floor(elapsedMs.value / 1000));
const percent = computed(() => {
  if (!progress.value || progress.value.stage === 'validating') return 5;
  if (progress.value.stage === 'finalizing') return 98;
  return 10 + (completed.value / progressDivisor.value) * 85;
});
const stageLabel = computed(() => {
  if (privacyStore.cancellingExecution) return t('loading.cancellingCleanup');
  if (progress.value?.stage === 'validating') return t('loading.validating');
  if (progress.value?.stage === 'finalizing') return t('loading.finalizing');
  return t('privacy.executing');
});
const activeItem = computed(() => {
  if (progress.value?.stage !== 'cleaning') return null;
  return planItems.value.find(item => item.token === progress.value?.currentToken) ?? null;
});
const title = computed(() =>
  activeItem.value ? t('loading.cleaningCurrentItem', { name: activeItem.value.sourceName }) : stageLabel.value
);
const items = computed(() => {
  const terminalStates = new Map(progress.value?.completedItems.map(item => [item.token, item.status]) ?? []);
  return planItems.value.map(item => ({
    ...item,
    state: (terminalStates.get(item.token) ??
      (progress.value?.stage === 'cleaning' && item.token === progress.value.currentToken
        ? 'active'
        : 'pending')) as ExecutionItemState,
  }));
});

function itemStatusLabel(state: ExecutionItemState): string {
  switch (state) {
    case 'cleared':
      return t('loading.cleanupItemDone');
    case 'unchanged':
      return t('loading.cleanupItemUnchanged');
    case 'failed':
      return t('loading.cleanupItemFailed');
    case 'cancelled':
      return t('loading.cleanupItemCancelled');
    case 'active':
      return t('loading.cleanupItemActive');
    default:
      return t('loading.cleanupItemPending');
  }
}

watch(
  () => privacyStore.executing,
  executing => {
    if (!executing) cancellationConfirmOpen.value = false;
  }
);
watch(completed, async () => {
  await nextTick();
  itemListElement.value?.querySelector<HTMLElement>('.privacy-operation-item.is-active')?.scrollIntoView({
    block: 'nearest',
  });
});

function requestCancellation() {
  if (privacyStore.cancellingExecution) return;
  cancellationConfirmOpen.value = true;
}

function confirmCancellation() {
  cancellationConfirmOpen.value = false;
  emit('cancel');
}

onMounted(() => {
  clockTimer = window.setInterval(() => {
    if (privacyStore.executing) clockMs.value = Date.now();
  }, 1000);
});
onBeforeUnmount(() => {
  if (clockTimer) window.clearInterval(clockTimer);
});
</script>

<template>
  <div v-if="privacyStore.executing" class="privacy-operation-overlay">
    <div class="privacy-operation-drag-region" data-tauri-drag-region aria-hidden="true" />
    <section class="privacy-operation-card">
      <div class="privacy-operation-heading" role="status" aria-live="polite">
        <span class="privacy-operation-icon"><MdIcon :name="ICON_NAMES.shield" :size="27" /></span>
        <div>
          <h2 :title="title">{{ title }}</h2>
          <p>
            {{
              privacyStore.cancellingExecution
                ? t('loading.cancellingCleanupHint')
                : t('loading.cleanupProgressSummary', {
                    completed: FormatUtils.integer(completed),
                    total: FormatUtils.integer(total),
                  })
            }}
          </p>
        </div>
      </div>

      <div ref="itemListElement" class="privacy-operation-list" :aria-label="t('loading.cleanupItemList')">
        <div v-for="item in items" :key="item.token" class="privacy-operation-item" :class="`is-${item.state}`">
          <span class="privacy-operation-item-status" aria-hidden="true">
            <MdIcon v-if="item.state === 'cleared' || item.state === 'unchanged'" :name="ICON_NAMES.check" :size="14" />
            <MdIcon
              v-else-if="item.state === 'failed' || item.state === 'cancelled'"
              :name="ICON_NAMES.info"
              :size="14"
            />
            <i v-else-if="item.state === 'active'" class="md-operational-motion" />
            <i v-else />
          </span>
          <span class="privacy-operation-item-content">
            <strong>{{ item.sourceName }}</strong>
            <small>{{ t(`privacy.kinds.${item.kind}`) }}</small>
          </span>
          <small class="privacy-operation-item-label">
            {{ itemStatusLabel(item.state) }}
          </small>
        </div>
      </div>

      <div
        class="privacy-operation-progress"
        role="progressbar"
        :aria-label="stageLabel"
        :aria-valuemin="0"
        :aria-valuemax="100"
        :aria-valuenow="Math.round(percent)"
      >
        <span :style="{ width: `${percent}%` }" />
      </div>

      <div class="privacy-operation-stats">
        <span>
          <small>{{ t('loading.ruleProgress') }}</small>
          <strong>{{ t('loading.ruleProgressValue', { completed, total }) }}</strong>
        </span>
        <span>
          <small>{{ t('loading.elapsed') }}</small>
          <strong>{{ t('loading.elapsedSeconds', { count: elapsedSeconds }, elapsedSeconds) }}</strong>
        </span>
        <span>
          <small>{{ t('loading.processedItems') }}</small>
          <strong>{{ FormatUtils.integer(progress?.affectedItemCount ?? 0) }}</strong>
        </span>
      </div>

      <div class="privacy-operation-actions">
        <Button
          variant="ghost"
          size="sm"
          type="button"
          :disabled="privacyStore.cancellingExecution"
          @click="requestCancellation"
        >
          {{
            privacyStore.cancellingExecution ? t('loading.cancellingCleanupAction') : t('loading.cancelCleanupAction')
          }}
        </Button>
      </div>
    </section>
  </div>

  <MdConfirmDialog
    v-model:open="cancellationConfirmOpen"
    :title="t('loading.cancelCleanupConfirmTitle')"
    :description="t('loading.cancelCleanupConfirmDescription')"
    :cancel-label="t('common.cancel')"
    :confirm-label="t('loading.stopCleanupAction')"
    confirm-variant="destructive"
    @confirm="confirmCancellation"
  />
</template>

<style scoped>
@reference "@assets/main.css";
.privacy-operation-overlay {
  position: fixed;
  z-index: 40;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  background-color: var(--modal-overlay-background);
}
.privacy-operation-drag-region {
  position: absolute;
  z-index: 0;
  inset: 0;
}
.privacy-operation-card {
  position: relative;
  z-index: 1;
  width: min(620px, calc(100vw - 48px));
  border-width: 1px;
  border-radius: 16px;
  padding: 25px 26px 20px;
  pointer-events: auto;
  user-select: none;
  @apply border-border bg-card text-card-foreground shadow-2xl shadow-foreground/10;
}
.privacy-operation-heading {
  display: flex;
  align-items: center;
  gap: 15px;
}
.privacy-operation-heading > div {
  min-width: 0;
  flex: 1;
}
.privacy-operation-heading h2 {
  overflow: hidden;
  margin: 0;
  font-size: 18px;
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.privacy-operation-heading p {
  margin: 6px 0 0;
  @apply text-muted-foreground;
  font-size: 12px;
  line-height: 1.55;
}
.privacy-operation-icon {
  display: grid;
  width: 52px;
  height: 52px;
  flex: none;
  place-items: center;
  border-radius: 14px;
  @apply text-primary;
  background: var(--surface-primary-subtle);
}
.privacy-operation-list {
  max-height: min(330px, 44vh);
  margin-top: 22px;
  overflow: auto;
  border-width: 1px;
  border-radius: 12px;
  @apply border-border;
}
.privacy-operation-item {
  display: grid;
  grid-template-columns: 26px minmax(0, 1fr) auto;
  align-items: center;
  gap: 11px;
  min-height: 66px;
  padding: 10px 15px;
  border-bottom-width: 1px;
  @apply border-border;
}
.privacy-operation-item:last-child {
  border-bottom: 0;
}
.privacy-operation-item.is-active {
  background: var(--surface-primary-subtle);
}
.privacy-operation-item-status {
  display: grid;
  width: 22px;
  height: 22px;
  place-items: center;
  border-width: 1px;
  border-radius: 999px;
  @apply border-border text-muted-foreground;
}
.privacy-operation-item.is-cleared .privacy-operation-item-status,
.privacy-operation-item.is-unchanged .privacy-operation-item-status {
  @apply border-primary/25 text-primary;
  background: var(--surface-primary-subtle);
}
.privacy-operation-item.is-failed .privacy-operation-item-status,
.privacy-operation-item.is-cancelled .privacy-operation-item-status {
  @apply border-destructive/25 text-destructive;
  background: var(--surface-destructive-subtle);
}
.privacy-operation-item-status i {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  @apply bg-muted-foreground/35;
}
.privacy-operation-item-status i.md-operational-motion {
  width: 13px;
  height: 13px;
  border-width: 2px;
  @apply border-primary/20 border-t-primary bg-transparent;
  animation: privacy-operation-spin 0.8s linear infinite;
}
.privacy-operation-item-content {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 4px;
}
.privacy-operation-item-content strong,
.privacy-operation-item-content small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.privacy-operation-item-content strong {
  font-size: 13px;
}
.privacy-operation-item-content small,
.privacy-operation-item-label {
  @apply text-muted-foreground;
  font-size: 11px;
}
.privacy-operation-progress {
  height: 5px;
  margin-top: 18px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--surface-primary-subtle);
}
.privacy-operation-progress span {
  display: block;
  height: 100%;
  border-radius: inherit;
  transition: width 180ms ease-out;
  @apply bg-primary;
}
.privacy-operation-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
  margin-top: 16px;
}
.privacy-operation-stats > span {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 4px;
}
.privacy-operation-stats small {
  @apply text-muted-foreground;
  font-size: 10px;
}
.privacy-operation-stats strong {
  font-size: 13px;
}
.privacy-operation-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 14px;
}
@keyframes privacy-operation-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
