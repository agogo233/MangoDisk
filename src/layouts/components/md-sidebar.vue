<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdIcon from '@/components/icons/md-icon.vue';
import MdIconMangodisk from '@/components/icons/md-icon-mangodisk.vue';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { APP_NAME, PRIMARY_NAV_ITEMS, SECONDARY_NAV_ITEMS } from '@/lib/models/application-shell';
import type { PageId } from '@/lib/models/application-shell';

const { t } = useI18n({ useScope: 'global' });

const props = withDefaults(
  defineProps<{
    currentPage: PageId;
    busyPages: PageId[];
    noticePages: PageId[];
    showBrand?: boolean;
    expanded?: boolean;
  }>(),
  {
    showBrand: true,
    expanded: false,
  }
);
const emit = defineEmits<{ navigate: [page: PageId] }>();
const openTooltipPage = ref<PageId | null>(null);

function updateTooltip(page: PageId, open: boolean) {
  if (props.expanded) {
    openTooltipPage.value = null;
    return;
  }
  if (open) {
    openTooltipPage.value = page;
  } else if (openTooltipPage.value === page) {
    openTooltipPage.value = null;
  }
}

function navigate(page: PageId) {
  openTooltipPage.value = null;
  emit('navigate', page);
}

function isBusy(page: PageId): boolean {
  return props.busyPages.includes(page);
}

watch(
  [() => props.expanded, () => props.currentPage],
  () => {
    openTooltipPage.value = null;
  },
  { flush: 'sync' }
);
</script>

<template>
  <aside class="sidebar" :class="{ expanded }">
    <div v-if="showBrand" class="brand">
      <span class="brand-icon">
        <MdIconMangodisk :size="expanded ? 40 : 44" />
      </span>
      <strong v-if="expanded">{{ APP_NAME }}</strong>
    </div>

    <nav class="nav-list" :aria-label="APP_NAME">
      <Tooltip
        v-for="item in PRIMARY_NAV_ITEMS"
        :key="item.id"
        :disabled="expanded"
        :open="!expanded && openTooltipPage === item.id"
        @update:open="updateTooltip(item.id, $event)"
      >
        <TooltipTrigger as-child>
          <button
            type="button"
            :aria-label="t(`navigation.${item.id}`)"
            :aria-current="currentPage === item.id ? 'page' : undefined"
            :aria-busy="isBusy(item.id)"
            class="nav-item"
            :class="{ active: currentPage === item.id }"
            @click="navigate(item.id)"
          >
            <span class="nav-icon" aria-hidden="true">
              <MdIcon :name="item.icon" />
              <span v-if="!expanded && isBusy(item.id)" class="nav-icon-status md-operational-motion" />
            </span>
            <span v-if="expanded" class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
            <span class="nav-accessory">
              <span v-if="expanded && isBusy(item.id)" class="nav-status md-operational-motion" aria-hidden="true" />
            </span>
          </button>
        </TooltipTrigger>
        <TooltipContent v-if="!expanded" side="right" :side-offset="10">
          {{ t(`navigation.${item.id}`) }}
        </TooltipContent>
      </Tooltip>
    </nav>

    <div class="sidebar-footer">
      <Tooltip
        v-for="item in SECONDARY_NAV_ITEMS"
        :key="item.id"
        :disabled="expanded"
        :open="!expanded && openTooltipPage === item.id"
        @update:open="updateTooltip(item.id, $event)"
      >
        <TooltipTrigger as-child>
          <button
            type="button"
            :aria-label="t(`navigation.${item.id}`)"
            :aria-current="currentPage === item.id ? 'page' : undefined"
            :aria-busy="isBusy(item.id)"
            class="nav-item"
            :class="{ active: currentPage === item.id }"
            @click="navigate(item.id)"
          >
            <span class="nav-icon" aria-hidden="true">
              <MdIcon :name="item.icon" />
              <span v-if="!expanded && isBusy(item.id)" class="nav-icon-status md-operational-motion" />
            </span>
            <span v-if="expanded" class="nav-label">{{ t(`navigation.${item.id}`) }}</span>
            <span class="nav-accessory">
              <span v-if="expanded && isBusy(item.id)" class="nav-status md-operational-motion" aria-hidden="true" />
              <span
                v-else-if="!isBusy(item.id) && noticePages.includes(item.id)"
                class="nav-notice"
                :aria-label="t('updates.navigationNotice')"
              />
            </span>
          </button>
        </TooltipTrigger>
        <TooltipContent v-if="!expanded" side="right" :side-offset="10">
          {{ t(`navigation.${item.id}`) }}
        </TooltipContent>
      </Tooltip>
    </div>
  </aside>
