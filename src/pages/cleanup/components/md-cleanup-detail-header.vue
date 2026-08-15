<script setup lang="ts">
import { useI18n } from 'vue-i18n';

import MdResultCheckbox from '@/components/custom/md-result-checkbox.vue';
import { ByteSizeService } from '@/lib/services/byte-size-service';

withDefaults(
  defineProps<{
    title: string;
    selectedBytes: number;
    totalBytes: number;
    selection: 'all' | 'partial' | 'none';
    disabled?: boolean;
  }>(),
  { disabled: false }
);
const emit = defineEmits<{
  'update:selected': [selected: boolean];
}>();
const { t } = useI18n({ useScope: 'global' });
</script>

<template>
  <header class="detail-header">
    <strong class="detail-title">{{ title }}</strong>
    <span class="detail-size">
      <small>{{ t('cleanup.selected') }} / {{ t('cleanup.cleanableFound') }}</small>
      <strong>{{ ByteSizeService.bytes(selectedBytes) }}</strong>
      <i>/ {{ ByteSizeService.bytes(totalBytes) }}</i>
    </span>
    <label class="category-selection">
      <MdResultCheckbox
        :checked="selection === 'all'"
        :indeterminate="selection === 'partial'"
        :disabled="disabled"
        @update:checked="emit('update:selected', $event)"
      />
      <span>{{ t('cleanup.selectAll') }}</span>
    </label>
  </header>
</template>

<style scoped>
@reference "@assets/main.css";

.detail-header {
  @apply border-border;
  display: grid;
  min-height: 46px;
  flex: none;
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 12px;
  border-bottom-width: 1px;
  padding: 5px 12px;
}

.detail-title {
  min-width: 0;
  overflow: hidden;
  font-size: var(--font-content-primary);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-size {
  display: flex;
  align-items: baseline;
  gap: 6px;
  white-space: nowrap;
}

.detail-size small {
  @apply text-muted-foreground;
  font-size: 10px;
}

.detail-size strong {
  @apply text-primary;
  font-size: 15px;
}

.detail-size i {
  @apply text-muted-foreground;
  font-size: 11px;
  font-style: normal;
}

.category-selection {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  cursor: pointer;
}

@container cleanup (max-width: 760px) {
  .detail-header {
    grid-template-columns: minmax(0, 1fr) auto;
    padding-inline: 10px;
  }

  .detail-size,
  .category-selection span {
    display: none;
  }
}
</style>
