//! Project-artifact cleanup orchestration. Discovery, measurement, planning, and execution
//! remain private to this cleaner; platform queries only supply filesystem facts.

use self::planning::build_plan_with_progress;
use self::root_discovery::configured_codex_home;
use self::scan_result::{apply_codex_process_guard, cleanup_source_details, scan_result};
use super::project_artifact_schema::{parse_catalog, ProjectArtifactRuleSource};
use crate::{
    cleanup::{
        codex_worktrees, exclusions::CleanupExclusions, measurement::MeasureResult,
        source_selection::SourceSelectionPolicy, CleanupSourceDetail, ScanItemStatus,
        ScanRuleResult,
    },
    shared::operation::OperationGuard,
};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
include!(concat!(
    env!("OUT_DIR"),
    "/embedded-project-artifact-rules.rs"
));
static CURRENT_PLATFORM_RULES: OnceLock<Result<Vec<ProjectArtifactRuleSource>, String>> =
    OnceLock::new();
mod candidate_selection;
mod execution;
mod measurement;
mod planning;
mod project_discovery;
mod root_discovery;
mod scan_result;
pub(super) use execution::execute_selected_with_progress;

#[derive(Debug, Clone)]
struct ProjectMatch {
    rule_index: usize,
    project_root: PathBuf,
    allow_descendant_scan: bool,
}

#[derive(Debug, Clone)]
struct ArtifactDraft {
    rule_index: usize,
    project_root: PathBuf,
    path: PathBuf,
}

#[derive(Debug, Clone)]
struct ArtifactCandidate {
    project_root: PathBuf,
    codex_checkout: Option<PathBuf>,
    path: PathBuf,
    bytes: u64,
    file_count: u64,
    modified_at_ms: Option<u64>,
    measurement_limited: bool,
}

#[derive(Debug)]
struct RulePlan {
    source: ProjectArtifactRuleSource,
    candidates: Vec<ArtifactCandidate>,
}

#[derive(Debug)]
struct CatalogPlan {
    rules: Vec<RulePlan>,
    read_failures: mangodisk_platform::FileReadFailures,
    limited: bool,
    elapsed_ms: u64,
}

#[derive(Debug, Default)]
struct CleanupSourceSummary {
    sources: Vec<CleanupSourceDetail>,
    source_count: u64,
}

#[derive(Default)]
struct ArtifactMeasurement {
    measured: MeasureResult,
    modified_at_ms: Option<u64>,
    authored_entry: Option<String>,
}

#[derive(Debug)]
struct DirectoryEntries {
    file_names: Vec<String>,
    directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectRootMode {
    Explicit,
    Standard,
    Deep,
    SelectedVolumes,
}

#[derive(Debug, Default)]
struct ProjectDiscoveryRoots {
    exact_roots: Vec<PathBuf>,
    recursive_roots: Vec<PathBuf>,
}

impl ProjectDiscoveryRoots {
    #[cfg(not(test))]
    fn extend(&mut self, other: Self) {
        self.exact_roots.extend(other.exact_roots);
        self.recursive_roots.extend(other.recursive_roots);
    }

    fn root_count(&self) -> usize {
        self.exact_roots.len() + self.recursive_roots.len()
    }

    fn is_empty(&self) -> bool {
        self.exact_roots.is_empty() && self.recursive_roots.is_empty()
    }
}

struct ProjectPlanRequest<'a> {
    configured_roots: &'a [String],
    deep_project_discovery: bool,
    rules: &'a [ProjectArtifactRuleSource],
    is_cancelled: &'a (dyn Fn() -> bool + Sync),
    report_path: &'a (dyn Fn(&Path) + Sync),
    report_files: &'a (dyn Fn(&Path, u64, u64) + Sync),
    codex_home: Option<&'a Path>,
    exclusions: &'a CleanupExclusions,
}

struct ProjectDiscoveryScan<'a> {
    is_cancelled: &'a (dyn Fn() -> bool + Sync),
    report_path: &'a (dyn Fn(&Path) + Sync),
    report_files: &'a (dyn Fn(&Path, u64, u64) + Sync),
    exclusions: &'a CleanupExclusions,
}

struct ProjectTraversalPolicy<'a> {
    skip_hidden_directories: bool,
    deep: bool,
    maximum_depth: usize,
    exclusions: &'a CleanupExclusions,
}

pub(super) struct ProjectExecutionRequest<'a> {
    pub(super) selected_ids: &'a [String],
    pub(super) configured_roots: &'a [String],
    pub(super) selected_volume_scope: bool,
    pub(super) source_selections: &'a SourceSelectionPolicy,
    pub(super) dry_run: bool,
    pub(super) operation: &'a OperationGuard,
    pub(super) exclusions: &'a CleanupExclusions,
}

