<script setup lang="ts">
import { computed } from 'vue';
import { Button } from '@/components/ui/button';
import MdResourceFacts from './md-resource-facts.vue';
import MdCpuTemperature from './md-cpu-temperature.vue';
import MdTemperatureDetails from './md-temperature-details.vue';
import MdResourceOverview from './md-resource-overview.vue';
import { useI18n } from 'vue-i18n';
import { summarizeUtilizationHistory } from '@/lib/utils/utilization-history-summary';
import type { ResourceReadings } from '@/lib/models/system-resources';
const props = withDefaults(
  defineProps<{
    reading: ResourceReadings;
    active?: boolean;
    temperatureEnabled?: boolean;
    temperatureSupported?: boolean;
  }>(),
  { active: true, temperatureEnabled: true, temperatureSupported: true }
);
defineEmits<{ configureTemperature: [] }>();
const { t } = useI18n({ useScope: 'global' });
const facts = computed(() => {
  const frequencyReading = props.reading.cpuFrequency;
  const frequency =
    frequencyReading.status === 'failed' || frequencyReading.status === 'unsupported' ? null : frequencyReading.value;
  const frequencyFacts = [
    { label: 'cpuDetails.average', mhz: frequency?.averageMhz },
    { label: 'cpuDetails.efficiency', mhz: frequency?.efficiencyMhz },
    { label: 'cpuDetails.performance', mhz: frequency?.performanceMhz },
  ]
    .filter(fact => fact.mhz != null && Number.isFinite(fact.mhz) && fact.mhz >= 1 && fact.mhz <= 20_000)
    .map(fact => ({
      label: fact.label,
      value: (fact.mhz! / 1000).toFixed(2),
      unit: 'GHz',
      cached: frequencyReading.status !== 'ready',
    }));
  const hasFrequencyClasses =
    frequencyFacts.some(fact => fact.label === 'cpuDetails.efficiency') &&
    frequencyFacts.some(fact => fact.label === 'cpuDetails.performance');
  const selectedFrequencyFacts = hasFrequencyClasses
    ? frequencyFacts.filter(fact => fact.label !== 'cpuDetails.average')
    : frequencyFacts;
  const history = summarizeUtilizationHistory(props.reading.cpuHistory, props.reading.observedAtMs);
  const usageFacts = history
    ? [
        { label: 'cpuDetails.minuteAverage', value: history.average },
        { label: 'cpuDetails.minutePeak', value: history.peak },
      ]
        .filter(fact => !hasFrequencyClasses || fact.label !== 'cpuDetails.minutePeak')
        .map(fact => ({
          label: fact.label,
          value: Math.round(fact.value).toString(),
          unit: '%',
          cached: props.reading.cpu.status !== 'ready',
        }))
    : [];
  return [...selectedFrequencyFacts, ...usageFacts].map(fact => ({
    ...fact,
    key: fact.label,
    label: t(fact.label),
  }));
});
</script>
<template>
  <MdResourceOverview
    class="cpu-summary"
    metric="cpu"
    :heading="t('systemStatus.cpuUsage')"
    :show-cpu-temperature="false"
    :reading="reading"
    :active="active"
  />
  <MdTemperatureDetails
    v-if="temperatureSupported && reading.cpuTemperature.status !== 'unsupported'"
    :history="temperatureEnabled ? reading.cpuTemperatureHistory : []"
    :observed-at-ms="reading.observedAtMs"
    :active="active"
    :label="t('cpuTemperature.trend')"
    :identity="`${reading.cpuTemperature.value?.source}:${reading.cpuTemperature.value?.kind}:${reading.cpuTemperature.value?.sensorCount}`"
    :aria-label="t('cpuTemperature.label')"
  >
    <template v-if="!temperatureEnabled">
      <p class="text-content-secondary">{{ t('windowsMonitoring.disabledHint') }}</p>
      <Button variant="ghost" size="sm" @click="$emit('configureTemperature')">{{
        t('windowsMonitoring.manage')
      }}</Button>
    </template>
    <MdCpuTemperature v-else :reading="reading.cpuTemperature" :observed-at-ms="reading.observedAtMs" />
  </MdTemperatureDetails>
  <MdResourceFacts :facts="facts" class="cpu-details" :aria-label="t('cpuDetails.summary')" />
</template>
<style scoped>
.cpu-summary :deep(.resource-trend.cpu) {
  height: 28px;
}
</style>
