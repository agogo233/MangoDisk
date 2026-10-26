use super::super::project_artifact_schema::ProjectArtifactRuleSource;
use super::candidate_selection::{deduplicate_artifacts, path_identity_key};
use super::measurement::measure_artifacts;
use super::project_discovery::{
    collect_artifact_drafts, discover_projects, sort_and_deduplicate_project_matches,
};
use super::root_discovery::{
    automatic_codex_worktrees, automatic_project_roots, cached_project_matches,
    configured_codex_home, normalize_roots, retain_recent_project_matches, root_mode_name,
    update_project_root_index,
};
use super::root_discovery::{MAX_DISCOVERY_DEPTH, MAX_STANDARD_PROJECT_ROOTS};
use super::{
    ArtifactCandidate, CatalogPlan, ProjectDiscoveryRoots, ProjectDiscoveryScan,
    ProjectPlanRequest, ProjectRootMode, ProjectTraversalPolicy, RulePlan,
};
use crate::{
    cleanup::{codex_worktrees, exclusions::CleanupExclusions},
    filesystem::metadata::diagnostic_path,
};
use mangodisk_platform::{current_platform, Platform};
use std::{collections::HashMap, time::Instant};

pub(super) fn build_plan(
    configured_roots: &[String],
    deep_project_discovery: bool,
    rules: &[ProjectArtifactRuleSource],
    is_cancelled: &(dyn Fn() -> bool + Sync),
    exclusions: &CleanupExclusions,
) -> Result<CatalogPlan, String> {
    build_plan_with_progress(ProjectPlanRequest {
        configured_roots,
        deep_project_discovery,
        rules,
        is_cancelled,
        report_path: &|_| {},
        report_files: &|_, _, _| {},
        codex_home: configured_codex_home().as_deref(),
        exclusions,
    })
}

