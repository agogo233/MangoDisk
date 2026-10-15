<script setup lang="ts">
import type { CpuTemperature, MetricReading } from '@/lib/models/system-resources';
import MdResourceTemperature from './md-resource-temperature.vue';
withDefaults(
  defineProps<{
    reading: MetricReading<CpuTemperature>;
    observedAtMs: number;
    compact?: boolean;
    interactive?: boolean;
    heading?: string;
  }>(),
  { compact: false, interactive: false, heading: undefined }
);
defineEmits<{ activate: [] }>();
</script>
<template>
  <MdResourceTemperature
    class="cpu-temperature"
    metric="cpu"
    :reading="reading"
    :observed-at-ms="observedAtMs"
    :compact="compact"
    :interactive="interactive"
    :heading="heading"
    @activate="$emit('activate')"
  />
</template>
