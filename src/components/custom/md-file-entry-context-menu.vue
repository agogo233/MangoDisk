<script setup lang="ts">
import { onDeactivated, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdIcon from '@/components/icons/md-icon.vue';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu';
import { ICON_NAMES } from '@/lib/models/ui';

const { t } = useI18n({ useScope: 'global' });

const props = withDefaults(
  defineProps<{
    entryKey?: string;
    openDisabled?: boolean;
    deleteDisabled?: boolean;
    revealDisabled?: boolean;
  }>(),
  {
    entryKey: undefined,
    openDisabled: false,
    deleteDisabled: false,
    revealDisabled: false,
  }
);

const emit = defineEmits<{
  open: [];
  reveal: [];
  delete: [];
  menuStateChange: [open: boolean];
}>();
// Recycled virtual rows must not leave a menu targeting the previous file.
const open = ref(false);
onDeactivated(() => {
  open.value = false;
});
watch(
  () => props.entryKey,
  () => {
    open.value = false;
  },
  { flush: 'sync' }
);
</script>

<template>
  <!-- The owning domain maps these presentation-only actions to its trusted entry model. -->
  <ContextMenu v-model:open="open" @update:open="emit('menuStateChange', $event)">
    <ContextMenuTrigger as-child>
      <slot />
    </ContextMenuTrigger>
    <ContextMenuContent v-if="open">
      <ContextMenuItem :disabled="openDisabled" @select="emit('open')">
        <MdIcon :name="ICON_NAMES.external" :size="16" />
        {{ t('common.open') }}
      </ContextMenuItem>
      <ContextMenuItem :disabled="revealDisabled" @select="emit('reveal')">
        <MdIcon :name="ICON_NAMES.folder" :size="16" />
        {{ t('common.showInFileManager') }}
      </ContextMenuItem>
      <ContextMenuItem
        class="text-destructive focus:text-destructive"
        :disabled="deleteDisabled"
        @select="emit('delete')"
      >
        <MdIcon :name="ICON_NAMES.trash" :size="16" />
        {{ t('common.deletePermanently') }}
      </ContextMenuItem>
    </ContextMenuContent>
  </ContextMenu>
</template>
