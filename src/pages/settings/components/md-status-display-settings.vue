<script setup lang="ts">
import MdTooltip from '@/components/custom/md-tooltip.vue';
import { METRIC_LABEL_KEYS } from '@/lib/models/system-resources';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdCheckbox from '@/components/custom/md-checkbox.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import MdIconMangodisk from '@/components/icons/md-icon-mangodisk.vue';
import MdSettingsRow from '@/components/custom/md-settings-row.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdWindowsDisplayFeedback from './md-windows-display-feedback.vue';
import MdWindowsTemperatureSetup from './md-windows-temperature-setup.vue';
import { Select, SelectContent, SelectItem } from '@/components/ui/select';
import { SelectTrigger } from 'reka-ui';
import MdTaskbarPosition from './md-taskbar-position.vue';
import MdStatusAppearance from './md-status-appearance.vue';
import MdStatusColorRuleEditor from './md-status-color-rule-editor.vue';
import type { ColorMetricId, MetricColorRule } from '@/lib/models/resident-color-rules';
import { ICON_NAMES } from '@/lib/models/ui';
import type { NetworkInterface, ResourceVolume } from '@/lib/models/system-resources';
import type { ResidentPreferences, ResidentReading, ResidentDisplayMetricId } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';
import { useResidentSettingsStore } from '@/stores/resident-settings-store';
import { useWindowsMonitoringStore } from '@/stores/windows-monitoring-store';
import { LoggerService } from '@/lib/services/logger-service';

