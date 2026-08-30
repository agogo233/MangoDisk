use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard, OnceLock,
    },
};

use super::CustomCleanupRule;

const CUSTOM_CLEANUP_SESSION_LIMIT: usize = 8;
static NEXT_CUSTOM_CLEANUP_SCAN_ID: AtomicU64 = AtomicU64::new(1);
static CUSTOM_CLEANUP_SESSIONS: OnceLock<Mutex<VecDeque<CustomCleanupSession>>> = OnceLock::new();

struct CustomCleanupSession {
    scan_id: u64,
    rules: Vec<CustomCleanupRule>,
    include_standard_rules: bool,
}

fn sessions() -> &'static Mutex<VecDeque<CustomCleanupSession>> {
    CUSTOM_CLEANUP_SESSIONS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn lock_sessions() -> Result<MutexGuard<'static, VecDeque<CustomCleanupSession>>, String> {
    sessions()
        .lock()
        .map_err(|_| "the custom cleanup result session is unavailable".to_string())
}

pub(super) fn publish(
    rules: Vec<CustomCleanupRule>,
    include_standard_rules: bool,
) -> Result<u64, String> {
    let scan_id = NEXT_CUSTOM_CLEANUP_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    let rule_count = rules.len();
    let mut sessions = lock_sessions()?;
    sessions.push_front(CustomCleanupSession {
        scan_id,
        rules,
        include_standard_rules,
    });
    sessions.truncate(CUSTOM_CLEANUP_SESSION_LIMIT);
    log::info!(
        "custom_cleanup_session_published scan_id={scan_id} rule_count={rule_count} include_standard_rules={include_standard_rules}"
    );
    Ok(scan_id)
}

/// Resolves the exact rules retained by Core after a successful custom scan.
///
/// The WebView can retain names for presentation, but it cannot substitute a
/// different directory or matcher when cleanup begins.
pub(super) fn resolve(
    scan_id: u64,
    requested_rules: &[CustomCleanupRule],
    include_standard_rules: bool,
) -> Result<Vec<CustomCleanupRule>, String> {
    let sessions = lock_sessions()?;
    let Some(session) = sessions.iter().find(|session| session.scan_id == scan_id) else {
        log::warn!("custom_cleanup_session_resolution_failed scan_id={scan_id} reason=notFound");
        return Err("the custom cleanup result expired; scan again".to_string());
    };
    if session.rules != requested_rules {
        log::warn!(
            "custom_cleanup_session_resolution_failed scan_id={} reason=rulesChanged expected_rule_count={} requested_rule_count={}",
            scan_id,
            session.rules.len(),
            requested_rules.len()
        );
        return Err("the custom cleanup rules no longer match the scan result".to_string());
    }
    if session.include_standard_rules != include_standard_rules {
        log::warn!(
            "custom_cleanup_session_resolution_failed scan_id={scan_id} reason=scopeChanged expected_include_standard_rules={} requested_include_standard_rules={include_standard_rules}",
            session.include_standard_rules
        );
        return Err("the custom cleanup scope no longer matches the scan result".to_string());
    }
    Ok(session.rules.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cleanup::{CustomCleanupModifiedTime, CUSTOM_CLEANUP_RULE_SCHEMA_VERSION};

    fn rule(root: &str) -> CustomCleanupRule {
        CustomCleanupRule {
            schema_version: CUSTOM_CLEANUP_RULE_SCHEMA_VERSION,
            id: "fixture-rule".to_string(),
            name: "Fixture files".to_string(),
            roots: vec![root.to_string()],
            name_patterns: vec!["*.tmp".to_string()],
            minimum_bytes: None,
            maximum_bytes: None,
            modified_time: CustomCleanupModifiedTime::Any,
            recursive: true,
        }
    }

    #[test]
    fn execution_rules_must_match_the_authoritative_scan_session() {
        let rules = vec![rule("/fixture")];
        let scan_id = publish(rules.clone(), false).expect("publish the custom cleanup session");

        assert_eq!(
            resolve(scan_id, &rules, false).expect("resolve the matching custom cleanup session"),
            rules
        );
        assert!(resolve(scan_id, &[rule("/different")], false).is_err());
        assert!(resolve(scan_id, &rules, true).is_err());
        assert!(resolve(u64::MAX, &rules, false).is_err());
    }
}
