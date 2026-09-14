use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryOverview {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub swap_used_bytes: u64,
    pub used_percent: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMemory {
    pub id: String,
    pub name: String,
    pub resident_bytes: u64,
    pub process_count: u32,
    /// Bundle or executable location for native icons and file-manager navigation; never telemetry.
    pub icon_path: Option<String>,
    pub is_bundle: bool,
    pub can_quit: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMemorySummary {
    pub applications: Vec<ApplicationMemory>,
    pub readable_process_count: u32,
    pub omitted_process_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemResourceSnapshot {
    pub schema_version: u32,
    pub sampled_at_ms: u64,
    pub memory: MemoryOverview,
    pub processes: Option<ProcessMemorySummary>,
}
