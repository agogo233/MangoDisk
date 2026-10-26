<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { SelectGroup, SelectLabel, SelectTrigger } from 'reka-ui';
import { Select, SelectContent, SelectItem } from '@/components/ui/select';
import MdTooltip from '@/components/custom/md-tooltip.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import type { ResidentPreferences } from '@/lib/models/resident/resident';
import type { ResourceDisk, ResourceVolume } from '@/lib/models/system-resources/system-resources';
import {
  diskDevices,
  diskDeviceVolumes,
  diskVolumeGroups,
  selectedDiskVolume,
} from '@/lib/utils/system-resources/disk-selection';

const props = defineProps<{
  temperature: boolean;
  volumeId: string | null;
  deviceId: string | null;
  volumes: ResourceVolume[];
  disabled: boolean;
  open: boolean;
}>();
const emit = defineEmits<{ change: [value: Partial<ResidentPreferences>]; 'update:open': [value: boolean] }>();
const { t } = useI18n({ useScope: 'global' });
const groups = computed(() => diskVolumeGroups(props.volumes));
const devices = computed(() => diskDevices(props.volumes));
const selectedVolume = computed(() => selectedDiskVolume(props.volumes, props.volumeId));
function deviceTitle(device: ResourceDisk): string {
  return `${t(device.number === null ? 'systemStatus.physicalDisk' : 'systemStatus.diskNumber', { number: device.number ?? '' })} · ${device.model ?? t('systemStatus.diskModelUnknown')}`;
}
function deviceLabel(device: ResourceDisk): string {
  const label = deviceTitle(device);
  const volumes = diskDeviceVolumes(device, props.volumes);
  return volumes.length ? `${label} (${volumes.join(', ')})` : label;
}
function volumeLabel(volume: ResourceVolume): string {
  const mount = volume.mountPoints[0];
  return mount && volume.name.replace(/\\$/, '') === mount ? `${t('systemStatus.localDisk')} (${mount})` : volume.name;
}
const follows = computed(() => (props.temperature ? props.deviceId === null : props.volumeId === null));
const missing = computed(
  () =>
    !follows.value &&
    (props.temperature
      ? !devices.value.some(device => device.id === props.deviceId)
      : !props.volumes.some(volume => volume.id === props.volumeId))
);
const label = computed(() => {
  if (follows.value) {
    if (props.temperature) return t('systemStatus.diskTemperatureFollow');
    const volume = selectedVolume.value;
    return volume
      ? t('systemStatus.systemDiskSelected', {
          name: volume.mountPoints.find(path => /^[a-z]:$/i.test(path)) ?? volume.name,
        })
      : t('systemStatus.systemDiskAutomatic');
  }
  if (missing.value) return t('systemStatus.savedDisconnected');
  return props.temperature
    ? deviceLabel(devices.value.find(device => device.id === props.deviceId)!)
    : volumeLabel(selectedVolume.value!);
});
const followedLabel = computed(() => {
  const volume = props.temperature ? selectedVolume.value : selectedDiskVolume(props.volumes, null);
  if (!volume) return t('systemStatus.savedDisconnected');
  if (!props.temperature) return volumeLabel(volume);
  if (volume.devices.length > 1) return t('systemStatus.spanningVolume');
  return volume.devices[0] ? deviceLabel(volume.devices[0]) : t('systemStatus.diskTopologyUnknown');
});
function select(value: unknown) {
  emit('update:open', false);
  emit(
    'change',
    props.temperature
      ? { diskTemperatureDevice: value === 'follow' ? null : String(value) }
      : { diskVolume: value === 'follow' ? null : String(value) }
  );
}
function canReadDevice(device: ResourceDisk) {
  return props.volumes.some(volume => volume.devices.length === 1 && volume.devices[0]?.id === device.id);
}
</script>

