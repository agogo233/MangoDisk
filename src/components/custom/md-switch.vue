<script setup lang="ts">
const props = withDefaults(
  defineProps<{
    modelValue?: boolean;
    disabled?: boolean;
    loading?: boolean;
  }>(),
  {
    modelValue: false,
    disabled: false,
    loading: false,
  }
);

const emit = defineEmits<{
  'update:modelValue': [value: boolean];
}>();

function toggle() {
  if (!props.disabled && !props.loading) emit('update:modelValue', !props.modelValue);
}
</script>

<template>
  <button
    type="button"
    class="md-switch"
    role="switch"
    :aria-checked="modelValue"
    :data-state="modelValue ? 'checked' : 'unchecked'"
    :disabled="disabled || loading"
    :aria-busy="loading"
    @click="toggle"
  >
    <span>
      <span v-if="loading" class="md-switch-spinner md-operational-motion" aria-hidden="true" />
    </span>
  </button>
</template>

<style scoped>
.md-switch {
  --switch-spinner-color: var(--switch-spinner);
  position: relative;
  width: 34px;
  height: 20px;
  flex: none;
  border: 1px solid var(--switch-border);
  border-radius: 999px;
  padding: 0;
  background: var(--switch-track);
  cursor: pointer;
  transition:
    border-color 140ms ease,
    background-color 140ms ease;
}
.md-switch > span {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 999px;
  background: var(--switch-thumb);
  box-shadow: 0 1px 2px color-mix(in oklab, var(--foreground) 18%, transparent);
  transition: transform 140ms ease;
}
.md-switch-spinner {
  position: absolute;
  inset: 2px;
  width: 10px;
  height: 10px;
  border: 1.5px solid var(--border);
  border-color: color-mix(in oklab, var(--switch-spinner-color) 22%, transparent);
  border-top-color: var(--switch-spinner-color);
  border-radius: 50%;
  animation: md-switch-spin 0.72s linear infinite;
}
.md-switch[data-state='checked'] {
  --switch-spinner-color: var(--primary);
  border-color: var(--primary);
  background: var(--primary);
}
.md-switch[data-state='checked'] > span {
  background: var(--switch-thumb-checked);
  transform: translateX(14px);
}
.md-switch:focus-visible {
  outline: 2px solid var(--switch-focus-ring);
  outline-offset: 2px;
}
.md-switch:disabled {
  cursor: not-allowed;
  opacity: 0.48;
}
.md-switch:disabled[aria-busy='true'] {
  opacity: 1;
}
@keyframes md-switch-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
