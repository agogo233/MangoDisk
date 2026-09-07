use serde::{Deserialize, Serialize};

const LARGE_FILE_RESULT_LIMIT: usize = 2_000;

/// Every scan retains candidates from this fixed floor. UI threshold changes therefore remain a
/// deterministic view over the active scan and never need to touch the filesystem again.
pub(crate) const LARGE_FILE_CANDIDATE_FLOOR_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LargeFileScanMode {
    /// Uses the operating system's metadata index and may omit files that have not been indexed.
    Quick,
    /// Enumerates the selected filesystem scope and is authoritative for reachable files.
    Complete,
}

impl LargeFileScanMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileEntry {
    pub name: String,
    pub path: String,
    pub parent_path: String,
    /// Physical storage charged to the volume and shown in cleanup estimates.
    pub bytes: u64,
    /// Logical content length retained for delete preflight.
    #[serde(skip)]
    pub(crate) logical_bytes: u64,
    pub modified_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFilesResult {
    pub scan_id: u64,
    pub root: String,
    /// Full candidate set retained by the Core session and never serialized to the WebView.
    ///
    /// Keeping this snapshot in the large-file domain avoids replacing the independent disk
    /// analysis cache when the user switches scan modes or thresholds.
    #[serde(skip)]
    pub(crate) retained_entries: Vec<LargeFileEntry>,
    pub scanned_at_ms: u64,
    pub scan_mode: LargeFileScanMode,
    /// Physical-size threshold used by large-file discovery.
    pub minimum_bytes: u64,
    /// Physical storage charged to all returned large files.
    pub total_bytes: u64,
    pub total_count: u64,
    pub returned_count: u64,
    pub truncated: bool,
    pub skipped_count: u64,
    pub entries: Vec<LargeFileEntry>,
}

impl LargeFilesResult {
    pub(crate) fn from_retained_entries(
        root: String,
        scanned_at_ms: u64,
        scan_mode: LargeFileScanMode,
        minimum_bytes: u64,
        skipped_count: u64,
        retained_entries: Vec<LargeFileEntry>,
    ) -> Self {
        let minimum_bytes = minimum_bytes.max(LARGE_FILE_CANDIDATE_FLOOR_BYTES);
        let mut entries = retained_entries
            .iter()
            .filter(|entry| entry.bytes >= minimum_bytes)
            .cloned()
            .collect::<Vec<_>>();
        let total_count = entries.len() as u64;
        let total_bytes = entries.iter().map(|entry| entry.bytes).sum();
        entries.truncate(LARGE_FILE_RESULT_LIMIT);
        let returned_count = entries.len() as u64;

        Self {
            scan_id: 0,
            root,
            retained_entries,
            scanned_at_ms,
            scan_mode,
            minimum_bytes,
            total_bytes,
            total_count,
            returned_count,
            truncated: returned_count < total_count,
            skipped_count,
            entries,
        }
    }

    pub(crate) fn filtered(&self, minimum_bytes: u64) -> Self {
        Self::from_retained_entries(
            self.root.clone(),
            self.scanned_at_ms,
            self.scan_mode,
            minimum_bytes,
            self.skipped_count,
            self.retained_entries.clone(),
        )
    }
}
