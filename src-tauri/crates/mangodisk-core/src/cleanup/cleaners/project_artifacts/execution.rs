use super::current_platform_rules;
use super::measurement::{measure_directory, validate_artifact_protection};
use super::planning::build_plan;
use super::project_discovery::{project_matches, validate_candidate};
use super::{ProjectExecutionRequest, RulePlan};
#[cfg(test)]
use crate::cleanup::source_selection::SourceSelectionPolicy;
use crate::{
    cleanup::{
        codex_worktrees, exclusions::CleanupExclusions, source_selection::SourceScope,
        CleanupActionKind, CleanupActionReason, CleanupActionResult, CleanupActionStatus,
    },
    filesystem::{
        metadata::diagnostic_path,
        permanent_delete::{delete_path_permanently, prepare_path_for_permanent_delete},
    },
    shared::operation::OperationGuard,
};
use std::{collections::HashMap, sync::atomic::Ordering};

#[cfg(test)]
pub(in crate::cleanup::cleaners) fn execute_selected(
    selected_ids: &[String],
    configured_roots: &[String],
    source_selections: &SourceSelectionPolicy,
    dry_run: bool,
    operation: &OperationGuard,
) -> Vec<CleanupActionResult> {
    execute_selected_with_progress(
        ProjectExecutionRequest {
            selected_ids,
            configured_roots,
            selected_volume_scope: false,
            source_selections,
            dry_run,
            operation,
            exclusions: &CleanupExclusions::default(),
        },
        |_, _| {},
    )
}

/// Executes project rules sequentially and reports the boundary of the rule
/// that is actually running. Project discovery remains shared because
/// rebuilding the same catalog for every selected ecosystem would multiply
/// disk traversal cost.
pub(in crate::cleanup::cleaners) fn execute_selected_with_progress<F>(
    request: ProjectExecutionRequest<'_>,
    mut progress: F,
) -> Vec<CleanupActionResult>
where
    F: FnMut(&str, Option<&CleanupActionResult>),
{
    let ProjectExecutionRequest {
        selected_ids,
        configured_roots,
        selected_volume_scope,
        source_selections,
        dry_run,
        operation,
        exclusions,
    } = request;
    if selected_ids.is_empty() {
        return Vec::new();
    }
    let rules = match current_platform_rules() {
        Ok(rules) => rules,
        Err(error) => {
            log::error!(
                "project_artifact_execute_failed reason=catalogLoad error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            return failed_actions_with_progress(
                selected_ids,
                CleanupActionReason::CleanerUnavailable,
                &mut progress,
            );
        }
    };
    let plan = match build_plan(
        configured_roots,
        selected_volume_scope,
        rules,
        &|| operation.cancelled().load(Ordering::Relaxed),
        exclusions,
    ) {
        Ok(plan) if !plan.limited => plan,
        Ok(_) => {
            return failed_actions_with_progress(
                selected_ids,
                CleanupActionReason::PreflightFailed,
                &mut progress,
            )
        }
        Err(error) => {
            log::warn!(
                "project_artifact_execute_failed reason=preflight error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            return failed_actions_with_progress(
                selected_ids,
                CleanupActionReason::PreflightFailed,
                &mut progress,
            );
        }
    };
    let plans_by_id = plan
        .rules
        .iter()
        .map(|rule| (rule.source.id.as_str(), rule))
        .collect::<HashMap<_, _>>();
    selected_ids
        .iter()
        .map(|id| {
            progress(id, None);
            let action = plans_by_id.get(id.as_str()).map_or_else(
                || failed_action(id, CleanupActionReason::CleanerUnavailable),
                |rule| {
                    execute_rule_with_exclusions(
                        rule,
                        source_selections.scope(id),
                        dry_run,
                        operation,
                        exclusions,
                    )
                },
            );
            progress(id, Some(&action));
            action
        })
        .collect()
}

fn failed_actions_with_progress<F>(
    selected_ids: &[String],
    reason: CleanupActionReason,
    progress: &mut F,
) -> Vec<CleanupActionResult>
where
    F: FnMut(&str, Option<&CleanupActionResult>),
{
    selected_ids
        .iter()
        .map(|id| {
            progress(id, None);
            let action = failed_action(id, reason);
            progress(id, Some(&action));
            action
        })
        .collect()
}

#[cfg(test)]
pub(super) fn execute_rule(
    rule: &RulePlan,
    source_scope: Option<&SourceScope>,
    dry_run: bool,
    operation: &OperationGuard,
) -> CleanupActionResult {
    execute_rule_with_exclusions(
        rule,
        source_scope,
        dry_run,
        operation,
        &CleanupExclusions::default(),
    )
}