const props = withDefaults(defineProps<{ isMacOs: boolean; isLinux?: boolean }>(), { isLinux: false });
const { t } = useI18n({ useScope: 'global' });
const settings = useResidentSettingsStore();
const monitoring = useWindowsMonitoringStore();
const temperatureSelectionPending = ref(false);
const panelOpening = ref(false);
const panelError = ref(false);
async function openPanel() {
  panelOpening.value = true;
  panelError.value = false;
  try {
    await ResidentService.openPanel();
  } catch {
    panelError.value = true;
    LoggerService.warn('monitoring', 'resident_panel_open_failed');
  } finally {
    panelOpening.value = false;
  }
}
const interfaces = ref<NetworkInterface[]>([]);
const volumes = ref<ResourceVolume[]>([]);
const gpuAdapters = ref<{ id: string; name: string }[]>([]);
const catalogueError = ref(false);
const gpuTemperatureStatus = ref<ResidentReading['gpuTemperature']['status']>('loading');
const batteryPresent = ref<boolean | null>(null);
const batteryTemperatureStatus = ref<ResidentReading['batteryTemperature']['status']>('loading');
const chargingPowerStatus = ref<ResidentReading['chargingPower']['status']>('loading');
const systemPowerStatus = ref<ResidentReading['systemPower']['status']>('loading');
const batteryPowerStatus = ref<ResidentReading['batteryPower']['status']>('loading');
function batteryHint(id: ResidentDisplayMetricId) {
  const unavailable =
    id === 'batteryTemperature'
      ? batteryTemperatureStatus.value === 'unsupported'
      : id === 'batteryPower'
        ? batteryPowerStatus.value === 'unsupported'
        : chargingPowerStatus.value === 'unsupported';
  return t(
    unavailable
      ? 'systemStatus.batteryUnsupportedHint'
      : id === 'batteryPower'
        ? 'systemStatus.batteryPowerHint'
        : 'systemStatus.chargingPowerHint'
  );
}
const dragging = ref<ResidentDisplayMetricId | null>(null);
const pointerDragging = ref(false);
const dragRows = ref<ResidentPreferences['metrics'] | null>(null);
const announcement = ref('');
const metricRowsElement = ref<HTMLElement | null>(null);
const rows = computed(() => dragRows.value ?? settings.draft?.metrics ?? []);
function isBatteryMetric(id: ResidentDisplayMetricId | null): boolean {
  return id === 'batteryTemperature' || id === 'chargingPower' || id === 'batteryPower';
}
const displayLabelKeys = {
  ...METRIC_LABEL_KEYS,
  cpuTemperature: 'cpuTemperature.label',
  gpuTemperature: 'gpuTemperature.label',
  diskTemperature: 'systemStatus.diskTemperature',
  batteryTemperature: 'systemStatus.batteryTemperature',
  chargingPower: 'systemStatus.chargingPower',
  batteryPower: 'systemStatus.batteryPower',
  systemPower: 'systemStatus.systemPower',
};
const visibleRows = computed(() =>
  rows.value
    .filter(
      row =>
        (batteryPresent.value !== false || !isBatteryMetric(row.id)) &&
        (!props.isLinux || row.id !== 'gpuTemperature') &&
        (!props.isLinux || row.id !== 'cpuTemperature') &&
        (!props.isLinux || row.id !== 'diskTemperature') &&
        (!props.isLinux || row.id !== 'gpu') &&
        (props.isMacOs || (!isBatteryMetric(row.id) && row.id !== 'systemPower'))
    )
    .map(row =>
      row.id === 'cpuTemperature' && !props.isMacOs && !settings.draft?.windowsCpuTemperatureEnabled
        ? { ...row, enabled: false }
        : row
    )
);
// Older preferences may have every item cleared. Mirror the native Logo
// fallback without writing on load, and retain it when the next metric is added.
const showIcon = computed(() => (settings.draft?.showIcon ?? true) || !visibleRows.value.some(row => row.enabled));
const selectedCount = computed(() => Number(showIcon.value) + visibleRows.value.filter(row => row.enabled).length);
function selectionLocked(selected: boolean) {
  return !settings.draft || (selected && selectedCount.value <= 1);
}
function enableIcon(enabled: boolean) {
  if (!enabled && selectionLocked(showIcon.value)) return;
  void settings.change({ showIcon: enabled });
}
const settingsOpen = ref(false);
const temperatureSetupOpen = ref(false);
const selectTemperatureAfterSetup = ref(false);
const activeColor = ref<ColorMetricId | null>(null);
watch(batteryPresent, present => {
  if (present !== false) return;
  if (isBatteryMetric(activeColor.value)) activeColor.value = null;
  if (isBatteryMetric(dragging.value)) cancelDrag();
});
function openColor(id: ColorMetricId) {
  cancelDrag();
  expandedSelection.value = null;
  activeColor.value = id;
}
async function saveColor(rule: MetricColorRule) {
  if (!settings.draft) return;
  await settings.change({ colorRules: settings.draft.colorRules.map(saved => (saved.id === rule.id ? rule : saved)) });
  if (!settings.error) activeColor.value = null;
}
const expandedSelection = ref<'network' | 'disk' | 'gpu' | 'gpuTemperature' | 'diskTemperature' | null>(null);
// Keep the native switch and collapsed content on the committed state. Failed
// writes must not hide the user's controls or leave a misleading enabled state.
const displayEnabled = computed(() => settings.preferences?.enabled ?? false);
async function setDisplayEnabled(enabled: boolean) {
  cancelDrag();
  expandedSelection.value = null;
  await settings.change({ enabled });
}
watch(displayEnabled, enabled => {
  if (!enabled) {
    settingsOpen.value = false;
    cancelDrag();
    expandedSelection.value = null;
    activeColor.value = null;
  }
});
watch(settingsOpen, open => {
  if (open) {
    // The detail panel can change the shared GPU choice in another WebView.
    void settings.load();
    void loadCatalogue();
    if (!props.isMacOs && !props.isLinux) void monitoring.load();
  } else {
    // A dismissed editor must not leave a floating drag preview or a pending
    // reorder that could be committed after reopening. Saved choices stay intact.
    cancelDrag();
    expandedSelection.value = null;
    activeColor.value = null;
  }
});
function handleDialogEscape(event: KeyboardEvent) {
  if (dragging.value) {
    event.preventDefault();
    cancelDrag();
  }
}
function restoreConfigurationFocus(event: Event) {
  if (disposed) return;
  event.preventDefault();
  document.getElementById(displayEnabled.value ? 'resident-configure' : 'resident-enabled')?.focus();
}
function setSelectionOpen(id: ResidentDisplayMetricId, open: boolean) {
  if (id !== 'network' && id !== 'disk' && id !== 'gpu' && id !== 'gpuTemperature' && id !== 'diskTemperature') return;
  if (open) {
    expandedSelection.value = id;
  } else if (expandedSelection.value === id) expandedSelection.value = null;
}
watch(rows, value => {
  if (expandedSelection.value && !value.some(row => row.id === expandedSelection.value && row.enabled)) {
    expandedSelection.value = null;
  }
});
const missingInterface = computed(
  () => settings.draft?.networkInterface && !interfaces.value.some(item => item.id === settings.draft?.networkInterface)
);
const missingVolume = computed(
  () => settings.draft?.diskVolume && !volumes.value.some(item => item.id === settings.draft?.diskVolume)
);
const missingGpu = computed(
  () => settings.draft?.gpuAdapter && !gpuAdapters.value.some(item => item.id === settings.draft?.gpuAdapter)
);
const selectedGpuLabel = computed(() => {
  const id = settings.draft?.gpuAdapter;
  if (!id) return t('systemStatus.automatic');
  return gpuAdapters.value.find(item => item.id === id)?.name ?? t('systemStatus.savedDisconnected');
});
// Select's inferred item text can outlive a locale or catalogue update while
// its popup is closed. Keep the displayed selection reactive without reopening it.
const selectedInterfaceLabel = computed(() => {
  const id = settings.draft?.networkInterface;
  if (!id) return t('systemStatus.automatic');
  const item = interfaces.value.find(item => item.id === id);
  if (!item) return t('systemStatus.savedDisconnected');
  return `${item.name}${item.connected ? '' : ` · ${t('systemStatus.disconnected')}`}`;
});
const selectedVolumeLabel = computed(() => {
  const id = settings.draft?.diskVolume;
  if (!id) return t('systemStatus.systemDisk');
  return volumes.value.find(item => item.id === id)?.name ?? t('systemStatus.savedDisconnected');
});
let disposed = false;
let unlisten: (() => void) | null = null;
let revision = -1;
function accept(value: ResidentReading) {
  if (disposed || value.schemaVersion !== 22 || value.revision < revision) return;
  revision = value.revision;
  interfaces.value = value.interfaces;
  volumes.value = value.volumes;
  gpuAdapters.value = value.gpuAdapters;
  gpuTemperatureStatus.value = value.gpuTemperature.status;
  batteryPresent.value = value.batteryPresent;
  batteryTemperatureStatus.value = value.batteryTemperature.status;
  chargingPowerStatus.value = value.chargingPower.status;
  batteryPowerStatus.value = value.batteryPower.status;
  systemPowerStatus.value = value.systemPower.status;
}
async function enable(id: ResidentDisplayMetricId, enabled: boolean) {
  if (id === 'gpuTemperature' && enabled && gpuTemperatureStatus.value === 'unsupported') return;
  if (id === 'cpuTemperature' && !props.isMacOs && !props.isLinux && enabled) {
    if (temperatureSelectionPending.value || monitoring.loading || monitoring.busy) return;
    temperatureSelectionPending.value = true;
    try {
      await monitoring.load();
      if (disposed || !settingsOpen.value || !displayEnabled.value) return;
      if (monitoring.loading || monitoring.busy) return;
      if (monitoring.observation?.state !== 'ready' || !settings.preferences?.windowsCpuTemperatureEnabled) {
        selectTemperatureAfterSetup.value = true;
        temperatureSetupOpen.value = true;
        return;
      }
    } finally {
      temperatureSelectionPending.value = false;
    }
  }
  if (!enabled && selectionLocked(rows.value.some(row => row.id === id && row.enabled))) return;
  cancelDrag();
  void settings.change({
    showIcon: showIcon.value,
    metrics: rows.value.map(metric => (metric.id === id ? { ...metric, enabled } : { ...metric })),
  });
}
function begin(id: ResidentDisplayMetricId) {
  if (!settings.draft) return;
  dragging.value = id;
  dragRows.value = settings.draft.metrics.map(metric => ({ ...metric }));
  announce();
}
function move(target: ResidentDisplayMetricId) {
  if (!dragRows.value || !dragging.value || target === dragging.value) return;
  const from = dragRows.value.findIndex(row => row.id === dragging.value);
  const to = dragRows.value.findIndex(row => row.id === target);
  if (from < 0 || to < 0) return;
  const next = [...dragRows.value];
  const [item] = next.splice(from, 1);
  if (item) next.splice(to, 0, item);
  dragRows.value = next;
  if (!pointer) restoreHandleFocus(dragging.value);
  announce();
}
function restoreHandleFocus(id: ResidentDisplayMetricId) {
  // WebKit drops focus when Vue moves a keyed row. Restore it after the DOM
  // update so subsequent arrows, Enter and Escape still reach the same handle.
  void nextTick(() =>
    metricRowsElement.value
      ?.querySelector<HTMLButtonElement>(`[data-metric="${id}"] .drag-handle`)
      ?.focus({ preventScroll: true })
  );
}
function announce() {
  announcement.value = t('systemStatus.moveAnnouncement', {
    name: t(displayLabelKeys[dragging.value!]),
    position: visibleRows.value.findIndex(row => row.id === dragging.value) + 1,
    count: visibleRows.value.length,
  });
}
let pointer: {
  id: ResidentDisplayMetricId;
  pointerId: number;
  x: number;
  y: number;
  root: HTMLElement;
  row: HTMLElement;
  bounds: DOMRect;
  slots: DOMRect[];
} | null = null;
let dragPreview: HTMLElement | null = null;
function createDragPreview() {
  if (!pointer) return;
  // Clone only the presentation. The inert copy has no duplicate IDs, focusable
  // controls or accessible content; the original row remains the drop placeholder.
  dragPreview = pointer.row.cloneNode(true) as HTMLElement;
  dragPreview.classList.add('drag-preview');
  dragPreview.classList.remove('moving');
  dragPreview.inert = true;
  dragPreview.setAttribute('aria-hidden', 'true');
  dragPreview.removeAttribute('data-metric');
  dragPreview.querySelectorAll('[id]').forEach(element => element.removeAttribute('id'));
  dragPreview.querySelectorAll('label').forEach(element => element.removeAttribute('for'));
  Object.assign(dragPreview.style, {
    left: `${pointer.bounds.left}px`,
    top: `${pointer.bounds.top}px`,
    width: `${pointer.bounds.width}px`,
    height: `${pointer.bounds.height}px`,
  });
  document.body.append(dragPreview);
  pointerDragging.value = true;
}
function escapePointerDrag(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelDrag();
  }
}
function pointerDown(event: PointerEvent, id: ResidentDisplayMetricId) {
  if (event.button !== 0) return;
  cancelDrag();
  const handle = event.currentTarget as HTMLElement;
  const root = handle.closest<HTMLElement>('.metric-rows');
  const row = handle.closest<HTMLElement>('.metric-row');
  if (!root || !row) return;
  event.preventDefault();
  expandedSelection.value = null;
  pointer = {
    id,
    pointerId: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    root,
    row,
    bounds: row.getBoundingClientRect(),
    // Hit-test stable list slots, not the animated rows passing under the pointer,
    // so adjacent items cannot repeatedly swap while making room for the drop.
    slots: [...root.querySelectorAll<HTMLElement>('.metric-row')].map(item => item.getBoundingClientRect()),
  };
  // Native Tauri file drops must stay enabled for cleanup pages. Pointer events
  // keep this local reorder independent of the intercepted HTML drag pipeline.
  window.addEventListener('pointermove', pointerMove);
  window.addEventListener('pointerup', pointerUp);
  window.addEventListener('pointercancel', cancelDrag);
  window.addEventListener('blur', cancelDrag);
  window.addEventListener('keydown', escapePointerDrag);
  window.addEventListener('resize', cancelDrag);
  window.addEventListener('scroll', cancelDrag, true);
}
function pointerMove(event: PointerEvent) {
  if (!pointer || event.pointerId !== pointer.pointerId) return;
  if (!dragging.value && Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y) >= 4) {
    createDragPreview();
    begin(pointer.id);
  }
  if (!dragging.value) return;
  const dx = event.clientX - pointer.x;
  const dy = event.clientY - pointer.y;
  if (dragPreview) dragPreview.style.transform = `translate3d(${dx}px, ${dy}px, 0)`;
  const x = pointer.bounds.left + pointer.bounds.width / 2 + dx;
  const y = pointer.bounds.top + pointer.bounds.height / 2 + dy;
  const index = pointer.slots.findIndex(slot => x >= slot.left && x <= slot.right && y >= slot.top && y <= slot.bottom);
  const target = visibleRows.value[index];
  if (target) move(target.id);
}
function pointerUp(event: PointerEvent) {
  if (!pointer || event.pointerId !== pointer.pointerId) return;
  const target = document.elementFromPoint(event.clientX, event.clientY);
  if (dragging.value && target && pointer.root.contains(target)) commit();
  else cancelDrag();
}
function cancelDrag() {
  if (dragging.value && !pointer) restoreHandleFocus(dragging.value);
  dragPreview?.remove();
  dragPreview = null;
  pointerDragging.value = false;
  pointer = null;
  window.removeEventListener('pointermove', pointerMove);
  window.removeEventListener('pointerup', pointerUp);
  window.removeEventListener('pointercancel', cancelDrag);
  window.removeEventListener('blur', cancelDrag);
  window.removeEventListener('keydown', escapePointerDrag);
  window.removeEventListener('resize', cancelDrag);
  window.removeEventListener('scroll', cancelDrag, true);
  dragging.value = null;
  dragRows.value = null;
}
function commit() {
  const metrics = dragRows.value;
  cancelDrag();
  if (metrics && metrics.some((row, index) => row.id !== settings.draft?.metrics[index]?.id)) {
    void settings.change({ metrics });
  }
}
function key(event: KeyboardEvent, id: ResidentDisplayMetricId) {
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelDrag();
  } else if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    if (dragging.value) commit();
    else begin(id);
  } else if (dragging.value && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) {
    event.preventDefault();
    const index = visibleRows.value.findIndex(row => row.id === dragging.value);
    // Up/down follows the visible list; retain left/right as equivalent shortcuts.
    const next = visibleRows.value[index + (event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1)];
    if (next) move(next.id);
  }
}
let catalogueLoading = false;
async function loadCatalogue() {
  // Reopening while the initial subscription is pending must not create a
  // second listener whose cleanup handle would overwrite the first one.
  if (catalogueLoading) return;
  catalogueLoading = true;
  catalogueError.value = false;
  try {
    // A retry must restore the live subscription as well as the cached catalogue.
    if (!unlisten) {
      const dispose = await ResidentService.onReading(accept);
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
    }
    accept(await ResidentService.catalogue());
  } catch {
    if (!disposed) catalogueError.value = true;
  } finally {
    catalogueLoading = false;
  }
}
onMounted(() => {
  if (!settings.preferences) void settings.load();
});
onBeforeUnmount(() => {
  disposed = true;
  cancelDrag();
  unlisten?.();
});
</script>

