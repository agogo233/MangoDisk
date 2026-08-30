import { CLEANUP_SCAN_SCOPE_MODES, type CleanupScanScope } from '@/lib/models/cleanup';

export class CleanupScanScopeUtils {
  static includesStandardCleanup(scope: CleanupScanScope): boolean {
    return scope.mode !== CLEANUP_SCAN_SCOPE_MODES.custom || scope.includeStandardRules;
  }
}
