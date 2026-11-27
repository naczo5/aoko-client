//! Corrupt-store notices for the two data tabs.
//!
//! Ported from `Aoko/Views/StoreRecovery.cs`. A recovered store looks exactly like
//! a fresh install, so both tabs ask whether a corrupt file was moved aside during
//! *this* session.
//!
//! Two managed mechanisms do not survive, and neither is a loss:
//!
//! * The reflection hooks (`RecoveredFrom`, `PersistenceBlocked`) existed because
//!   `Aoko.Core` could not grow a member the view was allowed to see. In Rust the
//!   store hands these in through [`StoreNotice`], which is typed instead of probed.
//! * The directory scan stays out of this crate: `MIGRATION_ARCHITECTURE.md` section
//!   2 gives file-system access to `aoko-platform`, so the decision below takes an
//!   already-listed set of quarantine files and stays pure and testable.

/// The accounts store file, named in the notice.
pub const ACCOUNTS_FILE_NAME: &str = "accounts.json";

/// The servers store file, named in the notice.
pub const SERVERS_FILE_NAME: &str = "servers.json";

/// Infix of a quarantined copy: `accounts.json.corrupt-20240101T120000`.
pub const QUARANTINE_MARKER: &str = ".corrupt-";

/// A store whose file could not be read keeps the saved rows out of sight, so it
/// says so and pauses saving rather than overwriting them.
pub const READ_BLOCKED_MESSAGE: &str =
    "The saved file is locked or unreadable, so saving is paused to avoid overwriting it.";

/// What the store reports about itself, replacing the two reflection probes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreNotice {
    /// `RecoveredFrom`: path of the quarantine copy this session created.
    pub recovered_from: Option<String>,
    /// `PersistenceBlocked`: the file existed but could not be read.
    pub persistence_blocked: bool,
}

/// One `*.corrupt-*` file found beside a store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineFile {
    /// Full path, so the notice can name the quarantined copy.
    pub path: String,
    /// Last write time in milliseconds since the Unix epoch.
    pub modified_ms: i64,
}

/// `accounts.json could not be read. A copy was kept as accounts.json.corrupt-…`
#[must_use]
pub fn message(quarantined_path: &str) -> String {
    let file = file_name_of(quarantined_path);
    let original = match file.find(QUARANTINE_MARKER) {
        Some(index) if index > 0 => &file[..index],
        _ => "The saved file",
    };

    format!("{original} could not be read. A copy was kept as {file}.")
}

/// The decision [`notice`] makes, with the sentence already resolved.
#[must_use]
pub fn notice(
    store: Option<&StoreNotice>,
    quarantine: &[QuarantineFile],
    store_file_name: &str,
    recovered_after_ms: i64,
) -> Option<String> {
    if let Some(store) = store {
        if store.persistence_blocked {
            return Some(READ_BLOCKED_MESSAGE.to_owned());
        }

        if let Some(recovered) = store
            .recovered_from
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return Some(message(recovered));
        }
    }

    find_quarantined(quarantine, store_file_name, recovered_after_ms)
}

/// Newest quarantine file written at or after `recovered_after_ms`.
///
/// The cut-off is what stops an old `*.corrupt-*` copy from a previous run raising
/// the notice forever: anything older than the process start belongs to that run.
#[must_use]
pub fn find_quarantined(
    quarantine: &[QuarantineFile],
    store_file_name: &str,
    recovered_after_ms: i64,
) -> Option<String> {
    let prefix = format!("{store_file_name}{QUARANTINE_MARKER}");

    quarantine
        .iter()
        .filter(|file| {
            file_name_of(&file.path)
                .to_lowercase()
                .starts_with(&prefix.to_lowercase())
        })
        .filter(|file| file.modified_ms >= recovered_after_ms)
        .max_by_key(|file| file.modified_ms)
        .map(|file| message(&file.path))
}

/// Both separators, so a Windows path formats correctly on any host.
fn file_name_of(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_START_MS: i64 = 1_700_000_000_000;

    #[test]
    fn the_notice_names_the_original_and_the_kept_copy() {
        assert_eq!(
            message("/home/u/.config/aoko/accounts.json.corrupt-20240101T120000Z"),
            "accounts.json could not be read. \
             A copy was kept as accounts.json.corrupt-20240101T120000Z."
        );
        assert_eq!(
            message("C:\\Users\\u\\AppData\\aoko\\servers.json.corrupt-1"),
            "servers.json could not be read. A copy was kept as servers.json.corrupt-1."
        );
    }

    #[test]
    fn a_path_without_the_marker_falls_back_to_the_generic_subject() {
        assert_eq!(
            message("/tmp/mystery.bin"),
            "The saved file could not be read. A copy was kept as mystery.bin."
        );
    }

    #[test]
    fn a_blocked_store_pauses_saving_and_says_so_first() {
        let blocked = StoreNotice {
            recovered_from: Some("/tmp/accounts.json.corrupt-1".to_owned()),
            persistence_blocked: true,
        };

        assert_eq!(
            notice(Some(&blocked), &[], ACCOUNTS_FILE_NAME, SESSION_START_MS).as_deref(),
            Some(READ_BLOCKED_MESSAGE),
            "a blocked store outranks a quarantine copy"
        );
    }

    #[test]
    fn a_recovered_store_reports_the_copy_it_created() {
        let recovered = StoreNotice {
            recovered_from: Some("/tmp/servers.json.corrupt-9".to_owned()),
            persistence_blocked: false,
        };

        assert_eq!(
            notice(Some(&recovered), &[], SERVERS_FILE_NAME, SESSION_START_MS).as_deref(),
            Some("servers.json could not be read. A copy was kept as servers.json.corrupt-9.")
        );
    }

    #[test]
    fn an_older_run_never_raises_the_notice() {
        let files = vec![QuarantineFile {
            path: "/tmp/accounts.json.corrupt-old".to_owned(),
            modified_ms: SESSION_START_MS - 1,
        }];

        assert_eq!(
            find_quarantined(&files, ACCOUNTS_FILE_NAME, SESSION_START_MS),
            None
        );
        assert_eq!(
            notice(None, &files, ACCOUNTS_FILE_NAME, SESSION_START_MS),
            None
        );
    }

    #[test]
    fn the_newest_quarantine_of_this_run_wins() {
        let files = vec![
            QuarantineFile {
                path: "/tmp/accounts.json.corrupt-a".to_owned(),
                modified_ms: SESSION_START_MS + 10,
            },
            QuarantineFile {
                path: "/tmp/accounts.json.corrupt-b".to_owned(),
                modified_ms: SESSION_START_MS + 20,
            },
            // Another store's quarantine must not be reported by this tab.
            QuarantineFile {
                path: "/tmp/servers.json.corrupt-c".to_owned(),
                modified_ms: SESSION_START_MS + 30,
            },
        ];

        assert_eq!(
            find_quarantined(&files, ACCOUNTS_FILE_NAME, SESSION_START_MS).as_deref(),
            Some("accounts.json could not be read. A copy was kept as accounts.json.corrupt-b.")
        );
    }

    #[test]
    fn matching_ignores_case_like_the_managed_filter() {
        let files = vec![QuarantineFile {
            path: "/tmp/ACCOUNTS.JSON.CORRUPT-X".to_owned(),
            modified_ms: SESSION_START_MS,
        }];

        assert!(find_quarantined(&files, ACCOUNTS_FILE_NAME, SESSION_START_MS).is_some());
    }
}
