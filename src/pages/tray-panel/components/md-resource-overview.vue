<script setup lang="ts">
import MdTooltip from '@/components/custom/md-tooltip.vue';
import { computed } from 'vue';
import MdResourceTrend from './md-resource-trend.vue';
import { useI18n } from 'vue-i18n';
import {
  METRIC_LABEL_KEYS,
  METRIC_STATUS_KEYS,
  type MetricId,
  type ResourceReadings,
} from '@/lib/models/system-resources';
import { ByteSizeService } from '@/lib/services/byte-size-service';

const props = withDefaults(defineProps<{ metric: MetricId; reading: ResourceReadings; active?: boolean }>(), {
  active: true,
});
defineEmits<{ cleanup: []; memory: [] }>();
const { t } = useI18n({ useScope: 'global' });
const current = computed(() => props.reading[props.metric]);
const ready = computed(() => current.value.status === 'ready' && current.value.value !== null);
const memory = computed(() => props.reading.memory.value?.memory);
const percentage = computed(() => {
  if (!ready.value) return null;
  switch (props.metric) {
    case 'cpu':
      return props.reading.cpu.value?.usedPercent ?? null;
    case 'memory':
      return memory.value?.usedPercent ?? null;
    case 'disk':
      return props.reading.disk.value?.usedPercent ?? null;
    default:
      return null;
  }
});
const activityReady = computed(() =>
  props.metric === 'disk' ? props.reading.diskIo.status === 'ready' && props.reading.diskIo.value !== null : ready.value
);
const history = computed(
  () =>
    ({
      cpu: props.reading.cpuHistory,
      memory: props.reading.memoryHistory,
      disk: props.reading.diskIoHistory,
      network: props.reading.networkHistory,
    })[props.metric]
);
const peak = computed(() => {
  const samples = history.value.filter(
    point =>
      point.sampledAtMs <= props.reading.observedAtMs &&
      props.reading.observedAtMs - point.sampledAtMs <= 60000 &&
      Number.isFinite(point.primary)
  );
  if (!samples.length || !ready.value) return '—';
  return props.metric === 'cpu'
    ? `${Math.max(...samples.map(point => point.primary)).toFixed(0)}%`
    : `${ByteSizeService.bytes(Math.max(...samples.map(point => Math.max(point.primary, point.secondary ?? 0))))}/s`;
});
const source = computed(() =>
  props.metric === 'network'
    ? props.reading.network.value?.interface.name
    : props.reading.disk.value?.volume.system
      ? t('systemStatus.systemDisk')
      : props.reading.disk.value?.volume.name
);
const rates = computed(() =>
  [
    {
      direction: 'upload',
      arrow: '↑',
      bytes:
        props.metric === 'disk'
          ? props.reading.diskIo.value?.writtenBytesPerSecond
          : props.reading.network.value?.transmittedBytesPerSecond,
    },
    {
      direction: 'download',
      arrow: '↓',
      bytes:
        props.metric === 'disk'
          ? props.reading.diskIo.value?.readBytesPerSecond
          : props.reading.network.value?.receivedBytesPerSecond,
    },
  ].map(rate => {
    // Split only the app-owned byte formatter, keeping unit positions independent of digits.
    const formatted = activityReady.value && rate.bytes !== undefined ? ByteSizeService.bytes(rate.bytes) : '—';
    const separator = formatted.lastIndexOf(' ');
    return {
      ...rate,
      value: separator < 0 ? formatted : formatted.slice(0, separator),
      unit: separator < 0 ? '' : `${formatted.slice(separator + 1)}/s`,
    };
  })
);
</script>

