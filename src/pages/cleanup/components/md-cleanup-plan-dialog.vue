<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { computed } from 'vue';
import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import type { PresentedScanRuleResult } from '@/lib/models/cleanup';
import { ByteSizeService } from '@/lib/services/byte-size-service';
import { FormatUtils } from '@/lib/utils/format';

const { locale, t } = useI18n({ useScope: 'global' });

const props = defineProps<{
  busy: boolean;
  leftoverApplicationCount: number;
  leftoverBytes: number;
  leftoverItemCount: number;
  modelValue: boolean;
  rules: PresentedScanRuleResult[];
  selectedBytes: number;
  selectedItemCount: number;
}>();
const emit = defineEmits<{
  execute: [];
  'update:modelValue': [open: boolean];
}>();

const runningProcesses = computed(() => [
  ...new Set(props.rules.flatMap(rule => rule.runningProcesses).filter(Boolean)),
]);
const runningProcessLabel = computed(() => FormatUtils.list(runningProcesses.value, locale.value));
const requiresAppClose = computed(() => props.rules.some(rule => rule.requiresAppClose));
const planItems = computed(() => {
  const items = props.rules.map(rule => ({
    bytes: rule.bytes,
    description: rule.impact,
    key: `rule:${rule.ruleId}`,
    name: rule.name,
  }));

  if (props.leftoverItemCount) {
    const summary = t(
      'applicationLeftovers.planSummary',
      {
        applications: FormatUtils.integer(props.leftoverApplicationCount),
        locations: FormatUtils.integer(props.leftoverItemCount),
      },
      props.leftoverApplicationCount
    );
    items.push({
      bytes: props.leftoverBytes,
      description: `${summary} · ${t('applicationLeftovers.planImpact')}`,
      key: 'application-leftovers',
      name: t('applicationLeftovers.resultTitle'),
    });
  }

  return items.sort((left, right) => right.bytes - left.bytes);
});
</script>

<template>
  <Dialog :open="modelValue" @update:open="emit('update:modelValue', $event)">
    <MdDialogContent class="flex max-h-[84vh] min-h-0 flex-col gap-0 overflow-hidden p-0 sm:max-w-[720px]">
      <DialogHeader class="plan-header flex-none px-6 pt-5 pr-14">
        <DialogTitle>{{ t('cleanup.planDialogTitle') }}</DialogTitle>
        <DialogDescription class="plan-summary">
          <span>
            {{ t('cleanup.selectedItemCount', { count: FormatUtils.integer(selectedItemCount) }, selectedItemCount) }}
          </span>
          <span aria-hidden="true">·</span>
          <span>{{ t('cleanup.estimated') }}</span>
          <strong>{{ ByteSizeService.bytes(selectedBytes) }}</strong>
        </DialogDescription>
      </DialogHeader>

      <p v-if="requiresAppClose" class="process-warning flex-none">
        {{
          runningProcesses.length
            ? t('cleanup.closeAppsBeforeCleanup', {
                processes: runningProcessLabel,
              })
            : t('cleanup.closeAppsBeforeCleanupGeneric')
        }}
      </p>
      <div class="modal-rules scrollbar-stable min-h-0 flex-1">
        <div v-for="item in planItems" :key="item.key">
          <span class="plan-item-copy">
            <strong>{{ item.name }}</strong>
            <small :title="item.description">{{ item.description }}</small>
          </span>
          <strong class="plan-item-size">{{ ByteSizeService.bytes(item.bytes) }}</strong>
        </div>
      </div>

      <DialogFooter class="flex-none border-t border-border/70 px-6 py-3.5">
        <Button variant="outline" type="button" :disabled="busy" @click="emit('update:modelValue', false)">
          {{ t('cleanup.adjustSelection') }}
        </Button>
        <Button type="button" :disabled="busy" @click="emit('execute')">
          {{ t('cleanup.execute') }}
        </Button>
      </DialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
@reference "@assets/main.css";

.plan-header {
  gap: 4px;
}

.plan-summary {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: var(--font-content-secondary);
}

.plan-summary strong {
  @apply text-primary;
  font-size: 17px;
  font-weight: 600;
}

.modal-rules {
  @apply border border-border/70;
  margin: 8px 24px 10px;
  border-radius: 9px;
}

.process-warning {
  margin: 8px 24px 0;
  border-radius: 7px;
  padding: 6px 10px;
  @apply bg-warning/12 text-warning-foreground;
  font-size: var(--font-content-secondary);
}

.modal-rules > div {
  @apply border-t border-border/70;
  display: grid;
  min-height: 56px;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: center;
  gap: 16px;
  padding: 8px 14px;
}

.modal-rules > div:first-child {
  border-top: 0;
}

.plan-item-copy {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.plan-item-copy > strong,
.plan-item-size {
  font-size: 13px;
  font-weight: 500;
  line-height: 1.35;
}

.plan-item-copy small {
  @apply text-muted-foreground;
  overflow: hidden;
  font-size: 10.5px;
  line-height: 1.45;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.plan-item-size {
  white-space: nowrap;
}
</style>
