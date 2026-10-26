use super::{
    sanitize_detail_label, snapshot::snapshot_io_error, PrivacyDetailEntry, PrivacyTimeRange,
    NEXT_SNAPSHOT,
};
use crate::{filesystem::atomic_replace::replace_file, CoreError, CoreResult};
use std::{fs, io::Write, path::Path, sync::atomic::Ordering};

const MAX_FIREFOX_LOGIN_JSON_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn firefox_saved_password_details(
    path: &Path,
    offset: u64,
    limit: u32,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    let metadata = fs::metadata(path).map_err(snapshot_io_error)?;
    if metadata.len() > MAX_FIREFOX_LOGIN_JSON_BYTES {
        return Err(CoreError::operation_failed(
            "Firefox saved-password source exceeds the scan limit",
        ));
    }
    let document: FirefoxLoginDetailDocument = serde_json::from_reader(
        fs::File::open(path).map_err(snapshot_io_error)?,
    )
    .map_err(|_| CoreError::operation_failed("Firefox saved-password schema is unsupported"))?;
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    Ok(document
        .logins
        .into_iter()
        .skip(start)
        .take(limit as usize)
        .map(|login| PrivacyDetailEntry {
            label: sanitize_detail_label(&login.hostname),
            item_count: 1,
        })
        .collect())
}

#[derive(serde::Deserialize)]
struct FirefoxLoginDetailDocument {
    logins: Vec<FirefoxLoginDetail>,
}

#[derive(serde::Deserialize)]
struct FirefoxLoginDetail {
    hostname: String,
}

pub(super) fn count_firefox_saved_passwords(
    path: &Path,
    range: PrivacyTimeRange,
) -> CoreResult<u64> {
    if range != PrivacyTimeRange::AllTime {
        return Err(CoreError::operation_failed(
            "Firefox saved-password time range is unsupported",
        ));
    }
    let metadata = fs::metadata(path).map_err(snapshot_io_error)?;
    if metadata.len() > MAX_FIREFOX_LOGIN_JSON_BYTES {
        return Err(CoreError::operation_failed(
            "Firefox saved-password source exceeds the scan limit",
        ));
    }
    // Stream the top-level login entries as ignored values so encrypted passwords, hosts, and
    // usernames are never retained by Core, written to logs, or exposed across the protocol.
    let document: FirefoxLoginDocument = serde_json::from_reader(
        fs::File::open(path).map_err(snapshot_io_error)?,
    )
    .map_err(|_| CoreError::operation_failed("Firefox saved-password schema is unsupported"))?;
    Ok(document.login_count)
}

#[derive(serde::Deserialize)]
struct FirefoxLoginDocument {
    #[serde(rename = "logins", deserialize_with = "deserialize_entry_count")]
    login_count: u64,
}

fn deserialize_entry_count<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{IgnoredAny, SeqAccess, Visitor};

    struct EntryCountVisitor;

    impl<'de> Visitor<'de> for EntryCountVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an array of credential records")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut count = 0_u64;
            while sequence.next_element::<IgnoredAny>()?.is_some() {
                count = count
                    .checked_add(1)
                    .ok_or_else(|| serde::de::Error::custom("credential count overflowed"))?;
            }
            Ok(count)
        }
    }

    deserializer.deserialize_seq(EntryCountVisitor)
}

pub(super) fn clear_firefox_saved_passwords(
    path: &Path,
    range: PrivacyTimeRange,
) -> CoreResult<u64> {
    let before = count_firefox_saved_passwords(path, range)?;
    if before == 0 {
        return Ok(0);
    }
    let metadata = fs::metadata(path).map_err(snapshot_io_error)?;
    if metadata.len() > MAX_FIREFOX_LOGIN_JSON_BYTES {
        return Err(CoreError::operation_failed(
            "Firefox saved-password source exceeds the cleanup limit",
        ));
    }
    // Firefox keeps encrypted credentials in the `logins` array alongside profile metadata.
    // Replace only that array, preserve every unrelated top-level field, and never log or expose
    // the parsed document. The replacement is synced before an atomic same-directory file swap.
    let mut document: serde_json::Value = serde_json::from_reader(
        fs::File::open(path).map_err(snapshot_io_error)?,
    )
    .map_err(|_| CoreError::operation_failed("Firefox saved-password schema is unsupported"))?;
    let logins = document
        .as_object_mut()
        .and_then(|object| object.get_mut("logins"))
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            CoreError::operation_failed("Firefox saved-password schema is unsupported")
        })?;
    logins.clear();
    let replacement = serde_json::to_vec(&document).map_err(|_| {
        CoreError::operation_failed("Firefox saved-password cleanup serialization failed")
    })?;
    write_atomic_replacement(path, &replacement, metadata.permissions())?;
    if count_firefox_saved_passwords(path, range)? != 0 {
        return Err(CoreError::operation_failed(
            "Firefox saved-password verification failed",
        ));
    }
    Ok(before)
}

fn write_atomic_replacement(
    path: &Path,
    content: &[u8],
    permissions: fs::Permissions,
) -> CoreResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::operation_failed("privacy source parent is unavailable"))?;
    let temporary = parent.join(format!(
        ".mangodisk-privacy-{}-{}.tmp",
        std::process::id(),
        NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::set_permissions(&temporary, permissions)?;
        replace_file(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(CoreError::operation_failed(format!(
            "privacy source replacement failed kind={:?}",
            error.kind()
        )));
    }
    Ok(())
}