<template>
  <Select
    :open="open"
    :disabled="disabled"
    :model-value="(temperature ? deviceId : volumeId) ?? 'follow'"
    @update:open="emit('update:open', $event)"
    @update:model-value="select"
  >
    <SelectTrigger as-child>
      <MdTooltip :text="follows ? `${label}\n${followedLabel}` : label" multiline>
        <button
          type="button"
          class="selection-toggle"
          :aria-label="`${t(temperature ? 'systemStatus.physicalDisk' : 'systemStatus.volume')}: ${label}`"
        >
          <span class="truncate">{{ label }}</span
          ><MdIcon :name="open ? ICON_NAMES.chevronUp : ICON_NAMES.chevronDown" :size="14" class="shrink-0" />
        </button>
      </MdTooltip>
    </SelectTrigger>
    <SelectContent
      align="end"
      :class="['z-60', temperature ? 'max-w-[min(28rem,calc(100vw-2rem))]' : 'w-80 max-w-[calc(100vw-2rem)]']"
    >
      <SelectItem value="follow" class="disk-choice break-words">
        <span class="block">{{
          t(temperature ? 'systemStatus.diskTemperatureFollow' : 'systemStatus.systemDiskAutomatic')
        }}</span>
        <span v-if="temperature" class="block text-content-meta text-muted-foreground">{{
          t('systemStatus.currentDiskSource', { name: followedLabel })
        }}</span>
      </SelectItem>
      <template v-if="temperature">
        <SelectItem
          v-for="device in devices"
          :key="device.id"
          :value="device.id"
          :disabled="!canReadDevice(device)"
          class="disk-choice break-words"
        >
          {{ deviceLabel(device) }}
          <span v-if="!canReadDevice(device)" class="block text-content-meta">{{
            t('systemStatus.spanningVolume')
          }}</span>
        </SelectItem>
      </template>
      <template v-else>
        <SelectGroup
          v-for="(group, index) in groups"
          :key="group.id"
          :class="['disk-group', { 'disk-group-first': index === 0 }]"
        >
          <SelectLabel class="px-2 pt-2 pb-1 text-content-meta font-normal text-muted-foreground">
            <MdTooltip
              :text="group.devices.map(deviceTitle).join('\n') || t('systemStatus.diskTopologyUnknown')"
              multiline
            >
              <span class="block truncate">{{
                group.devices.length === 1
                  ? deviceTitle(group.devices[0]!)
                  : t(group.devices.length ? 'systemStatus.spanningVolume' : 'systemStatus.diskTopologyUnknown')
              }}</span>
            </MdTooltip>
          </SelectLabel>
          <SelectItem
            v-for="volume in group.volumes"
            :key="volume.id"
            :value="volume.id"
            class="disk-choice pl-4 break-words"
          >
            <span>{{ volumeLabel(volume) }}</span>
            <span
              v-if="volume.system"
              class="ml-2 rounded-sm border border-border px-1 py-0.5 text-content-meta text-muted-foreground"
              >{{ t('systemStatus.systemDisk') }}</span
            >
          </SelectItem>
        </SelectGroup>
      </template>
      <SelectItem v-if="missing" :value="(temperature ? deviceId : volumeId)!" class="disk-choice">{{
        t('systemStatus.savedDisconnected')
      }}</SelectItem>
    </SelectContent>
  </Select>
</template>

<style scoped>
@reference '@/assets/main.css';
.disk-group {
  @apply mt-1;
}
.disk-group-first {
  @apply mt-1 border-t border-border pt-1;
}
.disk-choice[data-state='checked'] {
  @apply bg-accent text-accent-foreground;
}
.disk-choice[data-highlighted] {
  background-color: var(--popover-hover);
  @apply text-foreground;
}
.selection-toggle {
  @apply text-muted-foreground rounded;
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  max-width: 55%;
  margin-inline-start: auto;
  height: 32px;
  padding-inline: 8px;
  font-size: var(--font-content-body);
  cursor: pointer;
}
.selection-toggle:hover {
  @apply bg-accent text-foreground;
}
.selection-toggle:focus-visible {
  outline: 2px solid var(--ring);
}
.selection-toggle:disabled {
  opacity: 0.4;
  cursor: default;
}
@media (pointer: coarse) {
  .selection-toggle {
    min-height: 44px;
  }
}
</style>
