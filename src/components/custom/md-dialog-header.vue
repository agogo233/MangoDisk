<script setup lang="ts">
import type { HTMLAttributes } from 'vue';

import DialogHeader from '@/components/ui/dialog/DialogHeader.vue';

defineProps<{ class?: HTMLAttributes['class'] }>();
</script>

<template>
  <DialogHeader data-tauri-drag-region :class="['relative', $props.class]">
    <slot />
    <!--
      Tauri only starts native window dragging when the pressed element is marked as a drag
      region. This overlay makes the complete visual header reliable, including title and
      description text, while the dialog close button remains above it in MdDialogContent.
    -->
    <div data-tauri-drag-region class="absolute inset-0 z-10" aria-hidden="true" />
  </DialogHeader>
</template>
