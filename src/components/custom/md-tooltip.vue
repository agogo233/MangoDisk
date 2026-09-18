<script setup lang="ts">
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { TOOLTIP_OPEN_DELAY_MS } from '@/lib/models/ui';

defineOptions({ inheritAttrs: false });
defineProps<{ text?: string | null }>();
</script>

<template>
  <!-- Standalone resident windows do not inherit the main window's provider. -->
  <TooltipProvider
    :delay-duration="TOOLTIP_OPEN_DELAY_MS"
    :disable-hoverable-content="true"
    :ignore-non-keyboard-focus="true"
  >
    <Tooltip :disabled="!text">
      <!-- Preserve the caller's element, layout, accessible name and handlers. -->
      <TooltipTrigger as-child v-bind="$attrs"><slot /></TooltipTrigger>
      <!-- Portal content does not inherit this wrapper's scoped CSS attributes. -->
      <!-- Natural wrapping avoids balanced lines leaving unused space in descriptions and file paths. -->
      <TooltipContent
        class="max-w-[min(24rem,calc(100vw-24px))] text-left whitespace-normal text-wrap [overflow-wrap:anywhere]"
      >
        {{ text }}
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>