</template>

<style scoped>
@reference "@assets/main.css";
.sidebar {
  display: flex;
  width: var(--sidebar-width, 256px);
  min-width: var(--sidebar-width, 256px);
  height: 100vh;
  flex-direction: column;
  @apply bg-transparent text-sidebar-foreground;
}
.brand {
  display: flex;
  height: var(--layout-sidebar-brand-height);
  flex: none;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  @apply text-foreground;
}
.brand-icon {
  display: grid;
  width: 44px;
  height: 44px;
  overflow: hidden;
  place-items: center;
  filter: drop-shadow(0 2px 2px var(--shadow-subtle));
  filter: drop-shadow(0 2px 2px color-mix(in oklab, var(--brand-stem, var(--foreground)) 16%, transparent));
}
.sidebar.expanded .brand {
  flex-direction: row;
  justify-content: flex-start;
  gap: 9px;
  padding-inline: 20px;
}
.sidebar.expanded .brand-icon {
  width: 40px;
  height: 40px;
  overflow: visible;
}
.brand strong {
  font-size: 18px;
  font-weight: 650;
  line-height: 1;
  letter-spacing: -0.35px;
}
.nav-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-inline: 8px;
  padding-block: 4px;
}
.nav-item {
  position: relative;
  display: flex;
  width: 100%;
  height: var(--layout-sidebar-item-height);
  align-items: center;
  justify-content: center;
  gap: 12px;
  border: 0;
  border-radius: 8px;
  padding: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 14px;
  cursor: pointer;
  transition:
    background-color 0.16s ease,
    color 0.16s ease,
    box-shadow 0.16s ease;
}
.sidebar.expanded .nav-list,
.sidebar.expanded .sidebar-footer {
  padding-inline: 10px;
}
.sidebar.expanded .nav-item {
  justify-content: flex-start;
  padding-inline: 12px;
}
.nav-item:hover:not(.active) {
  background: var(--sidebar-accent);
  background: color-mix(in oklab, var(--sidebar-accent) 52%, transparent);
  color: var(--sidebar-foreground);
}
.nav-item:active:not(.active) {
  background: var(--sidebar-accent);
  background: color-mix(in oklab, var(--sidebar-accent) 72%, transparent);
}
.nav-item.active {
  @apply bg-sidebar-accent text-sidebar-accent-foreground;
  font-weight: 600;
}
.nav-item.active::before {
  position: absolute;
  left: 0;
  width: 3px;
  height: 24px;
  border-radius: 999px;
  @apply bg-primary;
  content: '';
}
.nav-item:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--focus-ring-subtle);
  box-shadow: inset 0 0 0 2px color-mix(in oklab, var(--primary) 32%, transparent);
}
.nav-icon {
  position: relative;
  display: grid;
  width: 24px;
  height: 24px;
  flex: none;
  place-items: center;
  color: currentColor;
  font-size: 22px;
  line-height: 1;
}
.nav-icon-status {
  position: absolute;
  inset: -4px;
  border: 1.5px solid var(--border-primary-subtle);
  border: 1.5px solid color-mix(in oklab, var(--primary) 16%, transparent);
  border-top-color: var(--primary);
  border-top-color: color-mix(in oklab, var(--primary) 88%, transparent);
  border-right-color: var(--primary);
  border-right-color: color-mix(in oklab, var(--primary) 46%, transparent);
  border-radius: 50%;
  pointer-events: none;
  animation: nav-spin 0.9s linear infinite;
}
.nav-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nav-accessory {
  position: absolute;
  top: 8px;
  right: 8px;
  display: grid;
  width: 12px;
  height: 12px;
  flex: none;
  place-items: center;
}
.sidebar.expanded .nav-accessory {
  position: static;
  margin-left: auto;
}
.nav-status {
  width: 11px;
  height: 11px;
  aspect-ratio: 1;
  border: 1.5px solid var(--border-primary-subtle);
  border: 1.5px solid color-mix(in oklab, var(--primary) 20%, transparent);
  border-top-color: var(--primary);
  border-top-color: color-mix(in oklab, var(--primary) 78%, transparent);
  border-radius: 50%;
  animation: nav-spin 0.75s linear infinite;
}
.nav-notice {
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 50%;
  @apply bg-destructive ring-2 ring-sidebar;
}
.sidebar-footer {
  display: flex;
  margin-top: auto;
  flex-direction: column;
  gap: 3px;
  padding-inline: 8px;
  padding-block: 8px 14px;
}
@keyframes nav-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
