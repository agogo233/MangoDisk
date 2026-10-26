use super::NativePrivacyCandidate;
use crate::history::{
    HistoryService, OperationCategory, OperationDetails, OperationOutcome, OperationRecord,
    PrivacyCleanupHistoryItem, PrivacyCleanupHistoryItemStatus, PrivacyCleanupOperationDetails,
    OPERATION_RECORD_SCHEMA_VERSION,
};
use crate::privacy::{PrivacyExecutionItemResult, PrivacyExecutionItemStatus, PrivacyTimeRange};

pub(super) fn append_history_record(
    operation_id: u64,
    plan_id: String,
    time_range: PrivacyTimeRange,
    candidates: &[NativePrivacyCandidate],
    results: &[PrivacyExecutionItemResult],
    started_at_ms: u64,
    finished_at_ms: u64,
) -> bool {
    let items = candidates
        .iter()
        .zip(results)
        .map(|(candidate, result)| PrivacyCleanupHistoryItem {
            source_id: candidate.item.source_id.clone(),
            kind: candidate.item.kind,
            affected_item_count: result.affected_item_count,
            status: match result.status {
                PrivacyExecutionItemStatus::Cleared => PrivacyCleanupHistoryItemStatus::Cleared,
                PrivacyExecutionItemStatus::Unchanged => PrivacyCleanupHistoryItemStatus::Unchanged,
                PrivacyExecutionItemStatus::Failed => PrivacyCleanupHistoryItemStatus::Failed,
                PrivacyExecutionItemStatus::Cancelled => PrivacyCleanupHistoryItemStatus::Cancelled,
            },
            failure_reason: result.failure_reason.clone(),
        })
        .collect::<Vec<_>>();
    let failed_item_count = items
        .iter()
        .filter(|item| item.status == PrivacyCleanupHistoryItemStatus::Failed)
        .count() as u64;
    let record = OperationRecord {
        schema_version: OPERATION_RECORD_SCHEMA_VERSION,
        operation_id: format!("privacy-{plan_id}"),
        category: OperationCategory::PrivacyCleanup,
        started_at_ms,
        finished_at_ms,
        outcome: privacy_history_outcome(&items),
        dry_run: false,
        selected_item_count: items.len() as u64,
        affected_item_count: items.iter().map(|item| item.affected_item_count).sum(),
        expected_bytes: 0,
        released_bytes: None,
        released_bytes_is_estimate: false,
        failed_item_count,
        details: OperationDetails::PrivacyCleanup(PrivacyCleanupOperationDetails {
            plan_id,
            time_range,
            items,
        }),
    };
    let record_operation_id = record.operation_id.clone();
    if let Err(error) = HistoryService::append(record) {
        log::warn!(
            "privacy_history_save_failed operation_id={} history_operation_id={} candidate_count={} result_count={} outcome=not_saved error={}",
            operation_id,
            record_operation_id,
            candidates.len(),
            results.len(),
            mangodisk_platform::diagnostics::text(&error)
        );
        false
    } else {
        true
    }
}

pub(super) fn privacy_history_outcome(items: &[PrivacyCleanupHistoryItem]) -> OperationOutcome {
    if items
        .iter()
        .all(|item| item.status == PrivacyCleanupHistoryItemStatus::Cancelled)
    {
        OperationOutcome::Cancelled
    } else if items.iter().any(|item| {
        matches!(
            item.status,
            PrivacyCleanupHistoryItemStatus::Failed | PrivacyCleanupHistoryItemStatus::Cancelled
        )
    }) {
        OperationOutcome::CompletedWithWarnings
    } else {
        OperationOutcome::Completed
    }
}
