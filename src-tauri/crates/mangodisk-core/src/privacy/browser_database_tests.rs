use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct DatabaseFixture {
    directory: PathBuf,
    path: PathBuf,
}

impl DatabaseFixture {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mangodisk-privacy-{name}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("privacy fixture directory must be created");
        let path = directory.join("database.sqlite");
        Self { directory, path }
    }
    fn connection(&self) -> Connection {
        Connection::open(&self.path).expect("privacy fixture database must open")
    }
}

impl Drop for DatabaseFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn browser_epochs_use_expected_units() {
    let now = 1_700_000_000_000;
    assert_eq!(
        cutoff_for(
            PrivacyTimeRange::AllTime,
            PlatformPrivacyBrowserKind::Chromium,
            now
        ),
        None
    );
    assert_eq!(
        cutoff_for(
            PrivacyTimeRange::LastHour,
            PlatformPrivacyBrowserKind::Firefox,
            now
        ),
        Some((now as i64 - 3_600_000) * 1_000)
    );
    assert_eq!(
        cutoff_for(
            PrivacyTimeRange::LastHour,
            PlatformPrivacyBrowserKind::Safari,
            now
        ),
        Some((now as i64 - 3_600_000) / 1_000 - SAFARI_EPOCH_OFFSET_SECONDS)
    );
}

#[test]
fn locked_browser_database_is_counted_from_private_snapshot() {
    let fixture = DatabaseFixture::new("locked-snapshot");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE visits(id INTEGER PRIMARY KEY, visit_time INTEGER);
                 INSERT INTO visits VALUES (1, 1), (2, 2);
                 PRAGMA locking_mode = EXCLUSIVE;
                 BEGIN EXCLUSIVE;",
        )
        .unwrap();

    assert_eq!(
        count(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    connection.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn snapshot_includes_committed_wal_records_and_is_removed_on_drop() {
    let fixture = DatabaseFixture::new("wal-snapshot");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
                 PRAGMA wal_autocheckpoint = 0;
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, visit_time INTEGER);
                 INSERT INTO visits VALUES (1, 1), (2, 2), (3, 3);",
        )
        .unwrap();

    let snapshot = DatabaseSnapshot::capture(&fixture.path).unwrap();
    let snapshot_directory = snapshot.directory.clone();
    assert!(snapshot_directory.is_dir());
    assert_eq!(
        count_at_path(
            snapshot.path(),
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::AllTime,
            0,
            true,
        )
        .unwrap(),
        3
    );
    drop(snapshot);
    assert!(!snapshot_directory.exists());
}

#[test]
fn count_schema_does_not_require_cleanup_only_tables() {
    let fixture = DatabaseFixture::new("minimal-count-schema");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE downloads(start_time INTEGER);
                 INSERT INTO downloads VALUES (1), (2);",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        count(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::DownloadHistory,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    assert!(clear(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::DownloadHistory,
        PrivacyTimeRange::AllTime,
        0,
    )
    .is_err());
}

