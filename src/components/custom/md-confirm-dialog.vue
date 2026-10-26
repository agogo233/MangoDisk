<script setup lang="ts">
import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdSpinner from '@/components/custom/md-spinner.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';

const props = withDefaults(
  defineProps<{
    open: boolean;
    title: string;
    description: string;
    descriptionClass?: string;
    cancelLabel: string;
    confirmLabel: string;
    confirmVariant?: 'default' | 'destructive';
    size?: 'compact' | 'standard';
    presentation?: 'standard' | 'panel';
    busy?: boolean;
  }>(),
  {
    descriptionClass: undefined,
    confirmVariant: 'default',
    size: 'compact',
    presentation: 'standard',
    busy: false,
  }
);

const emit = defineEmits<{
  'update:open': [value: boolean];
  confirm: [];
}>();

function updateOpen(open: boolean) {
  // An accepted operation may continue after confirmation. Keep the modal
  // visible while busy so its progress cannot disappear through Escape or an
  // outside click before the owning workflow has settled.
  if (!open && props.busy) return;
  emit('update:open', open);
}
</script>

<template>
  <Dialog :open="open" @update:open="updateOpen">
    <MdDialogContent
      :size="size"
      :show-close="presentation !== 'panel' && !busy"
      :class="presentation === 'panel' ? 'md-confirm-dialog--panel max-w-80' : undefined"
    >
      <MdDialogHeader>
        <DialogTitle :class="presentation === 'panel' ? 'text-sm leading-snug' : undefined">{{ title }}</DialogTitle>
        <DialogDescription
          :class="[descriptionClass, presentation === 'panel' ? 'text-xs leading-relaxed' : undefined]"
          >{{ description }}</DialogDescription
        >
      </MdDialogHeader>
      <div v-if="$slots.default" class="md-confirm-dialog-body">
        <slot />
      </div>
      <MdDialogFooter :class="presentation === 'panel' ? 'flex-row justify-end' : undefined">
        <Button
          :size="presentation === 'panel' ? 'sm' : 'default'"
          variant="outline"
          type="button"
          :disabled="busy"
          @click="updateOpen(false)"
        >
          {{ cancelLabel }}
        </Button>
        <Button
          :size="presentation === 'panel' ? 'sm' : 'default'"
          :variant="confirmVariant"
          type="button"
          :disabled="busy"
          @click="emit('confirm')"
        >
          <MdSpinner v-if="busy" size="small" />
          {{ confirmLabel }}
        </Button>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
.md-confirm-dialog-body {
  min-width: 0;
  padding: 0 var(--layout-dialog-body-inline-padding) 12px;
}
</style>

<style>
/* The panel's 390px viewport needs a small confirmation rather than the
   application's stacked mobile footer. Other confirmation dialogs keep their layout. */
.md-confirm-dialog--panel {
  --layout-dialog-header-height: 0px;
  --layout-dialog-header-padding: 14px 14px 8px;
  --layout-dialog-body-inline-padding: 14px;
  --layout-dialog-footer-height: 44px;
  --layout-dialog-footer-padding: 4px 14px 12px;
}
</style>
