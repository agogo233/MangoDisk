<script setup lang="ts">
import { computed, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { toast } from 'vue-sonner';

import { useAppStore } from '@/stores/app-store';

const APPLICATION_ERROR_TOAST_ID = 'application-error';
const store = useAppStore();
const { t } = useI18n({ useScope: 'global' });

const errorMessage = computed(() => {
  switch (store.errorReason) {
    case 'resourceBusy':
      return t('errorReasons.resourceBusy.message');
    case 'accessDeniedOrBusy':
      return t('errorReasons.accessDeniedOrBusy.message');
    case 'itemChanged':
      return t('errorReasons.itemChanged.message');
    case 'folderUnavailable':
      return t('errorReasons.folderUnavailable.message');
    case 'folderSelectionLimitExceeded':
      return t('errorReasons.folderSelectionLimitExceeded.message');
    case 'scanResourcesReleasing':
      return t('errorReasons.scanResourcesReleasing.message');
    default:
      return store.errorCode ? t(`errors.${store.errorCode}`) : '';
  }
});

const errorTitle = computed(() => {
  switch (store.errorReason) {
    case 'resourceBusy':
      return t('errorReasons.resourceBusy.title');
    case 'accessDeniedOrBusy':
      return t('errorReasons.accessDeniedOrBusy.title');
    case 'itemChanged':
      return t('errorReasons.itemChanged.title');
    case 'folderUnavailable':
      return t('errorReasons.folderUnavailable.title');
    case 'folderSelectionLimitExceeded':
      return t('errorReasons.folderSelectionLimitExceeded.title');
    case 'scanResourcesReleasing':
      return t('errorReasons.scanResourcesReleasing.title');
    default:
      return store.errorCode ? t(`errorTitles.${store.errorCode}`) : t('common.operationFailed');
  }
});

watch(
  [() => store.errorCode, () => store.errorReason, errorTitle, errorMessage],
  ([errorCode, errorReason, title, message]) => {
    if (!errorCode) {
      toast.dismiss(APPLICATION_ERROR_TOAST_ID);
      return;
    }
    // One notification renderer owns measurement and stacking for every command error. Keeping
    // this adapter outside the application shell also prevents page navigation from owning error
    // presentation details.
    toast.error(title, {
      id: APPLICATION_ERROR_TOAST_ID,
      description: message,
      duration: Infinity,
      onDismiss: () => {
        if (store.errorCode === errorCode && store.errorReason === errorReason) store.clearError();
      },
    });
  },
  { immediate: true }
);
</script>

<template><span class="hidden" aria-hidden="true" /></template>
