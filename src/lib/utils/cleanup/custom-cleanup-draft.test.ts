import { describe, expect, it } from 'vitest';
import type { CustomCleanupRule } from '@/lib/models/cleanup/custom-cleanup';
import {
  cloneCustomCleanupRule,
  customCleanupBytesFromMegabytes,
  customCleanupMegabytesFromBytes,
  customCleanupPatternsFromText,
  customCleanupRuleValidation,
} from './custom-cleanup-draft';

const rule: CustomCleanupRule = {
  schemaVersion: 1,
  id: 'logs',
  name: 'Logs',
  roots: ['/fixture/logs'],
  namePatterns: ['*.log'],
  minimumBytes: null,
  maximumBytes: null,
  modifiedTime: { mode: 'olderThan', days: 30 },
  recursive: true,
  removeEmptyDirectories: false,
};

describe('custom cleanup drafts', () => {
  it('isolates nested draft edits from the saved rule', () => {
    const draft = cloneCustomCleanupRule(rule);
    draft.roots.push('/fixture/cache');
    draft.namePatterns.push('*.tmp');
    draft.modifiedTime = { mode: 'olderThan', days: 60 };
    expect(rule.roots).toEqual(['/fixture/logs']);
    expect(rule.namePatterns).toEqual(['*.log']);
    expect(draft.modifiedTime).not.toBe(rule.modifiedTime);
  });

  it.each(['../*.log', 'cache\\*.log', '**.log', ' ', 'x'.repeat(81)])(
    'retains invalid-pattern feedback for %s',
    pattern => {
      expect(customCleanupRuleValidation({ ...rule, namePatterns: [pattern] }).patternsInvalid).toBe(true);
    }
  );

  it('keeps missing fields separate from invalid filters', () => {
    expect(customCleanupRuleValidation({ ...rule, name: ' ', roots: [], namePatterns: [] })).toEqual({
      missingName: true,
      missingDirectories: true,
      missingPatterns: true,
      patternsInvalid: false,
      sizeRangeInvalid: false,
      modifiedTimeInvalid: false,
    });
    expect(customCleanupRuleValidation({ ...rule, minimumBytes: 2, maximumBytes: 1 }).sizeRangeInvalid).toBe(true);
    expect(customCleanupRuleValidation({ ...rule, minimumBytes: 2, maximumBytes: 2 }).sizeRangeInvalid).toBe(false);
  });

  it.each([0, -1, 1.5, 3651, Number.NaN])('rejects invalid modified-time days %s', days => {
    expect(
      customCleanupRuleValidation({ ...rule, modifiedTime: { mode: 'newerThan', days } }).modifiedTimeInvalid
    ).toBe(true);
  });

  it('preserves comma parsing and optional size conversion', () => {
    expect(customCleanupPatternsFromText(' *.log, , *.tmp ')).toEqual(['*.log', '*.tmp']);
    expect(customCleanupPatternsFromText(undefined)).toEqual([]);
    for (const value of [undefined, '', ' ', '-1', 'NaN', 'Infinity']) {
      expect(customCleanupBytesFromMegabytes(value)).toBeNull();
    }
    expect(customCleanupBytesFromMegabytes('0')).toBe(0);
    expect(customCleanupBytesFromMegabytes('1.5')).toBe(1572864);
    expect(customCleanupBytesFromMegabytes('0.000001')).toBe(1);
    expect(customCleanupMegabytesFromBytes(null)).toBe('');
    expect(customCleanupMegabytesFromBytes(1572864)).toBe('1.5');
  });
});
