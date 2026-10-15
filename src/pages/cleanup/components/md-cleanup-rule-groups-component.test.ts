// @vitest-environment happy-dom
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia } from 'pinia';
import { expect, it, vi } from 'vitest';
import { i18n } from '@/i18n';
import type { PresentedScanRuleResult } from '@/lib/models/cleanup';
import { ByteSizeService } from '@/lib/services/byte-size-service';
import MdResultItemContent from '@/components/custom/md-result-item-content.vue';
import MdResultCheckbox from '@/components/custom/md-result-checkbox.vue';
import MdCleanupRuleGroups from './md-cleanup-rule-groups.vue';

vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'windows' }));

const rule: PresentedScanRuleResult = {
  ruleId: 'project.rust-build-artifacts',
  category: 'project',
  group: 'project',
  risk: 'recoverable',
  defaultSelected: false,
  recommendedSelected: false,
  bytes: 200,
  fileCount: 1,
  available: true,
  selectable: true,
  status: 'found',
  runningProcesses: ['Codex'],
  requiresAppClose: false,
  sources: [{ path: '/fixture/target', bytes: 200, fileCount: 1, modifiedAtMs: null, blockReason: 'requiresClose' }],
  sourceCount: 1,
  sourcesTruncated: false,
  scanElapsedMs: 1,
  name: 'Rust artifacts',
  description: 'Build output',
  impact: 'Source files remain',
  categoryLabel: 'Projects',
};

it('shows restart guidance for restricted native cleanup and reserves admin check for previous installations', async () => {
  const wrapper = mount(MdCleanupRuleGroups, {
    props: {
      busy: false,
      leftovers: null,
      rules: [
        'special.windows-system-logs',
        'special.windows-update-cleanup',
        'special.windows-previous-installations',
      ].map(
        ruleId =>
          ({
            ...rule,
            ruleId,
            category: 'system',
            group: 'system',
            bytes: ruleId === 'special.windows-system-logs' ? 12 * 1024 * 1024 : 0,
            selectable: false,
            status: 'requiresElevation',
            runningProcesses: [],
            sources: [],
            sourceCount: 0,
          }) as PresentedScanRuleResult
      ),
      selectedLeftoverIds: [],
      selectedRuleIds: [],
      sourceSelections: [],
      privilegedScanRuleId: null,
    },
    global: {
      plugins: [createPinia(), i18n],
      stubs: {
        MdResultTable: { methods: { scrollTo() {} }, template: '<div><slot name="header" /><slot /></div>' },
        MdAiAction: true,
      },
    },
  });
  await flushPromises();
  const summaries = wrapper.findAllComponents(MdResultItemContent);
  expect(summaries.map(summary => summary.props('description'))).toEqual([
    i18n.global.t('cleanup.privilegedScan.restartHint'),
    i18n.global.t('cleanup.privilegedScan.restartHint'),
    undefined,
  ]);
  expect(summaries.map(summary => summary.props('value'))).toEqual([
    ByteSizeService.bytes(12 * 1024 * 1024),
    i18n.global.t('cleanup.privilegedScan.sizePending'),
    i18n.global.t('cleanup.privilegedScan.required'),
  ]);
  expect(summaries.map(summary => summary.props('valueDetail'))).toEqual([
    i18n.global.t('cleanup.privilegedScan.sizePending'),
    undefined,
    i18n.global.t('cleanup.privilegedScan.sizePending'),
  ]);
  const buttons = wrapper.findAll('.privileged-scan-button');
  expect(buttons).toHaveLength(1);
  await buttons[0]!.trigger('click');
  expect(wrapper.emitted('privilegedScan')).toEqual([[]]);
  for (const checkbox of wrapper.findAllComponents(MdResultCheckbox)) {
    expect(checkbox.props('disabled')).toBe(true);
  }
  wrapper.unmount();
});

it('keeps a single rule collapsed and allows selecting its close-required source after expansion', async () => {
  const wrapper = mount(MdCleanupRuleGroups, {
    props: {
      busy: false,
      leftovers: null,
      rules: [rule],
      selectedLeftoverIds: [],
      selectedRuleIds: [],
      sourceSelections: [],
      privilegedScanRuleId: null,
    },
    global: {
      plugins: [createPinia(), i18n],
      stubs: {
        MdResultTable: { methods: { scrollTo() {} }, template: '<div><slot name="header" /><slot /></div>' },
        MdAiAction: true,
        MdIconAction: true,
      },
    },
  });
  await flushPromises();
  expect(wrapper.find('.result-detail-metric small').exists()).toBe(false);
  expect(wrapper.find('.rule-details').exists()).toBe(false);
  const summary = wrapper.findComponent(MdResultItemContent);
  expect(summary.props('expanded')).toBe(false);
  summary.vm.$emit('toggle');
  await flushPromises();
  const source = wrapper.get('.source-row').findComponent(MdResultCheckbox);
  expect(source.props('disabled')).toBe(false);
  source.vm.$emit('update:checked', true);
  expect(wrapper.emitted('toggleSource')).toEqual([[rule.ruleId, '/fixture/target']]);
  expect(wrapper.text()).not.toContain('cleanup.requiresClose');
  wrapper.unmount();
});

it('uses the checkbox for selection state while retaining the total for partial selection', async () => {
  const ruleWithTwoSources: PresentedScanRuleResult = {
    ...rule,
    sources: [
      { path: '/fixture/first', bytes: 100, fileCount: 1, modifiedAtMs: null, blockReason: null },
      { path: '/fixture/second', bytes: 100, fileCount: 1, modifiedAtMs: null, blockReason: null },
    ],
    sourceCount: 2,
  };
  const wrapper = mount(MdCleanupRuleGroups, {
    props: {
      busy: false,
      leftovers: null,
      rules: [ruleWithTwoSources],
      selectedLeftoverIds: [],
      selectedRuleIds: [],
      sourceSelections: [],
      privilegedScanRuleId: null,
    },
    global: {
      plugins: [createPinia(), i18n],
      stubs: {
        MdResultTable: { methods: { scrollTo() {} }, template: '<div><slot name="header" /><slot /></div>' },
        MdAiAction: true,
      },
    },
  });
  await flushPromises();

  const summary = wrapper.findComponent(MdResultItemContent);
  const checkbox = wrapper.findComponent(MdResultCheckbox);
  expect(summary.props('valueDetail')).toBeUndefined();
  expect(checkbox.props('checked')).toBe(false);

  await wrapper.setProps({ selectedRuleIds: [rule.ruleId] });
  expect(summary.props('valueDetail')).toBeUndefined();
  expect(checkbox.props('checked')).toBe(true);

  await wrapper.setProps({
    sourceSelections: [{ ruleId: rule.ruleId, mode: 'include', paths: ['/fixture/first'] }],
  });
  expect(checkbox.props('indeterminate')).toBe(true);
  expect(summary.props('valueDetail')).toBe(
    i18n.global.t('cleanup.totalSize', { size: ByteSizeService.bytes(ruleWithTwoSources.bytes) })
  );
  wrapper.unmount();
});
