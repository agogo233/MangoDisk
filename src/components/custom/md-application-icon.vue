<script setup lang="ts">
import { computed } from 'vue';

import MdIcon from '@/components/icons/md-icon.vue';
import MdIconWindowsExecutable from '@/components/icons/md-icon-windows-executable.vue';
import type { ApplicationUninstallPlatform } from '@/lib/models/application';
import { ICON_NAMES } from '@/lib/models/ui';

const props = withDefaults(
  defineProps<{
    src?: string;
    platform?: ApplicationUninstallPlatform;
    size?: number;
    artworkSize?: number;
  }>(),
  {
    src: '',
    platform: 'macosBundle',
    size: 36,
    artworkSize: 0,
  }
);
const emit = defineEmits<{
  error: [];
}>();

const resolvedArtworkSize = computed(() => {
  if (props.artworkSize > 0) return props.artworkSize;
  // Windows icon resources usually fill their canvas while macOS ICNS artwork includes optical
  // padding. Normalizing only the artwork keeps alignment slots identical across platforms.
  return props.platform === 'windowsRegistry' ? Math.round(props.size * 0.85) : props.size;
});
</script>

<template>
  <span
    class="md-application-icon"
    :class="{
      resolved: Boolean(src),
      'windows-fallback-container': !src && platform === 'windowsRegistry',
    }"
    :style="{ width: `${size}px`, height: `${size}px` }"
  >
    <img
      v-if="src"
      :src="src"
      alt=""
      :style="{ width: `${resolvedArtworkSize}px`, height: `${resolvedArtworkSize}px` }"
      @error="emit('error')"
    />
    <span v-else-if="platform === 'windowsRegistry'" class="windows-fallback" aria-hidden="true">
      <MdIconWindowsExecutable :size="Math.round(size * 0.72)" />
    </span>
    <MdIcon v-else :name="ICON_NAMES.application" :size="Math.round(size * 0.56)" />
  </span>
</template>

<style scoped>
@reference "@assets/main.css";

.md-application-icon {
  @apply bg-primary/10 text-primary;
  display: grid;
  flex: none;
  overflow: hidden;
  place-items: center;
  border-radius: calc(var(--radius) - 2px);
}

.md-application-icon.resolved {
  background: transparent;
}

.md-application-icon.windows-fallback-container {
  background: transparent;
}

img {
  object-fit: contain;
}

.windows-fallback {
  display: grid;
  width: 100%;
  height: 100%;
  place-items: center;
}
</style>