<template>
  <section class="resource-overview" :data-metric="metric" :aria-label="t(METRIC_LABEL_KEYS[metric])">
    <header>
      <span class="resource-label">{{ t(METRIC_LABEL_KEYS[metric]) }}</span>
      <div v-if="metric === 'network'" class="network-values">
        <span
          v-for="rate in rates"
          :key="rate.direction"
          class="network-rate"
          :aria-label="t(rate.direction === 'upload' ? 'systemStatus.upload' : 'systemStatus.download')"
        >
          <b :class="rate.direction" aria-hidden="true">{{ rate.arrow }}</b>
          <strong>{{ rate.value }}</strong
          ><small>{{ rate.unit }}</small>
        </span>
      </div>
      <strong v-else class="resource-value"
        >{{ percentage === null ? '—' : percentage.toFixed(0) }}<small v-if="percentage !== null">%</small></strong
      >
    </header>

    <div
      v-if="metric === 'disk'"
      class="capacity-track"
      :role="ready ? 'meter' : undefined"
      :aria-label="t(METRIC_LABEL_KEYS[metric])"
      :aria-valuenow="percentage ?? undefined"
      :aria-valuemin="0"
      :aria-valuemax="100"
    >
      <i v-if="percentage !== null" :style="{ width: `${Math.max(0, Math.min(100, percentage))}%` }" />
    </div>
    <div v-if="metric === 'disk'" class="resource-meta">
      <span v-if="ready && reading.disk.value"
        >{{ t('systemStatus.available') }} {{ ByteSizeService.bytes(reading.disk.value.availableBytes) }} /
        {{ ByteSizeService.bytes(reading.disk.value.totalBytes) }}</span
      >
      <span v-else role="status">{{ t(METRIC_STATUS_KEYS[current.status]) }}</span>
      <button class="cleanup-link" @click="$emit('cleanup')">
        {{ t('navigation.cleanup') }} <span aria-hidden="true">›</span>
      </button>
    </div>
    <div v-if="metric === 'disk'" class="disk-activity">
      <span v-for="rate in rates" :key="rate.direction" class="disk-rate">
        <span :class="rate.direction">{{
          t(rate.direction === 'upload' ? 'systemStatus.write' : 'systemStatus.read')
        }}</span>
        <strong>{{ rate.value }}</strong
        ><small>{{ rate.unit }}</small>
      </span>
    </div>
    <MdResourceTrend
      :key="metric === 'network' ? reading.network.value?.interface.id : metric"
      :metric="metric"
      :active="active"
      :history="history"
      :observed-at-ms="reading.observedAtMs"
      :label="t(metric === 'disk' ? 'systemStatus.diskActivityHistory' : 'systemStatus.lastMinute')"
    />
    <div class="resource-meta">
      <template v-if="metric === 'cpu'">
        <span v-if="!ready" role="status">{{ t(METRIC_STATUS_KEYS[current.status]) }}</span>
        <span v-else>{{ t('systemStatus.idle') }} {{ (100 - (percentage ?? 0)).toFixed(0) }}%</span>
        <span>{{ t('systemStatus.minutePeak') }} {{ peak }}</span>
      </template>
      <template v-else-if="metric === 'memory'">
        <span v-if="ready && memory"
          >{{ ByteSizeService.memory(memory.usedBytes) }} / {{ ByteSizeService.memory(memory.totalBytes) }}</span
        >
        <span v-else role="status">{{ t(METRIC_STATUS_KEYS[current.status]) }}</span>
        <button class="memory-link" @click="$emit('memory')">
          {{ t('systemStatus.memoryDetails') }} <span aria-hidden="true">›</span>
        </button>
      </template>
      <template v-else>
        <MdTooltip :text="source"
          ><span class="resource-source">{{
            source || t(metric === 'disk' ? 'systemStatus.volume' : 'systemStatus.automatic')
          }}</span></MdTooltip
        >
        <MdTooltip v-if="metric === 'disk'" :text="t('systemStatus.diskActivityHistory')"
          ><span
            >{{ t('systemStatus.allDisks') }} ·
            {{ activityReady ? t('systemStatus.lastMinute') : t(METRIC_STATUS_KEYS[reading.diskIo.status]) }}</span
          ></MdTooltip
        >
        <span v-else-if="!ready" role="status">{{ t(METRIC_STATUS_KEYS[current.status]) }}</span>
        <MdTooltip v-else :text="t('systemStatus.networkScope')"
          ><span>{{ t('systemStatus.minutePeak') }} {{ peak }}</span></MdTooltip
        >
      </template>
    </div>
    <div v-if="metric === 'memory'" class="resource-meta secondary-metrics">
      <span>{{ t('monitoring.free') }} {{ ready && memory ? ByteSizeService.memory(memory.freeBytes) : '—' }}</span>
      <span>{{ t('monitoring.swap') }} {{ ready && memory ? ByteSizeService.memory(memory.swapUsedBytes) : '—' }}</span>
    </div>
  </section>
</template>

<style scoped>
@reference "@assets/main.css";
.resource-overview {
  @apply rounded-xl border border-border bg-card;
  padding: 6px 12px;
  min-width: 0;
  flex: none;
}
header,
.resource-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
}
header {
  /* Reserve the percentage baseline even while the value is an em dash. */
  min-height: 30px;
}
.resource-label {
  font-size: 12px;
  flex: none;
}
.resource-value {
  font-size: 24px;
  line-height: 30px;
  font-variant-numeric: tabular-nums;
}
small {
  font-size: 11px;
  font-weight: normal;
}
.resource-value small {
  margin-left: 2px;
}
.resource-meta,
.resource-source {
  @apply text-muted-foreground;
  font-size: 10px;
  line-height: 16px;
  min-height: 16px;
}
.resource-meta {
  margin-top: 4px;
  min-height: 18px;
}
.resource-meta > span,
.resource-source {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.resource-meta > span {
  min-width: 0;
}
.resource-meta button {
  @apply text-primary;
  flex: none;
  font-size: 10px;
  min-height: 20px;
  padding: 0 4px;
  border-radius: 4px;
  cursor: pointer;
}
.resource-meta button:hover {
  @apply bg-accent;
}
.resource-meta button:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
.network-values {
  display: grid;
  gap: 2px;
}
.network-rate {
  display: grid;
  grid-template-columns: 12px 6ch 4.5ch;
  align-items: baseline;
  column-gap: 4px;
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}
.network-rate strong {
  text-align: right;
  font-weight: 600;
}
.network-rate small {
  white-space: nowrap;
}
.network-rate b {
  font-weight: 600;
}
.upload {
  color: var(--status-upload);
}
.download {
  color: var(--status-download);
}
.secondary-metrics {
  margin-top: 0;
  min-height: 16px;
}
.disk-activity {
  display: flex;
  gap: 12px;
  justify-content: space-between;
  margin-top: 4px;
}
.disk-rate {
  display: grid;
  grid-template-columns: auto 5.5ch 4.5ch;
  gap: 3px;
  align-items: baseline;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.disk-rate strong {
  text-align: right;
  font-weight: 500;
}
.disk-rate > span {
  font-size: 10px;
}
.capacity-track {
  @apply bg-muted rounded-full;
  height: 4px;
  overflow: hidden;
  margin-top: 4px;
}
.capacity-track i {
  background: var(--primary);
  opacity: 0.55;
  height: 100%;
  display: block;
}
</style>
