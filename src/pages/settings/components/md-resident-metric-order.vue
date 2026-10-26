<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import type { ResidentPreferences, ResidentDisplayMetricId } from '@/lib/models/resident/resident';
const props = defineProps<{
  metrics: ResidentPreferences['metrics'] | undefined;
  visibleRows: ResidentPreferences['metrics'];
  labelKeys: Record<ResidentDisplayMetricId, string>;
}>();
const emit = defineEmits<{
  preview: [ResidentPreferences['metrics'] | null];
  commit: [ResidentPreferences['metrics']];
  pointerStart: [];
}>();
const { t } = useI18n({ useScope: 'global' });
const dragging = ref<ResidentDisplayMetricId | null>(null);
const pointerDragging = ref(false);
const dragRows = ref<ResidentPreferences['metrics'] | null>(null);
const announcement = ref('');
const metricRowsElement = ref<HTMLElement | null>(null);
const visibleRows = computed(() => {
  // Local preview order must be available before the parent renders new props,
  // including to keyboard announcements emitted in the same event handler.
  const visible = new Map(props.visibleRows.map(row => [row.id, row]));
  return (dragRows.value ?? props.visibleRows).flatMap(row => {
    const presented = visible.get(row.id);
    return presented ? [presented] : [];
  });
});
watch(dragRows, value => emit('preview', value), { flush: 'sync' });
function begin(id: ResidentDisplayMetricId) {
  if (!props.metrics) return;
  dragging.value = id;
  dragRows.value = props.metrics.map(metric => ({ ...metric }));
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
    name: t(props.labelKeys[dragging.value!]),
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
  emit('pointerStart');
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
  if (metrics && metrics.some((row, index) => row.id !== props.metrics?.[index]?.id)) {
    emit('commit', metrics);
  }
}
function handleKey(event: KeyboardEvent, id: ResidentDisplayMetricId) {
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

defineExpose({ cancelDrag, dragging });
onBeforeUnmount(cancelDrag);
</script>
<template>
  <div ref="metricRowsElement" @keydown.esc="cancelDrag">
    <!-- Hidden panels have no layout box. Animate only explicit reordering so
         revealing a tab cannot turn its zero-size bounds into a move animation. -->
    <TransitionGroup name="metric-sort" tag="div" class="metric-rows" :data-reordering="dragging !== null">
      <slot name="logo" />
      <slot
        v-for="row in visibleRows"
        :key="row.id"
        :row="row"
        :dragging="dragging"
        :pointer-dragging="pointerDragging"
        :pointer-down="pointerDown"
        :handle-key="handleKey"
      />
    </TransitionGroup>
  </div>
  <span class="sr-only" aria-live="polite">{{ announcement }}</span>
</template>
<style scoped>
.metric-rows {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
}
.metric-rows[data-reordering='true'] > :deep(.metric-sort-move:not(.pointer-moving)) {
  transition: transform 200ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
@media (prefers-reduced-motion: reduce) {
  .metric-rows[data-reordering='true'] > :deep(.metric-sort-move:not(.pointer-moving)) {
    transition: none;
  }
}
</style>
