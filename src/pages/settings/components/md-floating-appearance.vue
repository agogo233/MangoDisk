<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import MdSettingsRow from '@/components/custom/md-settings-row.vue';
import MdSwitch from '@/components/custom/md-switch.vue';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import type { ResidentPreferences } from '@/lib/models/resident/resident';
import { FLOATING_FONT_SIZES, FLOATING_BACKGROUNDS } from '@/lib/models/resident/floating-appearance';
import '@/assets/themes/floating.css';

defineProps<{ preferences: ResidentPreferences }>();
const emit = defineEmits<{ change: [patch: Partial<ResidentPreferences>] }>();
const { t } = useI18n();
</script>
<template>
  <MdSettingsRow compact :title="t('floatingMonitor.layout')" description="" label-for="floating-layout">
    <Select
      :model-value="preferences.floating.layout"
      @update:model-value="
        emit('change', { floating: { ...preferences.floating, layout: $event as 'horizontal' | 'vertical' } })
      "
    >
      <SelectTrigger id="floating-layout" class="w-auto min-w-32"
        ><SelectValue>{{ t(`floatingMonitor.${preferences.floating.layout}`) }}</SelectValue></SelectTrigger
      >
      <SelectContent
        ><SelectItem value="horizontal">{{ t('floatingMonitor.horizontal') }}</SelectItem
        ><SelectItem value="vertical">{{ t('floatingMonitor.vertical') }}</SelectItem></SelectContent
      >
    </Select>
  </MdSettingsRow>

  <MdSettingsRow compact :title="t('floatingMonitor.fontSize')" description="" label-for="floating-font-size">
    <Select
      :model-value="preferences.floating.fontSize"
      @update:model-value="
        emit('change', {
          floating: { ...preferences.floating, fontSize: $event as ResidentPreferences['floating']['fontSize'] },
        })
      "
    >
      <SelectTrigger id="floating-font-size" class="w-auto min-w-32"
        ><SelectValue>{{ t(`floatingMonitor.fontSizes.${preferences.floating.fontSize}`) }}</SelectValue></SelectTrigger
      >
      <SelectContent>
        <SelectItem v-for="size in FLOATING_FONT_SIZES" :key="size" :value="size">{{
          t(`floatingMonitor.fontSizes.${size}`)
        }}</SelectItem>
      </SelectContent>
    </Select>
  </MdSettingsRow>
  <MdSettingsRow compact :title="t('floatingMonitor.bold')" description="" label-for="floating-bold">
    <MdSwitch
      id="floating-bold"
      :model-value="preferences.floating.bold"
      @update:model-value="emit('change', { floating: { ...preferences.floating, bold: $event } })"
    />
  </MdSettingsRow>
  <MdSettingsRow compact :title="t('floatingMonitor.dividers')" description="" label-for="floating-dividers">
    <MdSwitch
      id="floating-dividers"
      :model-value="preferences.floating.dividers"
      @update:model-value="emit('change', { floating: { ...preferences.floating, dividers: $event } })"
    />
  </MdSettingsRow>
  <MdSettingsRow compact :title="t('floatingMonitor.background')" description="" label-for="floating-background">
    <Select
      :model-value="preferences.floating.background"
      @update:model-value="
        emit('change', {
          floating: { ...preferences.floating, background: $event as ResidentPreferences['floating']['background'] },
        })
      "
    >
      <SelectTrigger id="floating-background" class="w-auto min-w-32">
        <span
          class="floating-background-swatch"
          :data-background="preferences.floating.background"
          aria-hidden="true"
        />
        <SelectValue>{{ t(`floatingMonitor.backgrounds.${preferences.floating.background}`) }}</SelectValue>
      </SelectTrigger>
      <SelectContent>
        <SelectItem v-for="background in FLOATING_BACKGROUNDS" :key="background" :value="background">
          <span class="inline-flex items-center gap-2"
            ><span class="floating-background-swatch" :data-background="background" aria-hidden="true" />
            {{ t(`floatingMonitor.backgrounds.${background}`) }}</span
          >
        </SelectItem>
      </SelectContent>
    </Select>
  </MdSettingsRow>
  <MdSettingsRow compact :title="t('floatingMonitor.backgroundOpacity')" description="" label-for="floating-opacity">
    <Select
      :model-value="String(preferences.floating.backgroundOpacity)"
      @update:model-value="emit('change', { floating: { ...preferences.floating, backgroundOpacity: Number($event) } })"
    >
      <SelectTrigger id="floating-opacity" class="w-auto min-w-32"
        ><SelectValue>{{ preferences.floating.backgroundOpacity }}%</SelectValue></SelectTrigger
      >
      <SelectContent>
        <SelectItem v-for="opacity in [100, 90, 80, 70, 60, 50]" :key="opacity" :value="String(opacity)"
          >{{ opacity }}%</SelectItem
        >
      </SelectContent>
    </Select>
  </MdSettingsRow>
</template>

<style scoped>
.floating-background-swatch {
  display: inline-block;
  width: 14px;
  height: 14px;
  flex: none;
  border-radius: 3px;
  border: 1px solid var(--floating-border, var(--border));
  background: var(--floating-surface, var(--background));
}
</style>
