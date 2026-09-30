<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdInlineNotice from '@/components/custom/md-inline-notice.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { ICON_NAMES } from '@/lib/models/ui';

defineProps<{
  modelValue: boolean;
  failureCount: number;
  retryDisabled: boolean;
}>();
const emit = defineEmits<{
  'update:modelValue': [value: boolean];
  openLogs: [];
  retry: [];
}>();
const { t } = useI18n({ useScope: 'global' });
function requestRetry() {
  emit('update:modelValue', false);
  emit('retry');
}
</script>

<template>
  <button
    type="button"
    class="scan-warning-trigger"
    aria-haspopup="dialog"
    :aria-expanded="modelValue"
    @click="emit('update:modelValue', true)"
  >
    <span>{{ t('cleanup.permission.otherWarning') }}</span>
    <MdIcon :name="ICON_NAMES.info" :size="13" aria-hidden="true" />
  </button>

  <Dialog :open="modelValue" @update:open="emit('update:modelValue', $event)">
    <MdDialogContent size="compact">
      <MdDialogHeader>
        <DialogTitle>{{ t('cleanup.permission.otherTitle') }}</DialogTitle>
        <DialogDescription>
          {{ t('cleanup.permission.otherDescription', { count: failureCount }) }}
        </DialogDescription>
      </MdDialogHeader>
      <MdInlineNotice class="scan-warning-instructions" :icon-name="ICON_NAMES.info" tone="info">
        {{ t('cleanup.permission.otherInstructions') }}
      </MdInlineNotice>
      <MdDialogFooter align="between">
        <Button type="button" variant="outline" @click="emit('openLogs')">
          {{ t('settings.feedbackDialog.openLogFolder') }}
        </Button>
        <Button type="button" :disabled="retryDisabled" @click="requestRetry">
          {{ t('overview.rescan') }}
        </Button>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
@reference "@assets/main.css";

.scan-warning-trigger {
  display: inline-flex;
  min-width: 0;
  max-width: min(440px, 46vw);
  align-items: center;
  gap: 5px;
  border: 0;
  padding: 4px 0;
  background: transparent;
  color: var(--muted-foreground);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.scan-warning-trigger span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.scan-warning-trigger :deep(svg) {
  flex: none;
}

.scan-warning-trigger:hover {
  color: var(--foreground);
  text-decoration: underline;
}

.scan-warning-trigger:focus-visible {
  border-radius: 3px;
  @apply outline-none ring-2 ring-ring;
}

.scan-warning-instructions {
  margin: 0 var(--layout-dialog-body-inline-padding) 14px;
}
</style>
