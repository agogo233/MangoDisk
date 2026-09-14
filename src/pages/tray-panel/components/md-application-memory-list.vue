<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdApplicationMemoryRow from './md-application-memory-row.vue';
import type { ProcessMemorySummary } from '@/lib/models/system-resources';

const props = defineProps<{ summary: ProcessMemorySummary | null }>();
// Compare rows with the largest item, not system RAM: shared RSS cannot be summed
// into a truthful system-memory percentage. Calculate once per snapshot update.
const largestBytes = computed(() => Math.max(0, ...(props.summary?.applications.map(row => row.residentBytes) ?? [])));
// Track the selected identity rather than its changing rank during live updates.
const expandedId = ref<string | null>(null);
watch(
  () => props.summary,
  summary => {
    if (!summary?.applications.some(row => row.id === expandedId.value)) expandedId.value = null;
  }
);
const { t } = useI18n({ useScope: 'global' });
</script>

<template>
  <section class="application-memory" :aria-label="t('monitoring.topApplications')">
    <div class="list-heading">
      <h2>{{ t('monitoring.topApplications') }}</h2>
      <span>{{ t('monitoring.residentMemory') }}</span>
    </div>
    <p v-if="!summary" class="list-empty" role="status">{{ t('monitoring.loadingApplications') }}</p>
    <p v-else-if="!summary.applications.length" class="list-empty">{{ t('monitoring.noApplications') }}</p>
    <ol v-else class="scrollbar-stable-end" tabindex="0" :aria-label="t('monitoring.topApplications')">
      <MdApplicationMemoryRow
        v-for="application in summary.applications"
        :key="application.id"
        :application="application"
        :expanded="expandedId === application.id"
        :share="largestBytes > 0 ? (Math.max(0, application.residentBytes) / largestBytes) * 100 : 0"
        @toggle="expandedId = expandedId === application.id ? null : application.id"
      />
    </ol>
  </section>
</template>

<style scoped>
@reference "@assets/main.css";
.application-memory {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
.list-heading {
  flex: none;
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  margin: 0 0 6px;
  padding-right: 12px;
}
h2 {
  font-size: 12px;
  font-weight: 650;
}
.list-heading > span,
.list-empty {
  @apply text-muted-foreground;
  font-size: 10px;
}
ol {
  /* Only rows scroll; the overview and column labels remain visible. */
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  list-style: none;
  padding: 0 12px 0 0;
  margin: 0;
}
ol:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
.list-empty {
  padding: 20px 0;
  text-align: center;
  font-size: 12px;
}
</style>
