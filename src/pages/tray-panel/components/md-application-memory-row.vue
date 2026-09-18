<script setup lang="ts">
import MdTooltip from '@/components/custom/md-tooltip.vue';
import { computed, ref, watch } from 'vue';
import { useMemoryReleaseStore } from '@/stores/memory-release-store';
import { OperatingSystemService } from '@/lib/services/operating-system-service';
import { useI18n } from 'vue-i18n';
import MdNativeFileIcon from '@/components/custom/md-native-file-icon.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import type { ApplicationQuitStatus } from '@/lib/models/resident';
import type { ApplicationMemory } from '@/lib/models/system-resources';
import { ICON_NAMES } from '@/lib/models/ui';
import { ByteSizeService } from '@/lib/services/byte-size-service';
import { ResidentService } from '@/lib/services/resident-service';
import { FileManagerService } from '@/lib/services/file-manager-service';
import { LoggerService } from '@/lib/services/logger-service';

const props = defineProps<{ application: ApplicationMemory; share: number; expanded: boolean }>();
const emit = defineEmits<{ toggle: [] }>();
const { t } = useI18n({ useScope: 'global' });
const memorySettings = useMemoryReleaseStore();
const windows = OperatingSystemService.isWindows();
const excluded = computed(() => memorySettings.excluded(props.application.iconPath));
const revealing = ref(false);
const failed = ref(false);
const quitting = ref(false);
const quitStatus = ref<ApplicationQuitStatus | 'failed' | null>(null);
const quitMessages = {
  requested: 'monitoring.quitRequested',
  unavailable: 'monitoring.quitUnavailable',
  unsupported: 'monitoring.quitUnsupported',
  failed: 'monitoring.quitFailed',
} as const;
const quitLabels = {
  requested: 'monitoring.quitButtonRequested',
  unavailable: 'monitoring.quitButtonUnavailable',
  unsupported: 'monitoring.quitButtonUnsupported',
  failed: 'monitoring.quitButtonFailed',
} as const;
const quitLabel = computed(() =>
  quitting.value
    ? 'monitoring.quittingApplication'
    : quitStatus.value
      ? quitLabels[quitStatus.value]
      : 'memoryRelease.quitAction'
);
watch(
  () => props.expanded,
  () => {
    failed.value = false;
    quitStatus.value = null;
  }
);

async function quit() {
  if (!props.application.canQuit || quitting.value) return;
  quitting.value = true;
  quitStatus.value = null;
  try {
    // The backend resolves this identity again; no PID or path is supplied by UI.
    quitStatus.value = await ResidentService.quitApplication(props.application.id);
  } catch {
    quitStatus.value = 'failed';
    LoggerService.warn('monitoring', 'application_quit_failed');
  } finally {
    quitting.value = false;
  }
}

async function reveal() {
  const path = props.application.iconPath;
  if (!path || revealing.value) return;
  revealing.value = true;
  failed.value = false;
  try {
    // The sampler's bundle/executable path is also the native icon identity.
    // Reuse file-manager navigation; never execute a sampled process image.
    await FileManagerService.reveal(path);
  } catch {
    failed.value = true;
    LoggerService.warn('monitoring', 'application_reveal_failed');
  } finally {
    revealing.value = false;
  }
}
</script>

