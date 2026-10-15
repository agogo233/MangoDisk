<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSettingsGroup from '@/components/custom/md-settings-group.vue';
import MdSettingsRow from '@/components/custom/md-settings-row.vue';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdTooltip from '@/components/custom/md-tooltip.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import type { ResidentPreferences } from '@/lib/models/resident';

const props = withDefaults(defineProps<{ preferences: ResidentPreferences; isMacOs: boolean; isLinux?: boolean }>(), {
  isLinux: false,
});
const emit = defineEmits<{ change: [patch: Partial<ResidentPreferences>] }>();
const { t } = useI18n({ useScope: 'global' });
const compactControlId = computed(() => {
  if (props.isMacOs) return 'menu-bar-compact';
  if (props.isLinux) return 'linux-tray-compact';
  return 'taskbar-compact';
});
const compactEnabled = computed(() =>
  props.isMacOs ? props.preferences.menuBarCompact : props.preferences.taskbarCompact
);
function changeCompact(enabled: boolean) {
  emit('change', props.isMacOs ? { menuBarCompact: enabled } : { taskbarCompact: enabled });
}
</script>

<template>
  <MdSettingsGroup plain :title="t('systemStatus.appearanceTitle')">
    <MdSettingsRow
      v-if="!isMacOs && !isLinux"
      compact
      :title="t('systemStatus.taskbarBackground')"
      description=""
      label-for="taskbar-background"
    >
      <MdSwitch
        id="taskbar-background"
        :model-value="preferences.taskbarBackground"
        @update:model-value="emit('change', { taskbarBackground: $event })"
      />
    </MdSettingsRow>
    <MdSettingsRow compact :title="t('systemStatus.taskbarCompact')" description="" :label-for="compactControlId">
      <template #help>
        <MdTooltip :text="t('systemStatus.menuBarCompactHint')">
          <button type="button" class="md-help-action" :aria-label="t('systemStatus.menuBarCompactHint')">
            <MdIcon :name="ICON_NAMES.help" :size="14" />
          </button>
        </MdTooltip>
      </template>
      <MdSwitch :id="compactControlId" :model-value="compactEnabled" @update:model-value="changeCompact" />
    </MdSettingsRow>
    <div>
      <MdSettingsRow compact :title="t('systemStatus.usageColors')" description="" label-for="usage-colors">
        <template #help>
          <MdTooltip :text="t('systemStatus.usageColorsHelp')">
            <button type="button" class="md-help-action" :aria-label="t('systemStatus.usageColorsHelp')">
              <MdIcon :name="ICON_NAMES.help" :size="14" />
            </button>
          </MdTooltip>
        </template>
        <MdSwitch
          id="usage-colors"
          :model-value="preferences.usageColors"
          @update:model-value="emit('change', { usageColors: $event })"
        />
      </MdSettingsRow>
    </div>
  </MdSettingsGroup>
</template>