pub(super) fn preview_all(
    configured_roots: &[String],
    deep_project_discovery: bool,
    is_cancelled: &(dyn Fn() -> bool + Sync),
    report_path: &(dyn Fn(&Path) + Sync),
    report_files: &(dyn Fn(&Path, u64, u64) + Sync),
    exclusions: &CleanupExclusions,
) -> super::CleanerScanPreview {
    let rules = match current_platform_rules() {
        Ok(rules) => rules,
        Err(error) => {
            log::error!(
                "project_artifact_catalog_load_failed error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            return super::CleanerScanPreview::default();
        }
    };
    match build_plan_with_progress(ProjectPlanRequest {
        configured_roots,
        deep_project_discovery,
        rules,
        is_cancelled,
        report_path,
        report_files,
        codex_home: configured_codex_home().as_deref(),
        exclusions,
    }) {
        Ok(plan) => {
            let processes = plan
                .rules
                .iter()
                .flat_map(|rule| &rule.candidates)
                .any(|candidate| candidate.codex_checkout.is_some())
                .then(codex_worktrees::blocking_processes);
            let results = plan
                .rules
                .iter()
                .map(|rule| {
                    let complete_bytes: u64 = rule
                        .candidates
                        .iter()
                        .filter(|candidate| !candidate.measurement_limited)
                        .map(|candidate| candidate.bytes)
                        .sum();
                    let complete_file_count: u64 = rule
                        .candidates
                        .iter()
                        .filter(|candidate| !candidate.measurement_limited)
                        .map(|candidate| candidate.file_count)
                        .sum();
                    let limited_bytes: u64 = rule
                        .candidates
                        .iter()
                        .filter(|candidate| candidate.measurement_limited)
                        .map(|candidate| candidate.bytes)
                        .sum();
                    let limited_file_count: u64 = rule
                        .candidates
                        .iter()
                        .filter(|candidate| candidate.measurement_limited)
                        .map(|candidate| candidate.file_count)
                        .sum();
                    let status = if plan.limited {
                        ScanItemStatus::Limited
                    } else if complete_bytes > 0 {
                        ScanItemStatus::Found
                    } else if rule
                        .candidates
                        .iter()
                        .any(|candidate| candidate.measurement_limited)
                    {
                        ScanItemStatus::Limited
                    } else {
                        ScanItemStatus::Clean
                    };
                    // A mixed rule exposes only fully measured bytes as
                    // reclaimable. If every candidate is limited, preserve the
                    // accessible estimate for the inspection report.
                    let (bytes, file_count) = if status == ScanItemStatus::Found {
                        (complete_bytes, complete_file_count)
                    } else {
                        (
                            complete_bytes.saturating_add(limited_bytes),
                            complete_file_count.saturating_add(limited_file_count),
                        )
                    };
                    let mut result = scan_result(
                        &rule.source,
                        status,
                        bytes,
                        file_count,
                        plan.elapsed_ms / plan.rules.len().max(1) as u64,
                        cleanup_source_details(&rule.candidates),
                    );
                    apply_codex_process_guard(&mut result, &rule.candidates, processes.as_ref());
                    result
                })
                .collect();
            super::CleanerScanPreview {
                rules: results,
                read_failures: plan.read_failures,
            }
        }
        Err(error) => {
            log::warn!(
                "project_artifact_preview_failed error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
            let results = rules
                .iter()
                .map(|rule| {
                    scan_result(
                        rule,
                        ScanItemStatus::Limited,
                        0,
                        0,
                        0,
                        CleanupSourceSummary::default(),
                    )
                })
                .collect();
            super::CleanerScanPreview {
                rules: results,
                ..Default::default()
            }
        }
    }
}

pub(super) fn preview_limited_all() -> Vec<ScanRuleResult> {
    current_platform_rules()
        .unwrap_or_default()
        .iter()
        .map(|rule| {
            scan_result(
                rule,
                ScanItemStatus::Limited,
                0,
                0,
                0,
                CleanupSourceSummary::default(),
            )
        })
        .collect()
}

pub(super) fn contains(id: &str) -> bool {
    current_platform_rules().is_ok_and(|rules| rules.iter().any(|rule| rule.id == id))
}

pub(super) fn count() -> usize {
    current_platform_rules().map_or(0, <[_]>::len)
}

pub(super) fn catalog_digest() -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"mangodisk-project-artifact-catalog-v4-authored-content");
    for (name, source) in EMBEDDED_PROJECT_ARTIFACT_RULE_SOURCES {
        hasher.update(name.as_bytes());
        hasher.update(source.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

pub(super) fn current_platform_rules() -> Result<&'static [ProjectArtifactRuleSource], String> {
    match CURRENT_PLATFORM_RULES.get_or_init(|| {
        parse_catalog(EMBEDDED_PROJECT_ARTIFACT_RULE_SOURCES).map(|rules| {
            rules
                .into_iter()
                .filter(|rule| {
                    rule.platforms
                        .iter()
                        .any(|platform| platform == current_platform_name())
                })
                .collect()
        })
    }) {
        Ok(rules) => Ok(rules),
        Err(error) => Err(error.clone()),
    }
}

#[cfg(target_os = "macos")]
const fn current_platform_name() -> &'static str {
    "macos"
}

#[cfg(windows)]
const fn current_platform_name() -> &'static str {
    "windows"
}

#[cfg(target_os = "linux")]
const fn current_platform_name() -> &'static str {
    "linux"
}

#[cfg(test)]
#[path = "project_artifacts_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "project_artifacts_execution_tests.rs"]
mod execution_tests;
