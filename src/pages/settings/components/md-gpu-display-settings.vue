<script setup lang="ts">
import { computed, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import MdTooltip from '@/components/custom/md-tooltip.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Select, SelectContent, SelectItem } from '@/components/ui/select';
import { SelectTrigger } from 'reka-ui';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import type { GpuDisplaySelection, ResidentPreferences } from '@/lib/models/resident/resident';
import type { GpuDeviceReading } from '@/lib/models/system-resources/system-resources';
import { gpuDeviceMetrics } from '@/lib/utils/system-resources/gpu-device-metrics';

const props = defineProps<{
  temperature: boolean;
  selection: GpuDisplaySelection;
  adapters: { id: string; name: string }[];
  devices: GpuDeviceReading[];
  observedAtMs: number;
  disabled: boolean;
  open: boolean;
}>();
const emit = defineEmits<{ change: [value: Partial<ResidentPreferences>]; 'update:open': [value: boolean] }>();
const { t } = useI18n({ useScope: 'global' });
const automatic = computed(() => (props.temperature ? 'followUsage' : 'automatic'));
const explicit = computed(() => props.selection.mode === 'selected');
const ids = computed(() => (props.selection.mode === 'selected' ? props.selection.adapterIds : []));
const missingIds = computed(() => ids.value.filter(id => !props.adapters.some(adapter => adapter.id === id)));
const canSelect = computed(() => props.adapters.length > 1 || missingIds.value.length > 0);
watch(
  canSelect,
  visible => {
    if (!visible && props.open) emit('update:open', false);
  },
  { immediate: true }
);
const label = computed(() =>
  explicit.value
    ? t('gpuDetails.selectedAdapters')
    : t(props.temperature ? 'gpuTemperature.followUsage' : 'systemStatus.automatic')
);
const choices = computed(() =>
  [...props.adapters, ...missingIds.value.map(id => ({ id, name: t('systemStatus.savedDisconnected') }))].map(
    adapter => {
      const device = props.devices.find(device => device.adapterId === adapter.id);
      const metrics = device ? gpuDeviceMetrics(device, props.observedAtMs) : null;
      const value = props.temperature ? metrics?.celsius : metrics?.usedPercent;
      return {
        ...adapter,
        value: value == null ? '—' : `${value.toFixed(0)}${props.temperature ? '°C' : '%'}`,
        missing: !props.adapters.some(item => item.id === adapter.id),
      };
    }
  )
);
function commit(selection: GpuDisplaySelection) {
  emit('change', props.temperature ? { gpuTemperatureDisplay: selection } : { gpuDisplay: selection });
}
function select(value: unknown) {
  emit('update:open', false);
  if (value === automatic.value) commit({ mode: 'automatic' });
  else if (value === 'selected') {
    const selected = ids.value.length ? ids.value : props.adapters.slice(0, 1).map(adapter => adapter.id);
    if (selected.length) commit({ mode: 'selected', adapterIds: selected });
  }
}
function locked(id: string) {
  return props.disabled || (ids.value.includes(id) && ids.value.length === 1);
}
function toggle(id: string, checked: boolean) {
  if (locked(id) || ids.value.includes(id) === checked) return;
  const selected = checked ? [...ids.value, id] : ids.value.filter(value => value !== id);
  if (selected.length) commit({ mode: 'selected', adapterIds: selected });
}
</script>
<template>
  <div class="contents" data-gpu-settings>
    <Select
      v-if="canSelect"
      :open="open"
      :disabled="disabled"
      :model-value="explicit ? 'selected' : automatic"
      @update:open="emit('update:open', $event)"
      @update:model-value="select"
    >
      <SelectTrigger as-child>
        <button
          type="button"
          class="selection-toggle"
          :aria-label="`${t(temperature ? 'gpuTemperature.label' : 'systemStatus.gpu')}: ${label}`"
        >
          <span class="truncate">{{ label }}</span
          ><MdIcon :name="ICON_NAMES.chevronDown" :size="14" class="shrink-0" />
        </button>
      </SelectTrigger>
      <SelectContent align="end" class="z-60 max-w-[min(20rem,calc(100vw-2rem))]">
        <SelectItem :value="automatic">{{
          t(temperature ? 'gpuTemperature.followUsage' : 'systemStatus.automatic')
        }}</SelectItem>
        <SelectItem value="selected">{{ t('gpuDetails.selectedAdapters') }}</SelectItem>
      </SelectContent>
    </Select>
    <div v-if="canSelect && explicit" class="gpu-options">
      <div
        class="gpu-options-list"
        role="group"
        :aria-label="t(temperature ? 'gpuTemperature.label' : 'systemStatus.gpu')"
      >
        <label
          v-for="(adapter, index) in choices"
          :key="adapter.id"
          class="gpu-option"
          :for="`gpu-choice-${temperature ? 'temperature' : 'usage'}-${index}`"
        >
          <MdCheckbox
            :id="`gpu-choice-${temperature ? 'temperature' : 'usage'}-${index}`"
            :data-adapter-id="adapter.id"
            :model-value="ids.includes(adapter.id)"
            :disabled="locked(adapter.id)"
            @update:model-value="toggle(adapter.id, $event === true)"
          />
          <MdTooltip :text="adapter.name"
            ><span class="min-w-0 truncate"
              >{{ adapter.name }}<span v-if="adapter.missing"> · {{ t('systemStatus.disconnected') }}</span></span
            ></MdTooltip
          >
          <span class="ml-auto shrink-0 pl-2 font-mono tabular-nums">{{ adapter.value }}</span>
        </label>
      </div>
    </div>
  </div>
</template>
<style scoped>
@reference "@assets/main.css";
.selection-toggle {
  @apply inline-flex h-8 min-w-0 cursor-pointer items-center justify-self-end gap-1.5 rounded px-2 text-content-body text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring disabled:cursor-default disabled:opacity-40;
  grid-column: 3;
  grid-row: 1;
}
.gpu-options {
  grid-row: 2;
  grid-column: 2 / -1;
  min-width: 0;
  padding: 2px 0 4px 27px;
  container: gpu-options / inline-size;
}
.gpu-options-list {
  @apply rounded-md bg-muted/40;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 4px 16px;
  max-height: 160px;
  overflow-y: auto;
  padding: 4px 10px;
}
.gpu-option {
  @apply text-foreground;
  display: flex;
  align-items: center;
  min-width: 0;
  min-height: 32px;
  gap: 10px;
  font-size: var(--font-content-body);
  cursor: pointer;
}
@container gpu-options (min-width: 480px) {
  .gpu-options-list {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
