<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import { useI18n } from 'vue-i18n';

import MdCopyButton from '@/components/custom/md-copy-button.vue';
import MdDialogContent from '@/components/custom/md-dialog-content.vue';
import MdDialogFooter from '@/components/custom/md-dialog-footer.vue';
import MdDialogHeader from '@/components/custom/md-dialog-header.vue';
import MdSafeRichText from '@/components/custom/md-safe-rich-text.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { Button } from '@/components/ui/button';
import { Dialog, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import { ICON_NAMES } from '@/lib/models/ui';

import {
  THIRD_PARTY_COMPONENTS,
  type ThirdPartyComponent,
  type ThirdPartyDocument,
} from '@/lib/models/third-party-licenses';

defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: []; openLink: [url: string] }>();
const { t } = useI18n({ useScope: 'global' });
const selection = ref<{ component: ThirdPartyComponent; document: ThirdPartyDocument } | null>(null);
const documentElement = ref<HTMLElement>();
const componentList = ref<HTMLElement>();
const title = computed(() =>
  selection.value
    ? t('thirdPartyLicenses.documentTitle', {
        component: t(selection.value.component.nameKey),
        version: selection.value.component.version,
        document: t(selection.value.document.titleKey),
      })
    : t('thirdPartyLicenses.title')
);
const description = computed(() =>
  selection.value
    ? t('thirdPartyLicenses.documentDetails', {
        version: selection.value.component.version,
        license: t(selection.value.component.licenseKey),
      })
    : t('thirdPartyLicenses.description')
);

function updateOpen(value: boolean) {
  if (!value) emit('close');
}
async function showDocument(component: ThirdPartyComponent, document: ThirdPartyDocument) {
  selection.value = { component, document };
  await nextTick();
  documentElement.value?.focus();
}
async function showComponents() {
  const previous = selection.value;
  selection.value = null;
  await nextTick();
  const trigger = [
    ...(componentList.value?.querySelectorAll<HTMLButtonElement>('button[data-document-action]') ?? []),
  ].find(button => button.dataset.documentAction === `${previous?.component.id}:${previous?.document.id}`);
  trigger?.focus();
}
</script>

<template>
  <Dialog :open="open" @update:open="updateOpen">
    <MdDialogContent class="third-party-dialog flex flex-col" size="large" :height="selection ? 'tall' : 'auto'">
      <MdDialogHeader class="license-header">
        <DialogTitle>{{ title }}</DialogTitle>
      </MdDialogHeader>
      <DialogDescription class="sr-only">{{ description }}</DialogDescription>

      <div v-if="!selection" ref="componentList" class="component-list">
        <section
          v-for="component in THIRD_PARTY_COMPONENTS"
          :key="component.id"
          class="component-card"
          :aria-labelledby="`${component.id}-heading`"
        >
          <div class="component-heading">
            <h3 :id="`${component.id}-heading`">
              <button type="button" class="component-project" @click="emit('openLink', component.projectUrl)">
                {{ t(component.nameKey) }}
                <MdIcon :name="ICON_NAMES.external" :size="13" aria-hidden="true" />
              </button>
            </h3>
            <span class="component-version">{{ component.version }}</span>
            <span class="component-license">{{ t(component.licenseKey) }}</span>
          </div>
          <p>{{ t(component.descriptionKey) }}</p>
          <p class="component-author">{{ t('thirdPartyLicenses.author', { author: component.author }) }}</p>
          <div v-if="component.documents.length" class="component-actions">
            <Button
              v-for="document in component.documents"
              :key="document.id"
              type="button"
              variant="outline"
              size="sm"
              :data-document-action="`${component.id}:${document.id}`"
              @click="showDocument(component, document)"
            >
              {{ t(document.actionKey) }}
            </Button>
          </div>
        </section>
      </div>

      <div
        v-else
        ref="documentElement"
        class="license-document scrollbar-stable"
        lang="en"
        tabindex="0"
        :aria-label="title"
      >
        <MdSafeRichText
          v-if="selection.document.format === 'markdown'"
          :content="selection.document.content"
          @open-link="emit('openLink', $event)"
        />
        <pre v-else>{{ selection.document.content }}</pre>
      </div>

      <MdDialogFooter class="license-footer" :align="!selection ? 'end' : 'between'">
        <Button v-if="selection" type="button" variant="ghost" size="sm" @click="showComponents">
          {{ t('thirdPartyLicenses.back') }}
        </Button>
        <div class="license-footer-actions">
          <MdCopyButton
            v-if="selection"
            :key="`${selection.component.id}:${selection.document.id}`"
            :text="selection.document.content"
          />
          <Button type="button" variant="outline" @click="emit('close')">{{ t('common.close') }}</Button>
        </div>
      </MdDialogFooter>
    </MdDialogContent>
  </Dialog>
</template>

<style scoped>
@reference "@assets/main.css";

.license-header {
  flex: none;
  padding-inline-end: 60px;
}

.component-list,
.license-document {
  min-height: 0;
  flex: 1;
  overflow: auto;
  padding: 18px 26px;
}

.component-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.component-card {
  flex: none;
  border-width: 1px;
  border-radius: 12px;
  padding: 16px;
  @apply border-border;
}

.component-heading {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.component-heading h3 {
  margin: 0;
  font-size: var(--font-content-body);
  font-weight: 600;
}

.component-project {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border-radius: 3px;
  text-align: start;
  cursor: pointer;
  text-underline-offset: 4px;
  @apply hover:text-primary-text hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring;
}

.component-project :deep(svg) {
  flex: none;
  @apply text-muted-foreground;
}

.component-version,
.component-license {
  font-size: var(--font-content-secondary);
  @apply text-muted-foreground;
}

.component-license {
  border-radius: 5px;
  padding: 2px 6px;
  @apply bg-muted;
}

.component-card p {
  margin: 10px 0 12px;
  font-size: var(--font-content-secondary);
  line-height: 1.6;
  @apply text-muted-foreground;
}

.component-card .component-author {
  margin-top: -6px;
}

.component-card p:last-child {
  margin-bottom: 0;
}

.component-actions,
.license-footer-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.component-actions :deep(button) {
  white-space: normal;
  text-align: start;
  height: auto;
  min-height: 32px;
}

.license-document,
.license-document :deep(*) {
  -webkit-user-select: text;
  user-select: text;
}

.license-document:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}

.license-document pre {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: var(--font-content-secondary);
  line-height: 1.6;
}

.license-document :deep(table) {
  display: block;
  max-width: 100%;
  overflow-x: auto;
}

.license-document :deep(th),
.license-document :deep(td) {
  vertical-align: top;
}

.license-document :deep(th),
.license-document :deep(td:nth-child(2)),
.license-document :deep(td:nth-child(4)) {
  white-space: nowrap;
}

.license-footer {
  flex-wrap: wrap;
}
</style>