<template>
  <div class="status-settings">
    <MdSettingsRow
      :title="t(isMacOs ? 'systemStatus.menuBarEnabled' : 'systemStatus.displayEnabled')"
      :description="
        t(isMacOs ? 'systemStatus.menuBarHint' : isLinux ? 'systemStatus.linuxDisplayHint' : 'systemStatus.displayHint')
      "
      title-id="resident-enabled-label"
      description-id="resident-enabled-hint"
    >
      <template #icon><MdIcon :name="isMacOs ? ICON_NAMES.menuBar : ICON_NAMES.taskbar" /></template>
      <Button
        v-if="displayEnabled"
        id="resident-open-panel"
        variant="ghost"
        size="sm"
        class="text-muted-foreground"
        :disabled="settings.loading || settings.saving || panelOpening"
        @click="openPanel"
        >{{ t('systemStatus.details') }}</Button
      >
      <Button
        v-if="displayEnabled"
        id="resident-configure"
        variant="ghost"
        size="sm"
        class="text-muted-foreground"
        :disabled="settings.loading || settings.saving"
        aria-haspopup="dialog"
        @click="settingsOpen = true"
        >{{ t('systemStatus.configureAction') }}</Button
      >
      <MdSwitch
        id="resident-enabled"
        :model-value="displayEnabled"
        :disabled="settings.loading || settings.saving || !settings.preferences"
        aria-labelledby="resident-enabled-label"
        aria-describedby="resident-enabled-hint"
        @update:model-value="setDisplayEnabled"
      />
    </MdSettingsRow>
    <MdWindowsDisplayFeedback
      v-if="!isMacOs && !isLinux && settings.preferences"
      class="display-feedback"
      :preferences="settings.preferences"
    />
    <p v-if="settings.error && !settingsOpen" class="settings-feedback display-feedback" role="status">
      {{ t('systemStatus.saveFailed') }}
      <button @click="settings.load()">{{ t('monitoring.refresh') }}</button>
    </p>
    <p v-if="panelError" class="settings-feedback display-feedback" role="alert">
      {{ t('monitoring.unavailable') }}
    </p>
    <Dialog :open="settingsOpen && displayEnabled" @update:open="settingsOpen = $event">
      <MdDialogContent
        class="flex min-h-0 flex-col"
        @interact-outside.prevent
        @escape-key-down="handleDialogEscape"
        @close-auto-focus="restoreConfigurationFocus"
      >
        <MdDialogHeader class="flex-none border-b border-border/70">
          <DialogTitle>{{
            t(isMacOs ? 'systemStatus.menuBarSettingsTitle' : 'systemStatus.displaySettingsTitle')
          }}</DialogTitle>
        </MdDialogHeader>
        <div class="min-h-0 overflow-y-auto p-5">
          <div
            id="resident-display-options"
            class="display-options"
            :inert="settings.loading"
            :aria-busy="settings.loading"
          >
            <MdTaskbarPosition
              v-if="!isMacOs && !isLinux && settings.draft"
              :preferences="settings.draft"
              @position="settings.change({ taskbarPosition: $event })"
            />
            <MdStatusAppearance
              v-if="settings.draft"
              :preferences="settings.draft"
              :is-mac-os="isMacOs"
              :is-linux="isLinux"
              @change="settings.change($event)"
            />
            <div class="status-controls">
              <div class="controls-heading">
                <h3>{{ t('systemStatus.displayItems') }}</h3>
                <span>{{ t('systemStatus.dragToReorder') }}</span>
              </div>
              <div ref="metricRowsElement" @keydown.esc="cancelDrag">
                <TransitionGroup name="metric-sort" tag="div" class="metric-rows">
                  <div key="logo" class="status-item">
                    <span class="logo-marker" aria-hidden="true">
                      <MdIconMangodisk :size="18" />
                    </span>
                    <label class="logo-label" for="status-app-icon">
                      <MdCheckbox
                        id="status-app-icon"
                        :model-value="showIcon"
                        :disabled="selectionLocked(showIcon)"
                        @update:model-value="enableIcon($event === true)"
                      />
                      {{ t('systemStatus.showIcon') }}
                    </label>
                  </div>
                  <div
                    v-for="row in visibleRows"
                    :key="row.id"
                    class="status-item metric-row"
                    :class="{
                      moving: dragging === row.id,
                      'pointer-moving': pointerDragging && dragging === row.id,
                    }"
                    :data-metric="row.id"
                  >
                    <div class="metric-heading">
                      <button
                        class="drag-handle"
                        :disabled="activeColor !== null"
                        :aria-label="t('systemStatus.reorder', { name: t(displayLabelKeys[row.id]) })"
                        :aria-pressed="dragging === row.id"
                        @pointerdown="pointerDown($event, row.id)"
                        @dragstart.prevent
                        @keydown="key($event, row.id)"
                      >
                        <MdIcon :name="ICON_NAMES.grip" :size="15" />
                      </button>
                      <div class="metric-label-group">
                        <label :for="`status-${row.id}`">
                          <MdCheckbox
                            :id="`status-${row.id}`"
                            :model-value="row.enabled"
                            :disabled="
                              selectionLocked(row.enabled) ||
                              (row.id === 'gpuTemperature' && !row.enabled && gpuTemperatureStatus === 'unsupported') ||
                              (row.id === 'cpuTemperature' &&
                                !isMacOs &&
                                !isLinux &&
                                (temperatureSelectionPending || monitoring.loading || monitoring.busy))
                            "
                            @update:model-value="enable(row.id, $event === true)"
                          />
                          {{
                            row.id === 'cpu'
                              ? t('systemStatus.cpuShort')
                              : row.id === 'disk'
                                ? t('systemStatus.volume')
                                : t(displayLabelKeys[row.id])
                          }}
                        </label>
                        <MdTooltip
                          v-if="
                            row.id === 'chargingPower' ||
                            row.id === 'batteryPower' ||
                            (row.id === 'batteryTemperature' && batteryTemperatureStatus === 'unsupported')
                          "
                          :text="batteryHint(row.id)"
                        >
                          <button type="button" class="md-help-action" :aria-label="batteryHint(row.id)">
                            <MdIcon :name="ICON_NAMES.info" :size="14" />
                          </button>
                        </MdTooltip>
                        <MdTooltip
                          v-if="row.id === 'systemPower' && systemPowerStatus === 'unsupported'"
                          :text="t('systemStatus.systemPowerUnsupportedHint')"
                        >
                          <button
                            type="button"
                            class="md-help-action"
                            :aria-label="t('systemStatus.systemPowerUnsupportedHint')"
                          >
                            <MdIcon :name="ICON_NAMES.info" :size="14" />
                          </button>
                        </MdTooltip>
                        <span
                          v-if="
                            row.id === 'cpuTemperature' &&
                            !isMacOs &&
                            !isLinux &&
                            monitoring.statusLabel(settings.preferences?.windowsCpuTemperatureEnabled ?? false)
                          "
                          role="status"
                          class="inline-flex shrink-0 items-center rounded bg-muted px-1.5 py-0.5 text-content-meta font-medium leading-4"
                          :class="monitoring.needsAttention ? 'text-destructive-text' : 'text-muted-foreground'"
                          >{{
                            t(
                              monitoring.statusLabel(settings.preferences?.windowsCpuTemperatureEnabled ?? false) ??
                                'windowsMonitoring.states.notEnabled'
                            )
                          }}</span
                        >
                      </div>

                      <Button
                        v-if="row.id === 'cpuTemperature' && !isMacOs && !isLinux"
                        variant="ghost"
                        size="sm"
                        class="text-muted-foreground"
                        @click="
                          selectTemperatureAfterSetup = false;
                          temperatureSetupOpen = true;
                        "
                        >{{ t('windowsMonitoring.manage') }}</Button
                      >
                      <Select
                        v-if="
                          row.id === 'network' ||
                          row.id === 'disk' ||
                          row.id === 'diskTemperature' ||
                          row.id === 'gpu' ||
                          row.id === 'gpuTemperature'
                        "
                        :disabled="row.id === 'gpuTemperature' ? !settings.draft || settings.saving : !row.enabled"
                        :open="expandedSelection === row.id"
                        :model-value="
                          row.id === 'network'
                            ? (settings.draft?.networkInterface ?? 'automatic')
                            : row.id === 'gpu' || row.id === 'gpuTemperature'
                              ? (settings.draft?.gpuAdapter ?? 'automatic')
                              : (settings.draft?.diskVolume ?? 'system')
                        "
                        @update:open="setSelectionOpen(row.id, $event)"
                        @update:model-value="
                          row.id === 'network'
                            ? settings.change({ networkInterface: $event === 'automatic' ? null : String($event) })
                            : row.id === 'gpu' || row.id === 'gpuTemperature'
                              ? settings.change({ gpuAdapter: $event === 'automatic' ? null : String($event) })
                              : settings.change({ diskVolume: $event === 'system' ? null : String($event) })
                        "
                      >
                        <SelectTrigger as-child>
                          <MdTooltip
                            :text="
                              row.id === 'network'
                                ? selectedInterfaceLabel
                                : row.id === 'gpu' || row.id === 'gpuTemperature'
                                  ? selectedGpuLabel
                                  : selectedVolumeLabel
                            "
                            ><button
                              type="button"
                              class="selection-toggle"
                              :aria-label="
                                row.id === 'network'
                                  ? `${t('systemStatus.interface')}: ${selectedInterfaceLabel}`
                                  : row.id === 'gpu' || row.id === 'gpuTemperature'
                                    ? `${t('systemStatus.gpu')}: ${selectedGpuLabel}`
                                    : `${t('systemStatus.volume')}: ${selectedVolumeLabel}`
                              "
                            >
                              <span class="truncate">{{
                                row.id === 'network'
                                  ? selectedInterfaceLabel
                                  : row.id === 'gpu' || row.id === 'gpuTemperature'
                                    ? selectedGpuLabel
                                    : selectedVolumeLabel
                              }}</span>
                              <MdIcon
                                :name="expandedSelection === row.id ? ICON_NAMES.chevronUp : ICON_NAMES.chevronDown"
                                :size="14"
                                class="shrink-0"
                              /></button
                          ></MdTooltip>
                        </SelectTrigger>
                        <SelectContent align="end" class="z-60 max-w-[min(20rem,calc(100vw-2rem))]">
                          <template v-if="row.id === 'network'">
                            <SelectItem value="automatic">{{ t('systemStatus.automatic') }}</SelectItem>
                            <SelectItem v-for="item in interfaces" :key="item.id" :value="item.id" class="break-all">
                              {{ item.name }}{{ item.connected ? '' : ` · ${t('systemStatus.disconnected')}` }}
                            </SelectItem>
                            <SelectItem v-if="missingInterface" :value="settings.draft!.networkInterface!">{{
                              t('systemStatus.savedDisconnected')
                            }}</SelectItem>
                          </template>
                          <template v-else-if="row.id === 'gpu' || row.id === 'gpuTemperature'">
                            <SelectItem value="automatic">{{ t('systemStatus.automatic') }}</SelectItem>
                            <SelectItem v-for="item in gpuAdapters" :key="item.id" :value="item.id" class="break-all">{{
                              item.name
                            }}</SelectItem>
                            <SelectItem v-if="missingGpu" :value="settings.draft!.gpuAdapter!">{{
                              t('systemStatus.savedDisconnected')
                            }}</SelectItem>
                          </template>
                          <template v-else>
                            <SelectItem value="system">{{ t('systemStatus.systemDisk') }}</SelectItem>
                            <SelectItem v-for="item in volumes" :key="item.id" :value="item.id" class="break-all">{{
                              item.name
                            }}</SelectItem>
                            <SelectItem v-if="missingVolume" :value="settings.draft!.diskVolume!">{{
                              t('systemStatus.savedDisconnected')
                            }}</SelectItem>
                          </template>
                        </SelectContent>
                      </Select>
                      <Button
                        v-if="row.id !== 'network'"
                        :id="`color-configure-${row.id}`"
                        variant="ghost"
                        size="sm"
                        class="color-toggle shrink-0 text-muted-foreground"
                        aria-haspopup="dialog"
                        :aria-controls="`color-editor-${row.id}`"
                        :aria-label="t('systemStatus.colorConfigure', { name: t(displayLabelKeys[row.id]) })"
                        :disabled="settings.saving"
                        @click="openColor(row.id)"
                      >
                        {{ t('systemStatus.colorAction') }}
                      </Button>
                    </div>
                  </div>
                </TransitionGroup>
              </div>
              <span class="sr-only" aria-live="polite">{{ announcement }}</span>
            </div>
          </div>
          <p v-if="settings.error || (displayEnabled && catalogueError)" class="settings-feedback" role="status">
            <template v-if="settings.error"
              >{{ t('systemStatus.saveFailed') }}
              <button @click="settings.load()">{{ t('monitoring.refresh') }}</button></template
            ><template v-else-if="catalogueError"
              >{{ t('systemStatus.catalogueFailed') }}
              <button @click="loadCatalogue">{{ t('monitoring.refresh') }}</button></template
            >
          </p>
        </div>
        <MdDialogFooter align="between" class="flex-row flex-wrap">
          <DialogDescription class="min-w-0 flex-1 text-content-secondary">{{
            t('systemStatus.autoSaveHint')
          }}</DialogDescription>
          <Button :disabled="settings.saving" @click="settingsOpen = false">{{ t('systemStatus.done') }}</Button>
        </MdDialogFooter>
      </MdDialogContent>
    </Dialog>
    <MdStatusColorRuleEditor
      v-if="activeColor && settings.draft"
      :key="activeColor"
      :name="t(displayLabelKeys[activeColor])"
      :rule="settings.draft.colorRules.find(rule => rule.id === activeColor)!"
      :saving="settings.saving"
      :error="settings.error ? t('systemStatus.saveFailed') : undefined"
      @save="saveColor"
      @cancel="activeColor = null"
    />
    <MdWindowsTemperatureSetup
      v-if="!isMacOs && !isLinux"
      :open="temperatureSetupOpen"
      :select-display-item="selectTemperatureAfterSetup"
      @update:open="temperatureSetupOpen = $event"
    />
  </div>
