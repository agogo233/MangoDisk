<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Select, SelectContent, SelectItem } from '@/components/ui/select';
import { SelectTrigger } from 'reka-ui';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import type { ResidentPreferences, ResidentReading } from '@/lib/models/resident/resident';

type FanMode = ResidentPreferences['fanDisplay'];
type FanPreferences = Pick<ResidentPreferences, 'fanDisplay' | 'fanIds'>;
const props = defineProps<{
  mode: FanMode;
  ids: number[];
  reading: ResidentReading['fanSpeed'];
  disabled: boolean;
  aggregateOnly?: boolean;
  open: boolean;
}>();
const emit = defineEmits<{
  change: [value: FanPreferences];
  'update:open': [value: boolean];
}>();
const { t } = useI18n({ useScope: 'global' });
const modeKeys = {
  highest: 'systemStatus.fanAutomatic',
  average: 'systemStatus.fanAverage',
  selected: 'systemStatus.fanSelected',
} as const;
const modes = computed(() => Object.entries(modeKeys).filter(([mode]) => !props.aggregateOnly || mode !== 'selected'));
const knownFanIds = ref<number[] | null>(null);
watch(
  () => props.reading,
  reading => {
    // A failed sample is not evidence that a physical fan disappeared.
    if (reading.status === 'ready' && reading.value) {
      knownFanIds.value = reading.value.fans.map(fan => fan.id);
    }
  },
  { immediate: true }
);
const canSelect = computed(() => {
  // Disabled Windows sampling has no useful aggregation control to expose.
  if (props.aggregateOnly && props.disabled) return false;
  const known = knownFanIds.value;
  return known === null || known.length > 1 || (props.mode === 'selected' && props.ids.some(id => !known.includes(id)));
});
watch(
  canSelect,
  visible => {
    if (!visible && props.open) emit('update:open', false);
  },
  { immediate: true }
);
const choices = computed(() => {
  const fans = props.reading.status === 'ready' ? (props.reading.value?.fans ?? []) : [];
  return [...new Set([...(knownFanIds.value ?? []), ...props.ids])]
    .sort((a, b) => a - b)
    .map(id => {
      const fan = fans.find(fan => fan.id === id);
      return {
        id,
        label: t('systemStatus.fanNumber', { number: id + 1 }),
        value: fan?.rpm == null ? '—' : Math.round(fan.rpm).toString(),
        missing: props.reading.status === 'ready' && props.reading.value !== null && !fan,
      };
    });
});
function selectMode(value: unknown) {
  if (props.aggregateOnly && value === 'selected') return;
  if (value !== 'highest' && value !== 'average' && value !== 'selected') return;
  emit('update:open', false);
  emit('change', { fanDisplay: value, fanIds: [...props.ids] });
}
function locked(id: number) {
  return props.disabled || (props.ids.includes(id) ? props.ids.length === 1 : props.ids.length === 2);
}
function toggle(id: number, checked: boolean) {
  if (locked(id) || props.ids.includes(id) === checked) return;
  const ids = checked ? [...props.ids, id] : props.ids.filter(selected => selected !== id);
  emit('change', { fanDisplay: 'selected', fanIds: ids.sort((a, b) => a - b) });
}
</script>

<template>
  <!-- The parent metric heading owns the shared header grid. -->
  <div class="contents" data-fan-settings>
    <Select
      v-if="canSelect"
      :open="open"
      :disabled="disabled"
      :model-value="mode"
      @update:open="emit('update:open', $event)"
      @update:model-value="selectMode"
    >
      <SelectTrigger as-child>
        <button
          type="button"
          class="selection-toggle inline-flex h-8 min-w-0 cursor-pointer items-center justify-self-end gap-1.5 rounded px-2 text-content-body text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring disabled:cursor-default disabled:opacity-40"
          :aria-label="`${t('systemStatus.fanSpeed')}: ${t(modeKeys[mode === 'selected' && aggregateOnly ? 'highest' : mode])}`"
        >
          <span class="truncate">{{ t(modeKeys[mode === 'selected' && aggregateOnly ? 'highest' : mode]) }}</span>
          <MdIcon :name="ICON_NAMES.chevronDown" :size="14" class="shrink-0" />
        </button>
      </SelectTrigger>
      <SelectContent align="end" class="z-60 max-w-[min(20rem,calc(100vw-2rem))]">
        <SelectItem v-for="[value, key] in modes" :key="value" :value="value">{{ t(key) }}</SelectItem>
      </SelectContent>
    </Select>
    <div v-if="canSelect && mode === 'selected' && !aggregateOnly" class="fan-options">
      <div
        class="fan-options-list"
        :class="{ 'fan-options-paired': choices.length > 1 }"
        role="group"
        :aria-label="t('systemStatus.fanSelectionLimit')"
      >
        <label v-for="fan in choices" :key="fan.id" class="fan-option" :for="`fan-choice-${fan.id}`">
          <MdCheckbox
            :id="`fan-choice-${fan.id}`"
            :model-value="ids.includes(fan.id)"
            :disabled="locked(fan.id)"
            @update:model-value="toggle(fan.id, $event === true)"
          />
          <span class="min-w-0 break-words"
            >{{ fan.label }}<span v-if="fan.missing"> · {{ t('systemStatus.disconnected') }}</span></span
          >
          <span class="ml-auto shrink-0 pl-2 font-mono tabular-nums">{{ fan.value }}</span>
        </label>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";

.selection-toggle {
  grid-column: 3;
  grid-row: 1;
}
.fan-options {
  grid-row: 2;
  grid-column: 2 / -1;
  min-width: 0;
  padding: 2px 0 4px 27px;
  container: fan-options / inline-size;
}
.fan-options-list {
  @apply rounded-md bg-muted/40;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 4px 16px;
  max-height: 160px;
  overflow-y: auto;
  padding: 4px 10px;
}
.fan-option {
  @apply text-foreground;
  display: flex;
  align-items: center;
  min-width: 0;
  min-height: 32px;
  gap: 10px;
  font-size: var(--font-content-body);
  cursor: pointer;
}
@container fan-options (min-width: 360px) {
  .fan-options-list.fan-options-paired {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