pub(super) fn build_plan_with_progress(
    request: ProjectPlanRequest<'_>,
) -> Result<CatalogPlan, String> {
    let ProjectPlanRequest {
        configured_roots,
        deep_project_discovery,
        rules,
        is_cancelled,
        report_path,
        report_files,
        codex_home,
        exclusions,
    } = request;
    let discovery_scan = ProjectDiscoveryScan {
        is_cancelled,
        report_path,
        report_files,
        exclusions,
    };
    let started = Instant::now();
    let mode = if deep_project_discovery && !configured_roots.is_empty() {
        ProjectRootMode::SelectedVolumes
    } else if !configured_roots.is_empty() {
        ProjectRootMode::Explicit
    } else if deep_project_discovery {
        ProjectRootMode::Deep
    } else {
        ProjectRootMode::Standard
    };
    let root_discovery_started = Instant::now();
    let mut roots = match mode {
        ProjectRootMode::Explicit => ProjectDiscoveryRoots {
            exact_roots: Vec::new(),
            recursive_roots: normalize_roots(configured_roots)?
                .into_iter()
                .filter(|root| !exclusions.matches(root))
                .collect(),
        },
        ProjectRootMode::Standard => {
            automatic_project_roots(rules, ProjectRootMode::Standard, &[], &discovery_scan)?
        }
        ProjectRootMode::Deep => {
            automatic_project_roots(rules, ProjectRootMode::Deep, &[], &discovery_scan)?
        }
        ProjectRootMode::SelectedVolumes => {
            let selected_volume_roots = normalize_roots(configured_roots)?;
            automatic_project_roots(
                rules,
                ProjectRootMode::SelectedVolumes,
                &selected_volume_roots,
                &discovery_scan,
            )?
        }
    };
    // The regular home discovery intentionally excludes hidden application
    // state. Add only positively identified linked checkouts, not `.codex`.
    // Explicit project requests keep their original, user-selected scope.
    let codex_roots = if mode != ProjectRootMode::Explicit
        && codex_home.is_some_and(|home| !exclusions.intersects(home))
    {
        let checkouts = automatic_codex_worktrees(codex_home, is_cancelled);
        roots.recursive_roots.extend(
            checkouts
                .iter()
                .filter(|root| !exclusions.matches(root))
                .cloned(),
        );
        checkouts
    } else {
        Vec::new()
    };
    let root_discovery_elapsed_ms = root_discovery_started.elapsed().as_millis();
    if roots.is_empty() {
        log::info!(
            "project_artifact_plan_built root_mode={} root_count=0 project_count=0 candidate_count=0 limited=false elapsed_ms={}",
            root_mode_name(mode),
            started.elapsed().as_millis()
        );
        return Ok(CatalogPlan {
            rules: rules
                .iter()
                .cloned()
                .map(|source| RulePlan {
                    source,
                    candidates: Vec::new(),
                })
                .collect(),
            read_failures: Default::default(),
            limited: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
    let automatic = mode != ProjectRootMode::Explicit;
    let project_discovery_started = Instant::now();
    // Native marker scans already identify the exact directory that owns a project marker.
    // Re-scanning an ancestor recursively discarded that information and repeated most directory
    // I/O. Exact roots only need rule validation in their own directory; recursive traversal is
    // reserved for explicit roots and roots where the platform fast path was unavailable.
    let (mut projects, exact_limited) = discover_projects(
        &roots.exact_roots,
        rules,
        ProjectTraversalPolicy {
            skip_hidden_directories: automatic,
            deep: mode != ProjectRootMode::Explicit,
            maximum_depth: 0,
            exclusions,
        },
        is_cancelled,
        report_path,
    )?;
    let (recursive_projects, recursive_limited) = discover_projects(
        &roots.recursive_roots,
        rules,
        ProjectTraversalPolicy {
            skip_hidden_directories: automatic,
            deep: mode != ProjectRootMode::Explicit,
            maximum_depth: MAX_DISCOVERY_DEPTH,
            exclusions,
        },
        is_cancelled,
        report_path,
    )?;
    projects.extend(recursive_projects);
    sort_and_deduplicate_project_matches(&mut projects);
    let discovery_limited = exact_limited || recursive_limited;
    let project_discovery_elapsed_ms = project_discovery_started.elapsed().as_millis();
    let cached_projects_started = Instant::now();
    if matches!(
        mode,
        ProjectRootMode::Standard | ProjectRootMode::SelectedVolumes
    ) {
        projects.extend(cached_project_matches(
            rules,
            is_cancelled,
            report_path,
            exclusions,
        )?);
        sort_and_deduplicate_project_matches(&mut projects);
    }
    let cached_projects_elapsed_ms = cached_projects_started.elapsed().as_millis();
    /*
     * Both automatic discovery and explicitly configured roots are validated
     * by Core before reaching this point. Persist every complete discovery so
     * specialized cleaners can protect project-owned runtimes during execute,
     * where adapter-supplied roots are intentionally discarded.
     */
    if !discovery_limited && !is_cancelled() {
        update_project_root_index(&projects);
    }
    if mode == ProjectRootMode::Standard {
        // Retain old verified Codex checkouts independently of the recent-
        // project cap: abandoned worktrees are useful cleanup candidates.
        let codex_projects = projects
            .iter()
            .filter(|project| {
                codex_roots.iter().any(|root| {
                    current_platform().path_is_same_or_child(&project.project_root, root)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        retain_recent_project_matches(&mut projects, MAX_STANDARD_PROJECT_ROOTS);
        projects.extend(codex_projects);
        sort_and_deduplicate_project_matches(&mut projects);
    }
    let draft_collection_started = Instant::now();
    let drafts = collect_artifact_drafts(&projects, rules, is_cancelled, report_path, exclusions);
    let draft_collection_elapsed_ms = draft_collection_started.elapsed().as_millis();
    let discovered_draft_count = drafts.len();
    let drafts = deduplicate_artifacts(drafts);
    let suppressed_draft_count = discovered_draft_count.saturating_sub(drafts.len());
    let measurement_started = Instant::now();
    let candidates = measure_artifacts(drafts, is_cancelled, report_path, report_files);
    let measurement_elapsed_ms = measurement_started.elapsed().as_millis();
    let mut candidates_by_rule = vec![Vec::new(); rules.len()];
    // Match the canonical project paths, including fixed macOS system aliases.
    // Failure keeps the lexical boundary rather than discarding protection.
    let codex_home = codex_home
        .filter(|home| !exclusions.intersects(home))
        .map(|home| {
            current_platform()
                .canonicalize_no_links(home)
                .unwrap_or_else(|_| home.to_path_buf())
        });
    let mut protected_projects = HashMap::new();
    let limited = discovery_limited;
    let mut read_failures = mangodisk_platform::FileReadFailures::default();
    for (draft, measured) in candidates {
        // Keep failures even when an unreadable artifact has no measurable bytes.
        read_failures.merge(&measured.measured.read_failures);
        let measurement_limited = measured.measured.skipped_count > 0;
        if measurement_limited {
            // One unreadable descendant must not hide a large, otherwise
            // measurable build directory. Preserve the accessible estimate,
            // but block only this candidate so complete sibling projects can
            // remain available through the same declarative rule.
            log::warn!(
                "project_artifact_measurement_incomplete rule_id={} path={} skipped_count={} read_failure_count={} permission_denied_count={} privacy_restriction_possible_count={}",
                rules[draft.rule_index].id,
                diagnostic_path(&draft.path),
                measured.measured.skipped_count,
                measured.measured.read_failures.count,
                measured.measured.read_failures.permission_denied_count,
                measured.measured.read_failures.privacy_restricted_count
            );
        }
        if measured.measured.bytes == 0
            && measured.measured.file_count == 0
            && measured.measured.read_failures.count == 0
        {
            continue;
        }
        candidates_by_rule[draft.rule_index].push(ArtifactCandidate {
            // Discovery provenance is a fast path, not the safety boundary.
            // Native/index results need protection even when Codex discovery
            // failed. Cache classification per project across artifact rules.
            codex_checkout: protected_projects
                .entry(draft.project_root.clone())
                .or_insert_with(|| {
                    codex_roots
                        .iter()
                        .find(|root| {
                            current_platform().path_is_same_or_child(&draft.project_root, root)
                        })
                        .cloned()
                        .or_else(|| {
                            codex_worktrees::protected_checkout(
                                &draft.project_root,
                                codex_home.as_deref(),
                            )
                        })
                })
                .clone(),
            project_root: draft.project_root,
            modified_at_ms: measured.modified_at_ms,
            path: draft.path,
            bytes: measured.measured.bytes,
            file_count: measured.measured.file_count,
            measurement_limited,
        });
    }
    let plans = rules
        .iter()
        .cloned()
        .zip(candidates_by_rule)
        .map(|(source, mut candidates)| {
            candidates.sort_by_cached_key(|candidate| path_identity_key(&candidate.path));
            RulePlan { source, candidates }
        })
        .collect::<Vec<_>>();
    log::info!(
        "project_artifact_plan_built root_mode={} root_count={} project_count={} candidate_count={} suppressed_candidate_count={} limited={} root_discovery_elapsed_ms={} project_discovery_elapsed_ms={} cached_projects_elapsed_ms={} draft_collection_elapsed_ms={} measurement_elapsed_ms={} elapsed_ms={}",
        root_mode_name(mode),
        roots.root_count(),
        projects.len(),
        plans.iter().map(|rule| rule.candidates.len()).sum::<usize>(),
        suppressed_draft_count,
        limited,
        root_discovery_elapsed_ms,
        project_discovery_elapsed_ms,
        cached_projects_elapsed_ms,
        draft_collection_elapsed_ms,
        measurement_elapsed_ms,
        started.elapsed().as_millis()
    );
    Ok(CatalogPlan {
        rules: plans,
        read_failures,
        limited: limited || is_cancelled(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}
