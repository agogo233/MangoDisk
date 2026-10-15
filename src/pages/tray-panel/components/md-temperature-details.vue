<script setup lang="ts">
import type { TrendPoint } from '@/lib/models/system-resources';
import MdResourceTrend from './md-resource-trend.vue';
defineProps<{ history: TrendPoint[]; observedAtMs: number; active: boolean; label: string; identity: string }>();
</script>
<template>
  <section class="temperature-details">
    <slot />
    <div v-if="history.length" class="temperature-chart">
      <MdResourceTrend
        :key="identity"
        metric="temperature"
        :history="history"
        :observed-at-ms="observedAtMs"
        :active="active"
        :label="label"
      />
    </div>
  </section>
</template>
<style scoped>
@reference "@assets/main.css";
.temperature-details {
  @apply rounded-xl border border-border bg-card;
  min-width: 0;
  padding: 10px 12px;
  flex: none;
}
.temperature-chart {
  display: flow-root;
}
.temperature-chart :deep(.resource-trend) {
  height: 28px;
  margin-top: 4px;
}
</style>
