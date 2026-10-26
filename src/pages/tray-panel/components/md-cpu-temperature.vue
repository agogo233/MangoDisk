<script setup lang="ts">
import type { CpuTemperature, MetricReading } from '@/lib/models/system-resources/system-resources';
import MdResourceTemperature from './md-resource-temperature.vue';
withDefaults(
  defineProps<{
    reading: MetricReading<CpuTemperature>;
    observedAtMs: number;
    active?: boolean;
    compact?: boolean;
    interactive?: boolean;
    heading?: string;
    showStatus?: boolean;
  }>(),
  { active: true, compact: false, interactive: false, heading: undefined, showStatus: true }
);
defineEmits<{ activate: [] }>();
</script>
<template>
  <MdResourceTemperature
    class="cpu-temperature"
    metric="cpu"
    :active="active"
    :reading="reading"
    :observed-at-ms="observedAtMs"
    :compact="compact"
    :interactive="interactive"
    :heading="heading"
    :show-status="showStatus"
    @activate="$emit('activate')"
  />
</template>
