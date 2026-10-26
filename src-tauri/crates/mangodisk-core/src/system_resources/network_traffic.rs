//! Per-interface monitored traffic. Only accepted intervals are accumulated;
//! operating-system counters are never treated as recoverable offline history.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

const MAX_INTERFACES: usize = 128;
const MAX_TIMESTAMP_MS: u64 = 253_402_300_799_999;
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficTotal {
    // Decimal strings retain exact counters across the JavaScript boundary.
    pub received_bytes: String,
    pub transmitted_bytes: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    received: u64,
    transmitted: u64,
    started_at_ms: u64,
}
impl Entry {
    fn total(&self) -> TrafficTotal {
        TrafficTotal {
            received_bytes: self.received.to_string(),
            transmitted_bytes: self.transmitted.to_string(),
            started_at_ms: self.started_at_ms,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct History {
    schema_version: u32,
    interfaces: BTreeMap<String, Entry>,
}
struct State {
    history: History,
    revision: u64,
    saved_revision: u64,
    closed: bool,
}

/// Memory updates do no I/O. Writers serialize snapshots so a delayed periodic
/// save cannot restore data from before a successful reset.
pub struct NetworkTraffic {
    path: PathBuf,
    state: Mutex<State>,
    writer: Mutex<()>,
    _lease: File,
}
impl NetworkTraffic {
    /// Missing history starts empty. Unknown, corrupt or oversized history is
    /// rejected and preserved, rather than silently replacing user totals.
    pub fn open(path: PathBuf) -> CoreResult<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| CoreError::invalid_input("traffic path has no parent"))?;
        fs::create_dir_all(parent).map_err(|error| failure(&path, "create_directory", error))?;
        let lease = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|error| failure(&path, "open_lease", error))?;
        lease
            .try_lock_exclusive()
            .map_err(|error| failure(&path, "lock", error))?;
        let history = match File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| failure(&path, "read", error))?;
                if bytes.len() as u64 > MAX_FILE_BYTES {
                    return Err(failure(&path, "decode", "history exceeds size limit"));
                }
                let history: History = serde_json::from_slice(&bytes)
                    .map_err(|error| failure(&path, "decode", error))?;
                if history.schema_version != 1
                    || history.interfaces.len() > MAX_INTERFACES
                    || history.interfaces.iter().any(|(id, entry)| {
                        !valid_id(id)
                            || entry.started_at_ms == 0
                            || entry.started_at_ms > MAX_TIMESTAMP_MS
                    })
                {
                    return Err(failure(&path, "validate", "unsupported or invalid history"));
                }
                history
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => History {
                schema_version: 1,
                interfaces: BTreeMap::new(),
            },
            Err(error) => return Err(failure(&path, "open", error)),
        };
        Ok(Self {
            path,
            state: Mutex::new(State {
                history,
                revision: 0,
                saved_revision: 0,
                closed: false,
            }),
            writer: Mutex::new(()),
            _lease: lease,
        })
    }

    pub fn record(
        &self,
        id: &str,
        received: u64,
        transmitted: u64,
        start_ms: u64,
        end_ms: u64,
    ) -> Option<TrafficTotal> {
        if !valid_id(id) || start_ms == 0 || end_ms < start_ms || end_ms > MAX_TIMESTAMP_MS {
            return None;
        }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return state.history.interfaces.get(id).map(Entry::total);
        }
        if !state.history.interfaces.contains_key(id)
            && state.history.interfaces.len() >= MAX_INTERFACES
        {
            return None;
        }
        let inserted = !state.history.interfaces.contains_key(id);
        let entry = state
            .history
            .interfaces
            .entry(id.to_owned())
            .or_insert(Entry {
                received: 0,
                transmitted: 0,
                started_at_ms: start_ms,
            });
        let before = (entry.received, entry.transmitted);
        // A reset can finish while a native query is in flight. Exclude its
        // entire overlapping interval instead of reintroducing pre-reset bytes.
        if start_ms >= entry.started_at_ms {
            entry.received = entry.received.saturating_add(received);
            entry.transmitted = entry.transmitted.saturating_add(transmitted);
        }
        let changed = inserted || before != (entry.received, entry.transmitted);
        let total = entry.total();
        if changed {
            state.revision = state.revision.wrapping_add(1);
        }
        Some(total)
    }

    pub fn total(&self, id: &str) -> Option<TrafficTotal> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .history
            .interfaces
            .get(id)
            .map(Entry::total)
    }

    /// Persist before publishing the reset. A failed write leaves memory and
    /// disk unchanged; expected_start prevents stale confirmations resetting twice.
    pub fn reset(&self, id: &str, expected_start: u64, now_ms: u64) -> CoreResult<TrafficTotal> {
        let _writer = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let entry = state
            .history
            .interfaces
            .get(id)
            .ok_or_else(|| CoreError::invalid_input("traffic interface is not recorded"))?;
        if state.closed
            || entry.started_at_ms != expected_start
            || now_ms <= entry.started_at_ms
            || now_ms > MAX_TIMESTAMP_MS
        {
            return Err(CoreError::operation_busy("traffic reset target changed"));
        }
        let mut history = state.history.clone();
        let reset = Entry {
            received: 0,
            transmitted: 0,
            started_at_ms: now_ms,
        };
        history.interfaces.insert(id.to_owned(), reset.clone());
        self.write(&history)?;
        state.history = history;
        state.revision = state.revision.wrapping_add(1);
        state.saved_revision = state.revision;
        Ok(reset.total())
    }

    /// Periodic snapshots release the state lock before doing file I/O. Closing
    /// freezes accumulation before the final snapshot, including idle shutdowns.
    pub fn flush(&self, close: bool) -> CoreResult<bool> {
        let _writer = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (history, revision) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.closed |= close;
            if state.revision == state.saved_revision {
                return Ok(false);
            }
            (state.history.clone(), state.revision)
        };
        self.write(&history)?;
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .saved_revision = revision;
        Ok(true)
    }

    fn write(&self, history: &History) -> CoreResult<()> {
        let parent = self.path.parent().expect("validated history directory");
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| failure(&self.path, "create_temporary", error))?;
        serde_json::to_writer(&mut temporary, history)
            .map_err(|error| failure(&self.path, "encode", error))?;
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|error| failure(&self.path, "sync", error))?;
        temporary
            .persist(&self.path)
            .map_err(|error| failure(&self.path, "replace", error.error))?;
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
}
fn failure(path: &Path, stage: &str, error: impl std::fmt::Display) -> CoreError {
    CoreError::persistence(format!(
        "network traffic stage={stage} path={} error={error}",
        path.display()
    ))
}

#[cfg(test)]
#[path = "network_traffic_tests.rs"]
mod tests;
