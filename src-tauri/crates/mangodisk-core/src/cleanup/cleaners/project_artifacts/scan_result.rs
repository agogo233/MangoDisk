use super::super::project_artifact_schema::ProjectArtifactRuleSource;
use super::{ArtifactCandidate, CleanupSourceSummary};
use crate::{
    cleanup::{CleanupSourceDetail, RiskLevel, ScanItemStatus, ScanRuleResult},
    filesystem::metadata::display_path,
};
use std::collections::HashSet;

pub(super) fn scan_result(
    rule: &ProjectArtifactRuleSource,
    status: ScanItemStatus,
    bytes: u64,
    file_count: u64,
    elapsed_ms: u64,
    source_summary: CleanupSourceSummary,
) -> ScanRuleResult {
    let available = status != ScanItemStatus::NotApplicable;
    ScanRuleResult {
        rule_id: rule.id.clone(),
        category: crate::cleanup::CleanupCategory::Project,
        group: crate::cleanup::CleanupGroup::Project,
        risk: RiskLevel::Recoverable,
        default_selected: false,
        recommended_selected: false,
        bytes,
        file_count,
        available,
        selectable: status == ScanItemStatus::Found && bytes > 0,
        status,
        running_processes: Vec::new(),
        requires_app_close: false,
        sources: source_summary.sources,
        source_count: source_summary.source_count,
        sources_truncated: false,
        scan_elapsed_ms: elapsed_ms,
    }
}

pub(super) fn apply_codex_process_guard(
    result: &mut ScanRuleResult,
    candidates: &[ArtifactCandidate],
    processes: Option<&Result<Vec<String>, String>>,
) {
    let paths = candidates
        .iter()
        .filter(|candidate| candidate.codex_checkout.is_some())
        .map(|candidate| display_path(&candidate.path))
        .collect::<HashSet<_>>();
    if paths.is_empty() {
        return;
    }
    let (reason, status) = match processes {
        Some(Ok(names)) if names.is_empty() => return,
        Some(Ok(names)) => {
            result.running_processes = names.clone();
            result.requires_app_close = true;
            (
                crate::cleanup::CleanupSourceBlockReason::RequiresClose,
                ScanItemStatus::RequiresClose,
            )
        }
        Some(Err(_)) | None => {
            log::warn!("codex_worktree_process_check_failed");
            (
                crate::cleanup::CleanupSourceBlockReason::IncompleteMeasurement,
                ScanItemStatus::Limited,
            )
        }
    };
    for source in &mut result.sources {
        if paths.contains(&source.path) && source.block_reason.is_none() {
            source.block_reason = Some(reason);
        }
    }
    // Known writers are a pre-cleanup warning, not a selection restriction.
    // Execution still checks live processes before deleting each candidate.
    // Unknown process state and incomplete measurements remain hard blocks.
    let is_selectable = |source: &&CleanupSourceDetail| {
        source.block_reason.is_none()
            || source.block_reason == Some(crate::cleanup::CleanupSourceBlockReason::RequiresClose)
    };
    if result.sources.iter().filter(is_selectable).count() == 0 {
        result.selectable = false;
        if result.status != ScanItemStatus::Limited {
            result.status = status;
        }
    } else if result.selectable {
        // Keep hard-blocked source estimates for inspection without including
        // them in selectable totals. The source inventory is complete here.
        result.bytes = result
            .sources
            .iter()
            .filter(is_selectable)
            .map(|source| source.bytes)
            .sum();
        result.file_count = result
            .sources
            .iter()
            .filter(is_selectable)
            .map(|source| source.file_count)
            .sum();
    }
    log::info!(
        "codex_worktree_sources_guarded rule_id={} source_count={} process_count={} selectable_bytes={}",
        result.rule_id,
        paths.len(),
        result.running_processes.len(),
        if result.selectable { result.bytes } else { 0 }
    );
}

pub(super) fn cleanup_source_details(candidates: &[ArtifactCandidate]) -> CleanupSourceSummary {
    let source_count = candidates.len() as u64;
    let mut sources = candidates
        .iter()
        .map(|candidate| CleanupSourceDetail {
            path: display_path(&candidate.path),
            bytes: candidate.bytes,
            file_count: candidate.file_count,
            modified_at_ms: candidate.modified_at_ms,
            block_reason: candidate
                .measurement_limited
                .then_some(crate::cleanup::CleanupSourceBlockReason::IncompleteMeasurement),
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    CleanupSourceSummary {
        sources,
        source_count,
    }
}