pub(super) fn execute_rule_with_exclusions(
    rule: &RulePlan,
    source_scope: Option<&SourceScope>,
    dry_run: bool,
    operation: &OperationGuard,
    exclusions: &CleanupExclusions,
) -> CleanupActionResult {
    execute_rule_with_process_check(
        rule,
        source_scope,
        dry_run,
        operation,
        exclusions,
        &codex_worktrees::blocking_processes,
    )
}

// Only process discovery is injectable. Fixtures still exercise real Git
// validation, filesystem preflight, deletion, and result aggregation.
pub(super) fn execute_rule_with_process_check(
    rule: &RulePlan,
    source_scope: Option<&SourceScope>,
    dry_run: bool,
    operation: &OperationGuard,
    exclusions: &CleanupExclusions,
    process_check: &dyn Fn() -> Result<Vec<String>, String>,
) -> CleanupActionResult {
    if source_scope.is_some_and(|scope| {
        scope
            .validate_known_paths(
                rule.candidates
                    .iter()
                    .map(|candidate| candidate.path.as_path()),
            )
            .is_err()
    }) {
        return failed_action(&rule.source.id, CleanupActionReason::PreflightFailed);
    }
    let candidates = rule
        .candidates
        .iter()
        // Partial measurements remain visible in preview diagnostics but can
        // never enter dry-run or real execution, even if an adapter submits a
        // forged source selection.
        .filter(|candidate| !candidate.measurement_limited)
        .filter(|candidate| source_scope.is_none_or(|scope| scope.selects(&candidate.path)))
        .collect::<Vec<_>>();
    let bytes_expected = candidates.iter().map(|candidate| candidate.bytes).sum();
    if dry_run {
        return CleanupActionResult {
            rule_id: rule.source.id.clone(),
            action_kind: CleanupActionKind::Delete,
            status: CleanupActionStatus::Previewed,
            reason_code: None,
            bytes_expected,
            released_bytes: 0,
            affected_item_count: 0,
            failed_item_count: 0,
            running_processes: Vec::new(),
        };
    }

    let mut released_bytes = 0_u64;
    let mut affected_item_count = 0_u64;
    let mut failed_item_count = 0_u64;
    let mut blocked_item_count = 0_u64;
    let mut preflight_failed_count = 0_u64;
    let mut running_processes = Vec::<String>::new();
    for candidate in candidates {
        if operation.cancelled().load(Ordering::Relaxed) {
            failed_item_count = failed_item_count.saturating_add(1);
            break;
        }
        if exclusions.protects_tree(&candidate.path, &|| {
            operation
                .cancelled()
                .load(std::sync::atomic::Ordering::Relaxed)
        }) {
            failed_item_count = failed_item_count.saturating_add(1);
            preflight_failed_count = preflight_failed_count.saturating_add(1);
            log::warn!(
                "project_artifact_delete_skipped operation_id={} rule_id={} path={} reason=exclusionOverlap",
                operation.id(), rule.source.id, diagnostic_path(&candidate.path)
            );
            continue;
        }
        let prepared = match prepare_path_for_permanent_delete(&candidate.path) {
            Ok(prepared) => prepared,
            Err(error) => {
                failed_item_count = failed_item_count.saturating_add(1);
                log::warn!(
                    "project_artifact_delete_skipped rule_id={} path={} reason=identityCaptureFailed error={}",
                    rule.source.id,
                    diagnostic_path(&candidate.path),
                    mangodisk_platform::diagnostics::text(&error)
                );
                continue;
            }
        };
        if !project_matches(
            &candidate.project_root,
            &rule.source.project_match,
            exclusions,
        ) || validate_candidate(&candidate.project_root, &candidate.path).is_err()
        {
            failed_item_count = failed_item_count.saturating_add(1);
            log::warn!(
                "project_artifact_delete_skipped rule_id={} path={} reason=revalidationFailed",
                rule.source.id,
                diagnostic_path(&candidate.path)
            );
            continue;
        }
        // Revalidate provenance and writers before the final tree measurement. Keeping the
        // authored-entry inspection inside that last traversal minimizes the interval in which
        // a writer could create a protected file before permanent deletion.
        if let Some(checkout) = &candidate.codex_checkout {
            if !codex_worktrees::is_linked_checkout(checkout) {
                failed_item_count = failed_item_count.saturating_add(1);
                preflight_failed_count += 1;
                log::warn!(
                    "project_artifact_delete_skipped operation_id={} rule_id={} reason=codexCheckoutChanged",
                    operation.id(), rule.source.id
                );
                continue;
            }
            match process_check() {
                Ok(names) if names.is_empty() => {}
                Ok(names) => {
                    failed_item_count += 1;
                    blocked_item_count += 1;
                    for name in names {
                        if !running_processes
                            .iter()
                            .any(|existing| existing.eq_ignore_ascii_case(&name))
                        {
                            running_processes.push(name);
                        }
                    }
                    log::info!(
                        "project_artifact_delete_skipped operation_id={} rule_id={} reason=codexWritersRunning process_count={}",
                        operation.id(), rule.source.id, running_processes.len()
                    );
                    continue;
                }
                Err(error) => {
                    failed_item_count += 1;
                    preflight_failed_count += 1;
                    log::warn!(
                        "project_artifact_delete_skipped operation_id={} rule_id={} reason=codexProcessInspectionFailed error={}",
                        operation.id(), rule.source.id, mangodisk_platform::diagnostics::text(&error)
                    );
                    continue;
                }
            }
        }
        let live = measure_directory(&candidate.path, &|| {
            operation.cancelled().load(Ordering::Relaxed)
        });
        if let Err(error) = validate_artifact_protection(&candidate.path, &live, &|| {
            operation.cancelled().load(Ordering::Relaxed)
        }) {
            failed_item_count += 1;
            preflight_failed_count += 1;
            log::warn!(
                "project_artifact_delete_skipped operation_id={} rule_id={} path={} reason=authoredContentOrUnknown error={}",
                operation.id(), rule.source.id, diagnostic_path(&candidate.path),
                mangodisk_platform::diagnostics::text(&error)
            );
            continue;
        }
        if live.measured.skipped_count > 0 {
            failed_item_count = failed_item_count.saturating_add(1);
            log::warn!(
                "project_artifact_delete_skipped rule_id={} path={} reason=incompleteMeasurement skipped_count={}",
                rule.source.id,
                diagnostic_path(&candidate.path),
                live.measured.skipped_count
            );
            continue;
        }
        let deletion = if exclusions.has_names() {
            crate::filesystem::permanent_delete::delete_directory_tree_with_name_exclusions(
                prepared,
                live.measured.bytes,
                live.measured.file_count,
                exclusions.names(),
                &|| operation.cancelled().load(Ordering::Relaxed),
            )
            .map(|_| ())
        } else {
            delete_path_permanently(prepared, live.measured.bytes, live.measured.file_count)
        };
        match deletion {
            Ok(()) => {
                released_bytes = released_bytes.saturating_add(live.measured.bytes);
                affected_item_count = affected_item_count.saturating_add(live.measured.file_count);
            }
            Err(error) => {
                released_bytes = released_bytes.saturating_add(error.released_bytes());
                affected_item_count =
                    affected_item_count.saturating_add(error.affected_item_count());
                failed_item_count = failed_item_count.saturating_add(1);
                log::warn!(
                    "project_artifact_delete_failed rule_id={} path={} partial={} released_bytes={} affected_item_count={} error={}",
                    rule.source.id,
                    diagnostic_path(&candidate.path),
                    error.is_partial(),
                    error.released_bytes(),
                    error.affected_item_count(),
                    mangodisk_platform::diagnostics::text(&error)
                );
            }
        }
    }
    let cancelled = operation.cancelled().load(Ordering::Relaxed);
    let status = if failed_item_count == 0 {
        CleanupActionStatus::Completed
    } else if released_bytes > 0 || affected_item_count > 0 {
        CleanupActionStatus::Partial
    } else if !cancelled && blocked_item_count == failed_item_count {
        CleanupActionStatus::Blocked
    } else {
        CleanupActionStatus::Failed
    };
    log::info!(
        "project_artifact_execution_finished operation_id={} rule_id={} status={:?} released_bytes={} affected_item_count={} failed_item_count={} blocked_item_count={} preflight_failed_count={} process_count={}",
        operation.id(), rule.source.id, status, released_bytes, affected_item_count,
        failed_item_count, blocked_item_count, preflight_failed_count, running_processes.len()
    );
    CleanupActionResult {
        rule_id: rule.source.id.clone(),
        action_kind: CleanupActionKind::Delete,
        status,
        reason_code: if cancelled {
            Some(CleanupActionReason::Cancelled)
        } else if failed_item_count > 0 && blocked_item_count == failed_item_count {
            Some(CleanupActionReason::RunningProcesses)
        } else if failed_item_count > 0 && preflight_failed_count == failed_item_count {
            Some(CleanupActionReason::PreflightFailed)
        } else {
            (failed_item_count > 0).then_some(CleanupActionReason::ItemsSkipped)
        },
        bytes_expected,
        released_bytes,
        affected_item_count,
        failed_item_count,
        running_processes,
    }
}

fn failed_action(id: &str, reason: CleanupActionReason) -> CleanupActionResult {
    CleanupActionResult {
        rule_id: id.to_string(),
        action_kind: CleanupActionKind::Delete,
        status: CleanupActionStatus::Failed,
        reason_code: Some(reason),
        bytes_expected: 0,
        released_bytes: 0,
        affected_item_count: 0,
        failed_item_count: 1,
        running_processes: Vec::new(),
    }
}
