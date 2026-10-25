<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSettingsRow from '@/components/custom/md-settings-row.vue';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/presentation/ui';
import { ExplorerIntegrationService } from '@/lib/services/settings/explorer-integration-service';
import { LoggerService } from '@/lib/services/diagnostics/logger-service';

const { t, locale } = useI18n({ useScope: 'global' });
const enabled = ref<boolean | null>(null);
const busy = ref(false);
const failed = ref(false);
let titleRefreshPending = false;
let failedUpdate: boolean | null = null;

function refreshPendingTitle() {
  if (!titleRefreshPending || busy.value || enabled.value === null) return;
  titleRefreshPending = false;
  if (enabled.value) void update(true);
}

async function load() {
  if (busy.value) return;
  busy.value = true;
  failed.value = false;
  failedUpdate = null;
  try {
    enabled.value = (await ExplorerIntegrationService.status()).enabled;
  } catch (error) {
    failed.value = true;
    LoggerService.warn('settings', 'explorer_integration_read_failed', { error });
  } finally {
    busy.value = false;
    refreshPendingTitle();
  }
}

async function update(value: boolean) {
  // An explicit retry retains the requested state even if reconciliation failed.
  if (busy.value) return;
  busy.value = true;
  failed.value = false;
  failedUpdate = null;
  try {
    enabled.value = (
      await ExplorerIntegrationService.setEnabled(value, t('settings.explorerIntegration.command'))
    ).enabled;
  } catch (error) {
    failed.value = true;
    failedUpdate = value;
    LoggerService.warn('settings', 'explorer_integration_update_failed', { enabled: value, error });
    try {
      enabled.value = (await ExplorerIntegrationService.status()).enabled;
    } catch {
      enabled.value = null;
    }
  } finally {
    busy.value = false;
    refreshPendingTitle();
  }
}

function retry() {
  if (failedUpdate === null) void load();
  else void update(failedUpdate);
}

watch(locale, () => {
  titleRefreshPending = true;
  refreshPendingTitle();
});
onMounted(load);
onBeforeUnmount(() => {
  titleRefreshPending = false;
});
</script>

<template>
  <div>
    <MdSettingsRow
      :title="t('settings.explorerIntegration.title')"
      :description="t('settings.explorerIntegration.description')"
      label-for="explorer-integration"
    >
      <template #icon><MdIcon :name="ICON_NAMES.folder" /></template>
      <MdSwitch
        id="explorer-integration"
        :model-value="enabled ?? false"
        :disabled="busy || enabled === null"
        :aria-label="t('settings.explorerIntegration.title')"
        @update:model-value="update"
      />
    </MdSettingsRow>
    <p v-if="failed" class="integration-error" role="alert">
      {{ t('settings.explorerIntegration.failed') }}
      <button type="button" @click="retry">{{ t('settings.explorerIntegration.retry') }}</button>
    </p>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";
.integration-error {
  @apply text-destructive-text;
  padding: 0 20px 16px;
  font-size: var(--font-content-secondary);
}
.integration-error button {
  text-decoration: underline;
  cursor: pointer;
}
</style>