</template>

<style scoped>
@reference "@assets/main.css";
.display-feedback {
  padding: 10px 14px;
}
.display-options {
  display: grid;
  gap: 14px;
  min-width: 0;
}
.controls-heading {
  @apply text-muted-foreground;
  display: flex;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-bottom: 8px;
  font-size: var(--font-content-secondary);
  font-weight: 400;
}
.controls-heading h3 {
  margin: 0;
  font: inherit;
  font-weight: 500;
}
.metric-rows {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
}
.status-item {
  @apply border-b border-border/50 bg-transparent;
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  min-height: 42px;
  padding: 4px 0;
}
.status-item:last-child {
  border-bottom-color: transparent;
}
.metric-row.moving {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
.metric-sort-move {
  transition: transform 200ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.metric-row.pointer-moving {
  outline: 1px dashed var(--ring);
  outline-offset: -1px;
  /* Keep the destination visible immediately; only neighbouring cards make way. */
  transition: none;
}
.pointer-moving .metric-heading {
  visibility: hidden;
}
.status-item.drag-preview {
  @apply rounded-md bg-card border border-primary/40 shadow-lg;
  position: fixed;
  z-index: 100;
  margin: 0;
  pointer-events: none;
  will-change: transform;
  cursor: grabbing;
}
@media (prefers-reduced-motion: reduce) {
  .metric-sort-move {
    transition: none;
  }
}
.metric-heading {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  gap: 8px;
}
.metric-label-group {
  display: flex;
  align-items: center;
  flex: 1;
  flex-wrap: wrap;
  min-width: 0;
  gap: 4px 8px;
}
.metric-heading label,
.logo-label {
  @apply text-foreground;
  display: flex;
  align-items: center;
  flex: 1;
  gap: 10px;
  min-width: 0;
  min-height: 32px;
  font-size: var(--font-content-body);
  font-weight: 400;
  cursor: pointer;
  overflow-wrap: anywhere;
}
.status-item .metric-label-group label {
  flex: 0 1 auto;
}
.logo-marker {
  display: grid;
  place-items: center;
  flex: none;
  width: 24px;
}
.drag-handle,
.selection-toggle {
  @apply text-foreground;
  @apply text-muted-foreground rounded;
  display: grid;
  place-items: center;
  flex: none;
  height: 32px;
  cursor: pointer;
}
.drag-handle {
  width: 24px;
  touch-action: none;
  user-select: none;
  cursor: grab;
}
.selection-toggle {
  display: flex;
  gap: 6px;
  min-width: 0;
  max-width: 55%;
  padding-inline: 8px;
  font-size: var(--font-content-body);
  font-weight: 400;
}
.drag-handle:active {
  cursor: grabbing;
}
.drag-handle:focus-visible,
.selection-toggle:focus-visible {
  outline: 2px solid var(--ring);
}
.drag-handle:hover,
.selection-toggle:hover {
  @apply bg-accent text-foreground;
}
.selection-toggle:disabled {
  opacity: 0.4;
  cursor: default;
}
@media (pointer: coarse) {
  .status-item label,
  .drag-handle,
  .selection-toggle {
    min-height: 44px;
  }
  .drag-handle,
  .logo-marker {
    width: 44px;
  }
}
.settings-feedback {
  @apply text-destructive-text;
  padding-top: 8px;
  font-size: 11px;
  line-height: 1.5;
}
.settings-feedback button {
  text-decoration: underline;
  cursor: pointer;
}
</style>
