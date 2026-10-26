import {
  MAX_CUSTOM_CLEANUP_FILTER_DAYS,
  MAX_CUSTOM_CLEANUP_PATTERNS_PER_RULE,
  MAX_CUSTOM_CLEANUP_TEXT_LENGTH,
  type CustomCleanupRule,
} from '@/lib/models/cleanup/custom-cleanup';

const MEBIBYTE = 1024 * 1024;

export function cloneCustomCleanupRule(rule: CustomCleanupRule): CustomCleanupRule {
  return {
    ...rule,
    roots: [...rule.roots],
    namePatterns: [...rule.namePatterns],
    modifiedTime: { ...rule.modifiedTime },
  };
}

export function customCleanupRuleValidation(rule: CustomCleanupRule) {
  const modifiedTime = rule.modifiedTime;
  return {
    missingName: rule.name.trim().length === 0,
    missingDirectories: rule.roots.length === 0,
    missingPatterns: rule.namePatterns.length === 0,
    patternsInvalid:
      rule.namePatterns.length > 0 &&
      (rule.namePatterns.length > MAX_CUSTOM_CLEANUP_PATTERNS_PER_RULE ||
        rule.namePatterns.some(
          pattern =>
            !pattern.trim() ||
            pattern.trim().length > MAX_CUSTOM_CLEANUP_TEXT_LENGTH ||
            /[/\\]/u.test(pattern) ||
            pattern.includes('**')
        )),
    sizeRangeInvalid: rule.minimumBytes !== null && rule.maximumBytes !== null && rule.minimumBytes > rule.maximumBytes,
    modifiedTimeInvalid:
      modifiedTime.mode !== 'any' &&
      (!Number.isSafeInteger(modifiedTime.days) ||
        modifiedTime.days < 1 ||
        modifiedTime.days > MAX_CUSTOM_CLEANUP_FILTER_DAYS),
  };
}

export function customCleanupPatternsFromText(value: string | undefined): string[] {
  return (value ?? '')
    .split(',')
    .map(pattern => pattern.trim())
    .filter(Boolean);
}

export function customCleanupMegabytesFromBytes(bytes: number | null): string {
  return bytes === null ? '' : String(bytes / MEBIBYTE);
}

export function customCleanupBytesFromMegabytes(value: string | undefined): number | null {
  const text = value ?? '';
  const number = Number(text);
  return text.trim() === '' || !Number.isFinite(number) || number < 0 ? null : Math.round(number * MEBIBYTE);
}
