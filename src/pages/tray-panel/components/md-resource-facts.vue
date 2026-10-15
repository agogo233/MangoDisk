<script setup lang="ts">
import { useI18n } from 'vue-i18n';
defineProps<{
  facts: { key: string; label: string; value: string; unit: string; cached?: boolean }[];
}>();
const { t } = useI18n({ useScope: 'global' });
</script>
<template>
  <dl v-if="facts.length" class="resource-facts" :class="{ compact: facts.length === 1 }">
    <div v-for="fact in facts" :key="fact.key" class="resource-fact">
      <dt>
        <span>{{ fact.label }}</span>
        <span v-if="fact.cached" class="cached">{{ t('cpuDetails.cached') }}</span>
      </dt>
      <dd>
        {{ fact.value }}<small v-if="fact.unit">{{ fact.unit }}</small>
      </dd>
    </div>
  </dl>
</template>
<style scoped>
@reference "@assets/main.css";
.resource-facts {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(90px, 1fr));
  gap: 8px;
  flex: none;
}
.resource-fact {
  @apply rounded-lg bg-muted/40;
  padding: 8px;
  min-width: 0;
}
dt {
  @apply text-muted-foreground;
  display: flex;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  font-size: 10px;
}
dt > span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
dd {
  margin-top: 6px;
  font-size: 18px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
small {
  margin-left: 3px;
  font-size: 10px;
  font-weight: normal;
}
.cached {
  flex: none;
  font-size: 8px;
}
.compact .resource-fact {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.compact dd {
  margin-top: 0;
  flex: none;
}
</style>
