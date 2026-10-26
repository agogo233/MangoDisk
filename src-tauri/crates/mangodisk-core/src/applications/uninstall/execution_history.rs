use super::models::{
    ApplicationUninstallActionStatus, ApplicationUninstallBatchPlan, ApplicationUninstallCandidate,
    ApplicationUninstallPlan, ApplicationUninstallResult,
};
use crate::history::{
    ApplicationUninstallApplicationDetails, ApplicationUninstallOperationDetails, HistoryService,
    OperationCategory, OperationDetails, OperationOutcome, OperationRecord,
    OPERATION_RECORD_SCHEMA_VERSION,
};

pub(super) fn save_execution_history(
    operation_id: u64,
    started_at_ms: u64,
    finished_at_ms: u64,
    plan: &ApplicationUninstallPlan,
    result: &ApplicationUninstallResult,
    application: Option<&ApplicationUninstallCandidate>,
) -> bool {
    let Some(application) = application else {
        log::warn!("application_uninstall_history_identity_missing operation_id={operation_id}");
        return false;
    };
    execution_history_record(UninstallHistoryRecordInput {
        operation_id,
        started_at_ms,
        finished_at_ms,
        batch_id: &plan.plan_id,
        expected_bytes: plan.expected_bytes,
        plans: std::slice::from_ref(plan),
        results: std::slice::from_ref(result),
        candidates: std::slice::from_ref(application),
    })
    .is_some_and(|record| append_uninstall_history(operation_id, record))
}

pub(super) fn save_batch_execution_history(
    operation_id: u64,
    started_at_ms: u64,
    finished_at_ms: u64,
    plan: &ApplicationUninstallBatchPlan,
    results: &[ApplicationUninstallResult],
    candidates: &[ApplicationUninstallCandidate],
) -> bool {
    execution_history_record(UninstallHistoryRecordInput {
        operation_id,
        started_at_ms,
        finished_at_ms,
        batch_id: &plan.batch_id,
        expected_bytes: plan.expected_bytes,
        plans: &plan.plans,
        results,
        candidates,
    })
    .is_some_and(|record| append_uninstall_history(operation_id, record))
}

pub(super) struct UninstallHistoryRecordInput<'a> {
    pub(super) operation_id: u64,
    pub(super) started_at_ms: u64,
    pub(super) finished_at_ms: u64,
    pub(super) batch_id: &'a str,
    pub(super) expected_bytes: u64,
    pub(super) plans: &'a [ApplicationUninstallPlan],
    pub(super) results: &'a [ApplicationUninstallResult],
    pub(super) candidates: &'a [ApplicationUninstallCandidate],
}

pub(super) fn execution_history_record(
    input: UninstallHistoryRecordInput<'_>,
) -> Option<OperationRecord> {
    let UninstallHistoryRecordInput {
        operation_id,
        started_at_ms,
        finished_at_ms,
        batch_id,
        expected_bytes,
        plans,
        results,
        candidates,
    } = input;
    if plans.len() != results.len() {
        log::warn!(
            "application_uninstall_history_result_count_mismatch operation_id={} plan_count={} result_count={}",
            operation_id,
            plans.len(),
            results.len()
        );
        return None;
    }
    let applications = plans
        .iter()
        .zip(results)
        .map(|(application_plan, result)| {
            candidates
                .iter()
                .find(|candidate| candidate.application_id == application_plan.application_id)
                .map(|candidate| application_history_details(application_plan, result, candidate))
        })
        .collect::<Option<Vec<_>>>();
    let Some(applications) = applications.filter(|applications| !applications.is_empty()) else {
        log::warn!("application_uninstall_history_identity_missing operation_id={operation_id}");
        return None;
    };
    let released_bytes_is_estimate = results
        .iter()
        .any(|result| result.released_bytes_is_estimate);
    let affected_application_count = results
        .iter()
        .filter(|result| result.affected_item_count > 0)
        .count() as u64;
    let failed_application_count = results
        .iter()
        .filter(|result| result.failed_item_count > 0)
        .count() as u64;
    let released_bytes = results.iter().fold(0_u64, |total, result| {
        total.saturating_add(result.released_bytes)
    });
    let restart_required = results.iter().any(|result| result.restart_required);
    let record = OperationRecord {
        schema_version: OPERATION_RECORD_SCHEMA_VERSION,
        operation_id: format!("application-uninstall-{operation_id}-{finished_at_ms}"),
        category: OperationCategory::ApplicationUninstall,
        started_at_ms,
        finished_at_ms,
        outcome: if results.iter().any(|result| {
            result
                .actions
                .iter()
                .any(|action| action.status == ApplicationUninstallActionStatus::Cancelled)
        }) {
            OperationOutcome::Cancelled
        } else if failed_application_count == 0 {
            OperationOutcome::Completed
        } else {
            OperationOutcome::CompletedWithWarnings
        },
        dry_run: false,
        selected_item_count: applications.len() as u64,
        affected_item_count: affected_application_count,
        expected_bytes,
        released_bytes: (!released_bytes_is_estimate).then_some(released_bytes),
        released_bytes_is_estimate,
        failed_item_count: failed_application_count,
        details: OperationDetails::ApplicationUninstall(application_uninstall_details(
            batch_id.to_string(),
            restart_required,
            applications,
        )),
    };
    Some(record)
}

fn application_history_details(
    plan: &ApplicationUninstallPlan,
    result: &ApplicationUninstallResult,
    application: &ApplicationUninstallCandidate,
) -> ApplicationUninstallApplicationDetails {
    ApplicationUninstallApplicationDetails {
        restart_required: result.restart_required,
        plan_id: plan.plan_id.clone(),
        application_id: plan.application_id.clone(),
        application_name: application.name.clone(),
        application_identifier: application.primary_identifier.clone(),
        application_version: application.version.clone(),
        application_publisher: application.publisher.clone(),
        application_platform: application.platform,
        installer_kind: application.installer_kind,
        component_ids: plan
            .items
            .iter()
            .map(|item| item.component_id.clone())
            .collect(),
        actions: result.actions.clone(),
    }
}

fn application_uninstall_details(
    batch_id: String,
    restart_required: bool,
    applications: Vec<ApplicationUninstallApplicationDetails>,
) -> ApplicationUninstallOperationDetails {
    ApplicationUninstallOperationDetails {
        batch_id,
        applications,
        restart_required,
    }
}

fn append_uninstall_history(operation_id: u64, record: OperationRecord) -> bool {
    match HistoryService::append(record) {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "application_uninstall_history_save_failed operation_id={} error={}",
                operation_id,
                mangodisk_platform::diagnostics::text(&error)
            );
            false
        }
    }
}