#[test]
fn credential_and_autofill_scans_return_only_aggregate_counts() {
    let login_fixture = DatabaseFixture::new("credential-count");
    let login_connection = login_fixture.connection();
    login_connection
        .execute_batch(
            "CREATE TABLE logins(origin_url TEXT, password_value BLOB);
                 INSERT INTO logins VALUES ('one', X'01'), ('two', X'02');",
        )
        .unwrap();
    drop(login_connection);
    assert_eq!(
        count(
            &login_fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::SavedPasswords,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );

    let autofill_fixture = DatabaseFixture::new("autofill-count");
    let autofill_connection = autofill_fixture.connection();
    autofill_connection
        .execute_batch(
            "CREATE TABLE autofill(name TEXT, value TEXT);
                 INSERT INTO autofill VALUES ('one', 'private'), ('two', 'private');",
        )
        .unwrap();
    drop(autofill_connection);
    assert_eq!(
        count(
            &autofill_fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::AutofillData,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );

    let firefox_fixture = DatabaseFixture::new("firefox-credential-count");
    fs::write(
        &firefox_fixture.path,
        br#"{"nextId":4,"logins":[{"id":1},{"id":2},{"id":3}]}"#,
    )
    .unwrap();
    assert_eq!(
        count(
            &firefox_fixture.path,
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::SavedPasswords,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        3
    );
}

#[test]
fn chromium_credential_cleanup_removes_password_rows_and_preserves_unrelated_data() {
    let fixture = DatabaseFixture::new("chromium-credential-cleanup");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE logins(id INTEGER PRIMARY KEY, origin_url TEXT, password_value BLOB);
                 CREATE TABLE password_notes(id INTEGER PRIMARY KEY, parent_id INTEGER, value BLOB);
                 CREATE TABLE stats(id INTEGER PRIMARY KEY, origin_domain TEXT);
                 CREATE TABLE unrelated_preferences(id INTEGER PRIMARY KEY, value TEXT);
                 INSERT INTO logins VALUES (1, 'one', X'01'), (2, 'two', X'02');
                 INSERT INTO password_notes VALUES (1, 1, X'03');
                 INSERT INTO stats VALUES (1, 'one');
                 INSERT INTO unrelated_preferences VALUES (1, 'preserve');",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::SavedPasswords,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    let connection = fixture.connection();
    for table in ["logins", "password_notes", "stats"] {
        let remaining = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(remaining, 0, "{table} must be cleared");
    }
    assert_eq!(
        connection
            .query_row("SELECT value FROM unrelated_preferences", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "preserve"
    );
}

#[test]
fn form_autofill_cleanup_preserves_profiles_and_unrelated_tables() {
    let chromium_fixture = DatabaseFixture::new("chromium-autofill-cleanup");
    let chromium_connection = chromium_fixture.connection();
    chromium_connection
        .execute_batch(
            "CREATE TABLE autofill(name TEXT, value TEXT);
                 CREATE TABLE autofill_profiles(guid TEXT PRIMARY KEY, full_name TEXT);
                 INSERT INTO autofill VALUES ('one', 'private'), ('two', 'private');
                 INSERT INTO autofill_profiles VALUES ('profile', 'preserve');",
        )
        .unwrap();
    drop(chromium_connection);
    assert_eq!(
        clear(
            &chromium_fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::AutofillData,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    let chromium_connection = chromium_fixture.connection();
    assert_eq!(
        chromium_connection
            .query_row("SELECT COUNT(*) FROM autofill_profiles", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );

    let firefox_fixture = DatabaseFixture::new("firefox-autofill-cleanup");
    let firefox_connection = firefox_fixture.connection();
    firefox_connection
        .execute_batch(
            "CREATE TABLE moz_formhistory(id INTEGER PRIMARY KEY, value TEXT);
                 CREATE TABLE unrelated_preferences(id INTEGER PRIMARY KEY, value TEXT);
                 INSERT INTO moz_formhistory VALUES (1, 'private'), (2, 'private');
                 INSERT INTO unrelated_preferences VALUES (1, 'preserve');",
        )
        .unwrap();
    drop(firefox_connection);
    assert_eq!(
        clear(
            &firefox_fixture.path,
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::AutofillData,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    let firefox_connection = firefox_fixture.connection();
    assert_eq!(
        firefox_connection
            .query_row("SELECT COUNT(*) FROM unrelated_preferences", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn firefox_credential_cleanup_preserves_non_login_metadata() {
    let fixture = DatabaseFixture::new("firefox-credential-cleanup");
    fs::write(
            &fixture.path,
            br#"{"nextId":4,"logins":[{"id":1,"encryptedPassword":"private"},{"id":2,"encryptedPassword":"private"}],"disabledHosts":["preserve.invalid"],"version":3}"#,
        )
        .unwrap();

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::SavedPasswords,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    let document: serde_json::Value =
        serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
    assert_eq!(document["logins"].as_array().unwrap().len(), 0);
    assert_eq!(document["nextId"], 4);
    assert_eq!(document["disabledHosts"][0], "preserve.invalid");
    assert_eq!(document["version"], 3);
}

#[test]
fn chromium_history_range_preserves_downloads_and_older_visits() {
    let fixture = DatabaseFixture::new("chromium-history");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
             CREATE TABLE downloads(id INTEGER PRIMARY KEY, start_time INTEGER);
             CREATE TABLE downloads_url_chains(id INTEGER, chain_index INTEGER, url TEXT);",
        )
        .unwrap();
    let now = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Chromium,
        now,
    )
    .unwrap();
    connection
        .execute(
            "INSERT INTO urls VALUES (1, 'https://old.invalid'), (2, 'https://new.invalid')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO visits VALUES (1, 1, ?1), (2, 2, ?2)",
            [cutoff - 1, cutoff + 1],
        )
        .unwrap();
    connection
        .execute("INSERT INTO downloads VALUES (7, ?1)", [cutoff + 1])
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::LastHour,
            now
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM visits", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM urls", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM downloads", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn chromium_history_cleanup_removes_matching_derived_records() {
    let fixture = DatabaseFixture::new("chromium-history-derived");
    let connection = fixture.connection();
    connection
            .execute_batch(
                "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(
                     id INTEGER PRIMARY KEY,
                     url INTEGER,
                     visit_time INTEGER,
                     from_visit INTEGER,
                     opener_visit INTEGER,
                     visited_link_id INTEGER
                 );
                 CREATE TABLE visit_source(id INTEGER PRIMARY KEY, source INTEGER);
                 CREATE TABLE content_annotations(visit_id INTEGER PRIMARY KEY, search_terms TEXT);
                 CREATE TABLE context_annotations(visit_id INTEGER PRIMARY KEY, flags INTEGER);
                 CREATE TABLE clusters(cluster_id INTEGER PRIMARY KEY, label TEXT);
                 CREATE TABLE clusters_and_visits(cluster_id INTEGER, visit_id INTEGER);
                 CREATE TABLE cluster_visit_duplicates(visit_id INTEGER, duplicate_visit_id INTEGER);
                 CREATE TABLE keyword_search_terms(url_id INTEGER, term TEXT);
                 CREATE TABLE segments(id INTEGER PRIMARY KEY, url_id INTEGER);
                 CREATE TABLE segment_usage(id INTEGER PRIMARY KEY, segment_id INTEGER);
                 CREATE TABLE visited_links(id INTEGER PRIMARY KEY, top_level_url TEXT);
                 INSERT INTO urls VALUES (1, 'https://old.invalid'), (2, 'https://new.invalid');
                 INSERT INTO visit_source VALUES (1, 1), (2, 1);
                 INSERT INTO content_annotations VALUES (1, 'old'), (2, 'new');
                 INSERT INTO context_annotations VALUES (1, 0), (2, 0);
                 INSERT INTO clusters VALUES (10, 'old'), (20, 'new');
                 INSERT INTO clusters_and_visits VALUES (10, 1), (20, 2);
                 INSERT INTO cluster_visit_duplicates VALUES (1, 2);
                 INSERT INTO keyword_search_terms VALUES (1, 'old'), (2, 'new');
                 INSERT INTO segments VALUES (100, 1), (200, 2);
                 INSERT INTO segment_usage VALUES (1000, 100), (2000, 200);
                 INSERT INTO visited_links VALUES (10000, 'old'), (20000, 'new');",
            )
            .unwrap();
    let now = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Chromium,
        now,
    )
    .unwrap();
    connection
        .execute(
            "INSERT INTO visits VALUES (1, 1, ?1, 2, 2, 10000), (2, 2, ?2, 0, 0, 20000)",
            [cutoff - 1, cutoff + 1],
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::LastHour,
            now,
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    for table in [
        "urls",
        "visits",
        "visit_source",
        "content_annotations",
        "context_annotations",
        "clusters",
        "clusters_and_visits",
        "keyword_search_terms",
        "segments",
        "segment_usage",
        "visited_links",
    ] {
        let count = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(count, 1, "{table} must retain only the older relationship");
    }
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM cluster_visit_duplicates", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row("SELECT from_visit + opener_visit FROM visits", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn chromium_download_cleanup_never_removes_downloaded_file() {
    let fixture = DatabaseFixture::new("chromium-download");
    let downloaded = fixture.directory.join("important-document.txt");
    fs::write(&downloaded, b"sentinel").unwrap();
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE downloads(id INTEGER PRIMARY KEY, start_time INTEGER);
             CREATE TABLE downloads_url_chains(id INTEGER, chain_index INTEGER, url TEXT);
             INSERT INTO downloads VALUES (1, 1);
             INSERT INTO downloads_url_chains VALUES (1, 0, 'https://download.invalid');",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::DownloadHistory,
            PrivacyTimeRange::AllTime,
            0
        )
        .unwrap(),
        1
    );
    assert_eq!(fs::read(&downloaded).unwrap(), b"sentinel");
}

#[test]
fn chromium_cookie_range_preserves_older_cookie_and_login_database() {
    let fixture = DatabaseFixture::new("chromium-cookies");
    let login_database = fixture.directory.join("Login Data");
    fs::write(&login_database, b"synthetic-password-marker").unwrap();
    let connection = fixture.connection();
    connection
            .execute_batch(
                "CREATE TABLE cookies(id INTEGER PRIMARY KEY, last_access_utc INTEGER, encrypted_value BLOB);",
            )
            .unwrap();
    let now = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Chromium,
        now,
    )
    .unwrap();
    connection
        .execute(
            "INSERT INTO cookies VALUES (1, ?1, X'01'), (2, ?2, X'02')",
            [cutoff - 1, cutoff + 1],
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::Cookies,
            PrivacyTimeRange::LastHour,
            now
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM cookies WHERE id = 1", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    assert_eq!(
        fs::read(&login_database).unwrap(),
        b"synthetic-password-marker"
    );
}

#[test]
fn firefox_history_preserves_bookmarks_and_download_visits() {
    let fixture = DatabaseFixture::new("firefox-history");
    let connection = fixture.connection();
    connection.execute_batch(
            "CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, foreign_count INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE moz_historyvisits(id INTEGER PRIMARY KEY, place_id INTEGER, visit_type INTEGER, visit_date INTEGER);
             CREATE TABLE moz_bookmarks(id INTEGER PRIMARY KEY, fk INTEGER);
             INSERT INTO moz_places VALUES (1, 'https://history.invalid', 0), (2, 'https://bookmark.invalid', 1), (3, 'https://download.invalid', 0);
             INSERT INTO moz_historyvisits VALUES (1, 1, 1, 1), (2, 2, 1, 1), (3, 3, 7, 1);
             INSERT INTO moz_bookmarks VALUES (1, 2);",
        ).unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::AllTime,
            0
        )
        .unwrap(),
        2
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM moz_historyvisits WHERE visit_type = 7",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM moz_places WHERE id = 2", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM moz_bookmarks", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn firefox_site_permissions_are_independent_from_cookies() {
    let fixture = DatabaseFixture::new("firefox-permissions");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE moz_perms(id INTEGER PRIMARY KEY, origin TEXT, type TEXT);
                 CREATE TABLE moz_cookies(id INTEGER PRIMARY KEY, lastAccessed INTEGER);
                 INSERT INTO moz_perms VALUES (1, 'https://permission.invalid', 'camera');
                 INSERT INTO moz_cookies VALUES (1, 1);",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::SitePermissions,
            PrivacyTimeRange::AllTime,
            0
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM moz_cookies", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn safari_history_range_preserves_older_visits() {
    let fixture = DatabaseFixture::new("safari-history");
    let connection = fixture.connection();
    connection
            .execute_batch(
                "CREATE TABLE history_items(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE history_visits(id INTEGER PRIMARY KEY, history_item INTEGER, visit_time REAL);",
            )
            .unwrap();
    let now = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Safari,
        now,
    )
    .unwrap();
    connection
            .execute(
                "INSERT INTO history_items VALUES (1, 'https://old.invalid'), (2, 'https://new.invalid')",
                [],
            )
            .unwrap();
    connection
        .execute(
            "INSERT INTO history_visits VALUES (1, 1, ?1), (2, 2, ?2)",
            [cutoff - 1, cutoff + 1],
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Safari,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::LastHour,
            now
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM history_visits", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM history_items", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn sqlite_wal_records_are_visible_without_profile_copy() {
    let fixture = DatabaseFixture::new("wal");
    let connection = fixture.connection();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection.execute_batch("CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT); CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER); INSERT INTO urls VALUES (1, 'https://wal.invalid'); INSERT INTO visits VALUES (1, 1, 1);").unwrap();

    assert_eq!(
        count(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::BrowsingHistory,
            PrivacyTimeRange::AllTime,
            0
        )
        .unwrap(),
        1
    );
    assert!(
        fixture.path.with_extension("sqlite-wal").exists()
            || PathBuf::from(format!("{}-wal", fixture.path.display())).exists()
    );
}

#[test]
#[ignore = "fixed 100,000-row privacy scan performance workload"]
fn large_chromium_history_count_remains_aggregate_and_bounded() {
    let fixture = DatabaseFixture::new("large-history");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                 INSERT INTO urls VALUES (1, 'https://aggregate.invalid');
                 WITH RECURSIVE sequence(value) AS (
                   SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 100000
                 )
                 INSERT INTO visits(id, url, visit_time) SELECT value, 1, value FROM sequence;",
        )
        .unwrap();
    drop(connection);

    let started = std::time::Instant::now();
    let item_count = count(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0,
    )
    .unwrap();
    println!(
        "privacy_large_history_count={} elapsed_ms={}",
        item_count,
        started.elapsed().as_millis()
    );
    assert_eq!(item_count, 100_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn failed_sqlite_transaction_rolls_back_history_deletion() {
    let fixture = DatabaseFixture::new("rollback");
    let connection = fixture.connection();
    connection.execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
             INSERT INTO urls VALUES (1, 'https://keep.invalid');
             INSERT INTO visits VALUES (1, 1, 1);
             CREATE TRIGGER block_visit_delete BEFORE DELETE ON visits BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
        ).unwrap();
    drop(connection);

    assert!(clear(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0
    )
    .is_err());
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM visits", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM urls", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn chromium_top_sites_cleanup_preserves_unrelated_database_state() {
    let fixture = DatabaseFixture::new("top-sites");
    let connection = fixture.connection();
    connection
        .execute_batch(
            "CREATE TABLE top_sites(url TEXT PRIMARY KEY, url_rank INTEGER, title TEXT);
                 CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
                 INSERT INTO top_sites VALUES
                   ('https://first.invalid', 0, 'private'),
                   ('https://second.invalid', 1, 'private');
                 INSERT INTO meta VALUES ('version', 'preserve');",
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::FrequentlyVisitedSites,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        2
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM meta", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn chromium_address_bar_shortcuts_honor_the_selected_time_range() {
    let fixture = DatabaseFixture::new("address-bar-shortcuts");
    let now_ms = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Chromium,
        now_ms,
    )
    .unwrap();
    let connection = fixture.connection();
    connection
        .execute_batch(&format!(
            "CREATE TABLE omni_box_shortcuts(
                   id TEXT PRIMARY KEY,
                   text TEXT,
                   last_access_time INTEGER
                 );
                 INSERT INTO omni_box_shortcuts VALUES
                   ('older', 'private', {}),
                   ('newer', 'private', {});",
            cutoff - 1,
            cutoff + 1
        ))
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::AddressBarShortcuts,
            PrivacyTimeRange::LastHour,
            now_ms,
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM omni_box_shortcuts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[test]
fn chromium_search_history_cleanup_preserves_navigation_history() {
    let fixture = DatabaseFixture::new("search-history");
    let now_ms = 1_700_000_000_000_u64;
    let cutoff = cutoff_for(
        PrivacyTimeRange::LastHour,
        PlatformPrivacyBrowserKind::Chromium,
        now_ms,
    )
    .unwrap();
    let connection = fixture.connection();
    connection
        .execute_batch(&format!(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, last_visit_time INTEGER);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                 CREATE TABLE keyword_search_terms(url_id INTEGER, term TEXT);
                 INSERT INTO urls VALUES (1, {}), (2, {});
                 INSERT INTO visits VALUES (1, 1, {}), (2, 2, {});
                 INSERT INTO keyword_search_terms VALUES (1, 'older'), (2, 'newer');",
            cutoff - 1,
            cutoff + 1,
            cutoff - 1,
            cutoff + 1
        ))
        .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::SearchHistory,
            PrivacyTimeRange::LastHour,
            now_ms,
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM keyword_search_terms", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM visits", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn chromium_website_icon_cleanup_preserves_database_metadata() {
    let fixture = DatabaseFixture::new("website-icons");
    let connection = fixture.connection();
    connection
            .execute_batch(
                "CREATE TABLE favicons(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE favicon_bitmaps(id INTEGER PRIMARY KEY, icon_id INTEGER, image_data BLOB);
                 CREATE TABLE icon_mapping(id INTEGER PRIMARY KEY, page_url TEXT, icon_id INTEGER);
                 CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
                 INSERT INTO favicons VALUES (1, 'https://icon.invalid/favicon.ico');
                 INSERT INTO favicon_bitmaps VALUES (1, 1, X'00');
                 INSERT INTO icon_mapping VALUES (1, 'https://icon.invalid', 1);
                 INSERT INTO meta VALUES ('version', 'preserve');",
            )
            .unwrap();
    drop(connection);

    assert_eq!(
        clear(
            &fixture.path,
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::WebsiteIcons,
            PrivacyTimeRange::AllTime,
            0,
        )
        .unwrap(),
        1
    );
    let connection = fixture.connection();
    for table in ["icon_mapping", "favicon_bitmaps", "favicons"] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM meta", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn unknown_schema_fails_closed() {
    let fixture = DatabaseFixture::new("unknown-schema");
    fixture
        .connection()
        .execute("CREATE TABLE unexpected(value TEXT)", [])
        .unwrap();
    assert!(count(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0
    )
    .is_err());
}

#[test]
fn history_details_group_matching_urls_and_page_without_exposing_database_paths() {
    let fixture = DatabaseFixture::new("history-details");
    fixture
        .connection()
        .execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                 INSERT INTO urls VALUES
                   (1, 'https://example.invalid/private?q=one'),
                   (2, 'https://second.invalid/path');
                 INSERT INTO visits VALUES (1, 1, 10), (2, 1, 20), (3, 2, 30);",
        )
        .unwrap();

    let first = details(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0,
        0,
        1,
    )
    .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].label, "https://second.invalid/path");
    assert_eq!(first[0].item_count, 1);

    let second = details(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0,
        1,
        1,
    )
    .unwrap();
    assert_eq!(second[0].label, "https://example.invalid/private?q=one");
    assert_eq!(second[0].item_count, 2);
}

#[test]
fn credential_details_expose_only_origins_and_counts() {
    let fixture = DatabaseFixture::new("credential-details");
    fixture
        .connection()
        .execute_batch(
            "CREATE TABLE logins(origin_url TEXT, username_value TEXT, password_value BLOB);
                 INSERT INTO logins VALUES
                   ('https://example.invalid', 'private-user', X'736563726574'),
                   ('https://example.invalid', 'second-user', X'6D6F7265');",
        )
        .unwrap();

    let entries = details(
        &fixture.path,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::SavedPasswords,
        PrivacyTimeRange::AllTime,
        0,
        0,
        20,
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].label, "https://example.invalid");
    assert_eq!(entries[0].item_count, 2);
    let serialized = serde_json::to_string(&entries).unwrap();
    assert!(!serialized.contains("private-user"));
    assert!(!serialized.contains("secret"));
}

#[cfg(unix)]
#[test]
fn symbolic_link_database_is_rejected_before_sqlite_opens_it() {
    use std::os::unix::fs::symlink;

    let fixture = DatabaseFixture::new("database-link");
    fixture
        .connection()
        .execute_batch(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT);
                 CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);",
        )
        .unwrap();
    let link = fixture.directory.join("linked.sqlite");
    symlink(&fixture.path, &link).unwrap();

    let error = count(
        &link,
        PlatformPrivacyBrowserKind::Chromium,
        PrivacyDataKind::BrowsingHistory,
        PrivacyTimeRange::AllTime,
        0,
    )
    .unwrap_err();
    assert!(!error.diagnostic().contains(link.to_string_lossy().as_ref()));
}
