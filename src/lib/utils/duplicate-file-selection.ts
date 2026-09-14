import { DUPLICATE_KEEPER_RULE_IDS } from '@/lib/models/duplicate-file';
import type { DuplicateFileEntry, DuplicateGroup, DuplicateKeeperRuleId } from '@/lib/models/duplicate-file';
/**
 * Selects removable copies while preserving one suggested keeper per group.
 * Changing the policy can recompute selections without filesystem I/O.
 */
export function keeper(
  entries: readonly DuplicateFileEntry[],
  rule: DuplicateKeeperRuleId
): DuplicateFileEntry | undefined {
  return [...entries].sort((left, right) => compare(left, right, rule))[0];
}
export function suggestedPaths(groups: readonly DuplicateGroup[], rule: DuplicateKeeperRuleId): string[] {
  return groups.flatMap(group => {
    const suggestedKeeper = keeper(group.entries, rule);
    return group.entries.filter(entry => entry.path !== suggestedKeeper?.path).map(entry => entry.path);
  });
}
export function selectedEntries(
  groups: readonly DuplicateGroup[],
  selectedPaths: readonly string[]
): DuplicateFileEntry[] {
  const selected = new Set(selectedPaths);
  return groups.flatMap(group => group.entries).filter(entry => selected.has(entry.path));
}
export function updateEntrySelection(
  selectedPaths: readonly string[],
  entry: DuplicateFileEntry,
  group: DuplicateGroup,
  selected: boolean
): string[] {
  const next = new Set(selectedPaths);
  if (selected) {
    // Every interaction must leave at least one copy in the group.
    const hasOtherKeeper = group.entries.some(item => item.path !== entry.path && !next.has(item.path));
    if (!hasOtherKeeper) return [...selectedPaths];
    next.add(entry.path);
  } else {
    next.delete(entry.path);
  }
  return [...next];
}
export function selectGroupCopies(
  selectedPaths: readonly string[],
  group: DuplicateGroup,
  rule: DuplicateKeeperRuleId
): string[] {
  const suggestedKeeper = keeper(group.entries, rule);
  const next = new Set(selectedPaths);
  group.entries.forEach(entry => {
    if (entry.path === suggestedKeeper?.path) next.delete(entry.path);
    else next.add(entry.path);
  });
  return [...next];
}
export function toggleGroupCopies(
  selectedPaths: readonly string[],
  group: DuplicateGroup,
  rule: DuplicateKeeperRuleId
): string[] {
  const suggestedKeeper = keeper(group.entries, rule);
  const selected = new Set(selectedPaths);
  const selectionApplied = group.entries.every(
    entry => selected.has(entry.path) === (entry.path !== suggestedKeeper?.path)
  );
  if (!selectionApplied) return selectGroupCopies(selectedPaths, group, rule);
  // Clearing only this group preserves selections made in every other
  // duplicate group and makes the group action behave as a true toggle.
  group.entries.forEach(entry => selected.delete(entry.path));
  return [...selected];
}
function compare(left: DuplicateFileEntry, right: DuplicateFileEntry, rule: DuplicateKeeperRuleId): number {
  if (rule === DUPLICATE_KEEPER_RULE_IDS.shortestName) {
    return (
      left.name.length - right.name.length ||
      left.name.localeCompare(right.name, undefined, { numeric: true }) ||
      compareShortestPath(left, right)
    );
  }
  if (rule === DUPLICATE_KEEPER_RULE_IDS.oldestModified) {
    return compareModifiedTime(left, right, false) || compareShortestPath(left, right);
  }
  if (rule === DUPLICATE_KEEPER_RULE_IDS.newestModified) {
    return compareModifiedTime(left, right, true) || compareShortestPath(left, right);
  }
  /*
   * Preserve the shortest path by default, then use modification time and
   * the full path as deterministic tie-breakers.
   */
  return (
    left.path.length - right.path.length ||
    compareModifiedTime(left, right, false) ||
    left.path.localeCompare(right.path, undefined, { numeric: true })
  );
}
function compareShortestPath(left: DuplicateFileEntry, right: DuplicateFileEntry): number {
  return left.path.length - right.path.length || left.path.localeCompare(right.path, undefined, { numeric: true });
}
/**
 * Files without modification times sort after known values. Stable path
 * ordering resolves groups where every timestamp is unavailable.
 */
function compareModifiedTime(left: DuplicateFileEntry, right: DuplicateFileEntry, newestFirst: boolean): number {
  if (left.modifiedAtMs === null && right.modifiedAtMs === null) return 0;
  if (left.modifiedAtMs === null) return 1;
  if (right.modifiedAtMs === null) return -1;
  return newestFirst ? right.modifiedAtMs - left.modifiedAtMs : left.modifiedAtMs - right.modifiedAtMs;
}