<template>
  <li>
    <button
      class="application-row"
      :aria-expanded="expanded"
      :aria-controls="expanded ? `application-details-${application.id}` : undefined"
      @click="emit('toggle')"
    >
      <span class="memory-share" aria-hidden="true" :style="{ width: `${share}%` }" />
      <MdNativeFileIcon
        v-if="application.iconPath"
        :path="application.iconPath"
        :name="application.name"
        :directory="application.isBundle"
        directory-mode="path"
        compact
      />
      <span v-else class="fallback-icon"><MdIcon :name="ICON_NAMES.application" :size="20" /></span>
      <span class="application-name">{{ application.name }}</span>
      <MdTooltip v-if="windows && excluded" :text="t('memoryRelease.exclusionsHint')"
        ><span class="excluded-badge">{{ t('memoryRelease.excluded') }}</span></MdTooltip
      >
      <strong>{{ ByteSizeService.memory(application.residentBytes) }}</strong>
      <MdIcon class="disclosure" :name="expanded ? ICON_NAMES.chevronUp : ICON_NAMES.chevronDown" :size="12" />
    </button>
    <div v-if="expanded" :id="`application-details-${application.id}`" class="application-details">
      <span>{{ t('monitoring.processCount', { count: application.processCount }) }}</span>
      <template v-if="application.iconPath">
        <span class="application-path">{{ application.iconPath }}</span>
      </template>
      <span v-else>{{ t('monitoring.locationUnavailable') }}</span>
      <div class="application-actions">
        <MdTooltip v-if="application.iconPath" :text="t('common.showInFileManager')"
          ><button class="reveal-button" :disabled="revealing" @click="reveal">
            <MdIcon :name="ICON_NAMES.folderOpen" :size="13" />
            {{ t('memoryRelease.revealAction') }}
          </button></MdTooltip
        >
        <MdTooltip v-if="windows && application.iconPath" :text="t('memoryRelease.exclusionsHint')"
          ><button
            class="reveal-button"
            :disabled="!memorySettings.preferences || memorySettings.saving"
            :aria-pressed="excluded"
            @click="memorySettings.toggle({ name: application.name, path: application.iconPath })"
          >
            <MdIcon :name="ICON_NAMES.shield" :size="13" />
            <span class="quit-label">
              <span class="quit-label-sizer" aria-hidden="true">{{ t('memoryRelease.includeAction') }}</span>
              <span class="quit-label-sizer" aria-hidden="true">{{ t('memoryRelease.excludeAction') }}</span>
              <span role="status">{{
                t(excluded ? 'memoryRelease.includeAction' : 'memoryRelease.excludeAction')
              }}</span>
            </span>
          </button></MdTooltip
        >
        <MdTooltip
          v-if="application.canQuit"
          :text="quitStatus ? t(quitMessages[quitStatus]) : t('monitoring.quitApplication')"
          ><button
            class="quit-application-button"
            :disabled="quitting"
            :aria-busy="quitting"
            :aria-label="t('monitoring.quitNamedApplication', { name: application.name })"
            @click="quit"
          >
            <MdIcon
              :name="quitting ? ICON_NAMES.refresh : quitStatus === 'requested' ? ICON_NAMES.check : ICON_NAMES.startup"
              :class="{ 'animate-spin motion-reduce:animate-none': quitting }"
              :size="13"
            />
            <span class="quit-label">
              <span :role="quitStatus === 'failed' ? 'alert' : 'status'" aria-atomic="true">{{ t(quitLabel) }}</span>
            </span>
          </button></MdTooltip
        >
      </div>
      <span v-if="failed" role="alert">{{ t('monitoring.revealFailed') }}</span>
    </div>
  </li>
</template>

<style scoped>
@reference "@assets/main.css";
li {
  margin: 2px 0;
}
.application-row {
  display: flex;
  align-items: center;
  gap: 8px;
  position: relative;
  isolation: isolate;
  width: 100%;
  min-height: 36px;
  padding: 3px 6px;
  border-radius: 5px;
  overflow: hidden;
  text-align: left;
  cursor: pointer;
}
.application-row:hover,
.application-row[aria-expanded='true'] {
  @apply bg-muted/60;
}
button:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
.memory-share {
  position: absolute;
  z-index: -1;
  inset: 0 auto 0 0;
  /* Explicit opacity also works in the WebView shipped with macOS 12.5. */
  background: var(--foreground);
  opacity: 0.05;
  pointer-events: none;
}
.excluded-badge {
  @apply text-muted-foreground;
  font-size: 10px;
  flex: none;
}
.application-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
strong {
  flex: none;
  font-size: 12px;
  font-weight: 550;
  font-variant-numeric: tabular-nums;
}
.fallback-icon {
  @apply text-muted-foreground;
  width: 30px;
  height: 30px;
  display: grid;
  place-items: center;
  flex: none;
}
.disclosure {
  @apply text-muted-foreground;
  flex: none;
}
.application-details {
  @apply text-muted-foreground;
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 8px 12px 12px 44px;
  font-size: 11px;
}
.application-details::before {
  content: '';
  position: absolute;
  inset: 8px auto 12px 21px;
  width: 1px;
  background: var(--border);
  pointer-events: none;
}
.application-path {
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}
.application-actions {
  display: flex;
  align-self: stretch;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 8px;
}
.reveal-button,
.quit-application-button {
  @apply text-primary;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 4px;
  margin-left: -4px;
  border-radius: 4px;
  cursor: pointer;
  font: inherit;
  text-align: left;
  background-color: transparent;
}
/* Keep status feedback inside the existing control, including at narrow widths. */
.quit-application-button {
  max-width: 100%;
  min-width: 0;
  margin-inline-start: auto;
}
.quit-application-button > :first-child {
  flex: none;
}
.quit-label {
  display: grid;
  min-width: 0;
}
.quit-label > span {
  grid-area: 1 / 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.quit-label-sizer {
  visibility: hidden;
}
.reveal-button:hover:not(:disabled),
.quit-application-button:hover:not(:disabled) {
  @apply bg-accent;
}
.reveal-button:disabled,
.quit-application-button:disabled {
  opacity: 0.5;
  cursor: default;
}
[role='alert'] {
  @apply text-destructive;
}
</style>
