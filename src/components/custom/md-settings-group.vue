<script setup lang="ts">
import { Card } from '@/components/ui/card';

defineProps<{ title: string }>();
</script>

<template>
  <section class="settings-section">
    <h2>{{ title }}</h2>
    <Card class="settings-list"><slot /></Card>
  </section>
</template>

<style scoped>
@reference "@assets/main.css";
.settings-section > h2 {
  margin: 1px 0 6px 2px;
  font-size: var(--font-content-body);
  font-weight: 600;
  @apply text-muted-foreground;
}
.settings-list {
  gap: 0;
  overflow: hidden;
  border-radius: 10px;
  @apply border-border/70 bg-card shadow-none;
}
/* Inset dividers belong to the group, including feature-owned row wrappers.
   Paint them without borders so row sizing and the full hover surface stay
   consistent. Opacity also keeps the line subtle on older WebKit versions. */
.settings-section > .settings-list > :deep(* + *) {
  position: relative;
}
.settings-section > .settings-list > :deep(* + *)::before {
  content: '';
  position: absolute;
  z-index: 1;
  top: 0;
  right: 14px;
  left: 14px;
  height: 1px;
  background: var(--border);
  opacity: 0.4;
  pointer-events: none;
}
</style>
