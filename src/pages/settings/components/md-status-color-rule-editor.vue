<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSettingsRow from '@/components/custom/md-settings-row.vue';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdNumberField from '@/components/custom/md-number-field.vue';
import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import { Dialog, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import {
  DISPLAY_COLORS,
  MAX_COLOR_RULES,
  defaultColorRule,
  colorMetricUnit,
  colorMetricLimit,
} from '@/lib/models/resident-color-rules';
import type { ColorRule, DisplayColor, MetricColorRule } from '@/lib/models/resident-color-rules';

const props = defineProps<{
  rule: MetricColorRule;
  name: string;
  saving: boolean;
  error?: string;
}>();
const emit = defineEmits<{ save: [rule: MetricColorRule]; cancel: [] }>();
const { t } = useI18n({ useScope: 'global' });
function restoreFocus(event: Event) {
  event.preventDefault();
  document.getElementById(`color-configure-${props.rule.id}`)?.focus();
}
interface DraftRule {
  key: number;
  condition: ColorRule['condition'];
  from: number | null;
  to: number | null;
  color: DisplayColor;
}
let nextKey = 0;
function editable(rule: ColorRule): DraftRule {
  return {
    key: nextKey++,
    condition: rule.condition,
    from: rule.condition === 'range' ? rule.from : rule.value,
    to: rule.condition === 'range' ? rule.to : null,
    color: rule.color,
  };
}
const enabled = ref(props.rule.enabled);
const rules = ref(props.rule.rules.map(editable));
const unit = computed(() => colorMetricUnit(props.rule.id));
const limit = computed(() => colorMetricLimit(props.rule.id));
function whole(value: number | null): value is number {
  return value !== null && Number.isInteger(value) && value >= 0 && value <= limit.value;
}
function bounds(rule: DraftRule): [number, number] | null {
  if (!whole(rule.from)) return null;
  if (rule.condition === 'below') return rule.from > 0 ? [0, rule.from] : null;
  if (rule.condition === 'atLeast') return [rule.from, Infinity];
  return whole(rule.to) && rule.to > rule.from ? [rule.from, rule.to] : null;
}
const validation = computed(() => {
  const intervals = rules.value.map(bounds);
  if (intervals.some(interval => interval === null))
    return t('systemStatus.colorInvalid', { max: limit.value, unit: unit.value });
  for (let first = 0; first < intervals.length; first++) {
    for (let second = first + 1; second < intervals.length; second++) {
      const a = intervals[first]!;
      const b = intervals[second]!;
      if (a[0] < b[1] && b[0] < a[1]) return t('systemStatus.colorOverlap', { first: first + 1, second: second + 1 });
    }
  }
  return null;
});
function save() {
  if (validation.value || props.saving) return;
  emit('save', {
    id: props.rule.id,
    enabled: enabled.value,
    rules: rules.value.map(rule =>
      rule.condition === 'range'
        ? { condition: 'range', from: rule.from!, to: rule.to!, color: rule.color }
        : { condition: rule.condition, value: rule.from!, color: rule.color }
    ),
  });
}
function addRule() {
  if (props.saving || rules.value.length >= MAX_COLOR_RULES) return;
  const key = nextKey++;
  rules.value.push({ key, condition: 'atLeast', from: null, to: null, color: 'red' });
  void nextTick(() => document.getElementById(`color-from-${key}`)?.focus());
}
function restore() {
  const defaults = defaultColorRule(props.rule.id);
  enabled.value = defaults.enabled;
  rules.value = defaults.rules.map(editable);
}
const colorLabelKeys: Record<DisplayColor, string> = {
  default: 'systemStatus.palette.default',
  green: 'systemStatus.palette.green',
  yellow: 'systemStatus.palette.yellow',
  orange: 'systemStatus.palette.orange',
  red: 'systemStatus.palette.red',
  blue: 'systemStatus.palette.blue',
};
const conditions = {
  below: 'systemStatus.colorBelow',
  range: 'systemStatus.colorRange',
  atLeast: 'systemStatus.colorAtLeast',
};
</script>

<template>
  <Dialog :open="true" @update:open="!$event && !saving && emit('cancel')">
    <MdDialogContent
      :id="`color-editor-${rule.id}`"
      size="wide"
      height="tall"
      class="color-rule-editor flex flex-col"
      @close-auto-focus="restoreFocus"
      @interact-outside.prevent
      @escape-key-down="saving && $event.preventDefault()"
    >
      <MdDialogHeader>
        <DialogTitle>{{ t('systemStatus.colorDialogTitle', { name }) }}</DialogTitle>
      </MdDialogHeader>
      <div class="min-h-0 min-w-0 flex-1 overflow-x-hidden scrollbar-stable-end px-4 py-3">
        <MdSettingsRow
          compact
          :title="t('systemStatus.colorEnabled')"
          description=""
          :label-for="`color-enabled-${rule.id}`"
        >
          <MdSwitch :id="`color-enabled-${rule.id}`" v-model="enabled" :disabled="saving" />
        </MdSettingsRow>
        <DialogDescription class="mb-3 text-content-secondary text-muted-foreground"
          >{{ t('systemStatus.colorFallback') }} {{ t('systemStatus.colorRangeHint') }}</DialogDescription
        >
        <p v-if="!rules.length" class="rounded-md bg-muted/40 p-3 text-content-body text-muted-foreground">
          {{ t('systemStatus.colorEmpty') }}
        </p>
        <div class="grid gap-2">
          <div
            v-for="(item, index) in rules"
            :key="item.key"
            class="rule-row rounded-md border border-border/50 p-2"
            :data-rule="index"
          >
            <div class="mb-1 flex items-center justify-between gap-2">
              <span class="text-content-secondary text-muted-foreground">{{
                t('systemStatus.colorRule', { index: index + 1 })
              }}</span>
              <Button
                variant="ghost"
                size="sm"
                class="h-7 px-2"
                :disabled="saving"
                :aria-label="t('systemStatus.colorRemoveRule', { index: index + 1 })"
                @click="rules.splice(index, 1)"
                >{{ t('systemStatus.colorRemove') }}</Button
              >
            </div>
            <div class="flex flex-wrap items-center gap-2">
              <Select v-model="item.condition" :disabled="saving">
                <SelectTrigger size="sm" class="w-32" :aria-label="t('systemStatus.colorCondition')"
                  ><SelectValue
                /></SelectTrigger>
                <SelectContent
                  ><SelectItem v-for="(key, condition) in conditions" :key="condition" :value="condition">{{
                    t(key)
                  }}</SelectItem></SelectContent
                >
              </Select>
              <label :for="`color-from-${item.key}`" class="sr-only">{{
                t('systemStatus.colorValue', { index: index + 1 })
              }}</label>
              <MdNumberField
                :id="`color-from-${item.key}`"
                v-model="item.from"
                class="color-number-field w-26"
                :label="t('systemStatus.colorValue', { index: index + 1 })"
                :min="0"
                :max="limit"
                :step="1"
                :step-snapping="false"
                :disabled="saving"
              />
              <template v-if="item.condition === 'range'">
                <span class="text-content-body">{{ t('systemStatus.colorTo') }}</span>
                <label :for="`color-to-${item.key}`" class="sr-only">{{
                  t('systemStatus.colorUpper', { index: index + 1 })
                }}</label>
                <MdNumberField
                  :id="`color-to-${item.key}`"
                  v-model="item.to"
                  class="color-number-field w-26"
                  :label="t('systemStatus.colorUpper', { index: index + 1 })"
                  :min="0"
                  :max="limit"
                  :step="1"
                  :step-snapping="false"
                  :disabled="saving"
                />
              </template>
              <span class="text-content-body text-muted-foreground">{{ unit }}</span>
              <Select v-model="item.color" :disabled="saving">
                <SelectTrigger
                  size="sm"
                  class="ml-auto w-40"
                  :aria-label="t('systemStatus.colorRuleColor', { index: index + 1 })"
                  ><SelectValue class="min-w-0 truncate"
                    ><span :data-color="item.color">{{ t(colorLabelKeys[item.color]) }}</span></SelectValue
                  ></SelectTrigger
                >
                <SelectContent>
                  <SelectItem v-for="color in DISPLAY_COLORS" :key="color" :value="color"
                    ><span class="inline-flex items-center gap-2" :data-color="color"
                      ><span class="color-swatch" aria-hidden="true" />{{ t(colorLabelKeys[color]) }}</span
                    ></SelectItem
                  >
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
        <p v-if="validation" role="alert" class="mt-3 text-content-secondary text-destructive-text">{{ validation }}</p>
        <Button
          variant="outline"
          size="sm"
          class="mt-3"
          :disabled="saving || rules.length >= MAX_COLOR_RULES"
          @click="addRule"
          >{{ t('systemStatus.colorAddRule') }}</Button
        >
        <p v-if="rules.length >= MAX_COLOR_RULES" class="mt-2 text-content-secondary text-muted-foreground">
          {{ t('systemStatus.colorLimit', { max: MAX_COLOR_RULES }) }}
        </p>
        <p v-if="error" role="alert" class="mt-3 text-content-secondary text-destructive-text">{{ error }}</p>
      </div>
      <MdDialogFooter class="flex items-center gap-2">
        <Button variant="ghost" size="sm" class="mr-auto" :disabled="saving" @click="restore">
          <span class="text-content-body">{{ t('systemStatus.colorRestore') }}</span>
        </Button>
        <Button variant="outline" size="sm" :disabled="saving" @click="emit('cancel')">
          <span class="text-content-body">{{ t('common.cancel') }}</span>
        </Button>
        <Button size="sm" :disabled="saving || !!validation" @click="save">
          <span class="text-content-body">{{ t('common.save') }}</span>
        </Button>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
:global(.color-rule-editor) {
  --layout-dialog-wide-width: 640px;
  --layout-dialog-tall-height: 520px;
  --layout-dialog-header-height: 56px;
  --layout-dialog-header-padding: 12px 48px 12px 16px;
  --layout-dialog-footer-height: 48px;
  --layout-dialog-footer-padding: 8px 16px;
}
:global(.color-rule-editor [data-slot='dialog-title']) {
  font-size: var(--font-content-primary);
}
:global(.color-rule-editor .color-number-field [data-slot='input']) {
  height: 32px;
}
:global(.color-rule-editor .color-number-field [data-slot='increment']),
:global(.color-rule-editor .color-number-field [data-slot='decrement']) {
  top: 2px;
}
.color-swatch {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
}
[data-color='green'] {
  color: var(--usage-green);
}
[data-color='yellow'] {
  color: var(--usage-yellow);
}
[data-color='orange'] {
  color: var(--usage-warning);
}
[data-color='red'] {
  color: var(--usage-critical);
}
[data-color='blue'] {
  color: var(--usage-blue);
}
</style>
