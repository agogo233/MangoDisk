use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntryInfo {
    pub name: String,
    pub path: String,
    /// Physical storage charged to the volume and shown by disk analysis.
    pub bytes: u64,
    /// Logical content length retained for delete preflight and cache updates.
    #[serde(skip)]
    pub(crate) logical_bytes: u64,
    pub file_count: u64,
    pub is_directory: bool,
    pub modified_at_ms: Option<u64>,
    pub content_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub scan_id: u64,
    pub root: String,
    pub scanned_at_ms: u64,
    /// Physical storage charged to all direct result entries.
    pub total_bytes: u64,
    pub skipped_count: u64,
    pub entries: Vec<DirectoryEntryInfo>,
}

/// Captures an entry from an authoritative analysis snapshot.
#[derive(Debug, Clone)]
pub(crate) struct AnalysisEntryCandidate {
    pub(crate) root: String,
    pub(crate) path: String,
    pub(crate) expected_logical_bytes: u64,
    pub(crate) expected_allocated_bytes: u64,
    pub(crate) expected_file_count: u64,
    pub(crate) is_directory: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisDeleteResult {
    pub removed_path: String,
    /// Physical storage released by the removed entry.
    pub released_bytes: u64,
    pub removed_file_count: u64,
}
