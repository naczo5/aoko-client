//! Profile file I/O: the disk half of `Aoko/Core/Profile.cs`.
//!
//! `aoko-protocol::profile` owns the JSON (`to_profile_json`,
//! `load_profile_from_json`) and the pure helpers (`get_profiles_directory`,
//! `get_legacy_profiles_directory`, `sanitize_config_name`). This module owns the
//! disk: resolving the app-data root, creating the profile directory, migrating the
//! legacy one, and reading or writing a single file. It never parses JSON — a caller
//! gets the text and hands it to the protocol, which is what keeps the wire format
//! out of the OS crate.
//!
//! ## Deviation from the managed host: atomic writes
//!
//! `ProfileManager.TryWriteProfile` used `File.WriteAllText`, which truncates the
//! target in place. A process that dies mid-write leaves a truncated or
//! half-written profile, and the next `LoadProfile` silently returns `null` for a
//! file that looked like a save.
//!
//! [`ProfileStore::write_profile`] writes a sibling temporary file, flushes it to
//! the device, and then renames it over the target. On success the observable
//! result is identical: the same bytes at the same path. On failure the previous
//! contents survive intact, and the temporary file is removed rather than left
//! behind.
//!
//! Unlike the managed host, which caught `IOException`/`UnauthorizedAccessException`
//! and returned silently, every operation here reports a classified
//! [`PlatformError`]. Callers decide whether a failure is fatal; the library does
//! not hide it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use aoko_protocol::profile::{
    get_legacy_profiles_directory, get_profiles_directory, sanitize_config_name,
    AUTO_SAVE_CONFIG_NAME,
};

use crate::error::PlatformError;

/// Distinguishes concurrent temporary files written by one process; the pid
/// separates processes.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A profile directory pair: the live one and the one the shipped app used.
///
/// Holds paths only, so it is cheap to clone and carries no open handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileStore {
    profiles_dir: PathBuf,
    legacy_profiles_dir: PathBuf,
}

impl ProfileStore {
    /// Builds a store over an explicit app-data root.
    ///
    /// This is the testable entry point: the root is a parameter, never read from
    /// the environment, so a caller or a test can point it at a scratch directory.
    #[must_use]
    pub fn new(app_data: &Path) -> Self {
        Self {
            profiles_dir: get_profiles_directory(app_data),
            legacy_profiles_dir: get_legacy_profiles_directory(app_data),
        }
    }

    /// Builds a store over the platform's own app-data root.
    ///
    /// # Errors
    /// [`PlatformError::Os`] when this session does not say where user data lives.
    /// There is deliberately no fallback to the working directory: profiles written
    /// next to the executable are lost on the next launch and invisible to the
    /// user.
    pub fn from_system() -> Result<Self, PlatformError> {
        Ok(Self::new(&crate::profile::app_data_directory()?))
    }

    /// Directory that holds the live `*.json` profiles.
    #[must_use]
    pub fn profiles_dir(&self) -> &Path {
        &self.profiles_dir
    }

    /// Directory the shipped `LegoClicker` app wrote to, scanned once for migration.
    ///
    /// Never created: an absent legacy directory is the normal case on a fresh
    /// install, and inventing one would leave state the old app never wrote.
    #[must_use]
    pub fn legacy_profiles_dir(&self) -> &Path {
        &self.legacy_profiles_dir
    }

    /// Path a profile named `name` lives at, or `None` when the name sanitises to
    /// nothing usable (empty, whitespace, `.`, `..`, or only rejected characters).
    ///
    /// The name is passed through the protocol's `sanitize_config_name`, so a name
    /// containing a path separator cannot escape the profile directory.
    #[must_use]
    pub fn profile_path(&self, name: &str) -> Option<PathBuf> {
        let stem = sanitize_config_name(Some(name));
        if stem.is_empty() {
            return None;
        }
        Some(self.profiles_dir.join(format!("{stem}.json")))
    }

    /// Creates the profile directory and migrates the legacy profiles into it.
    ///
    /// # Errors
    /// A classified [`PlatformError`] when the directory cannot be created, which is
    /// the one failure that makes every later save impossible and therefore must not
    /// be swallowed the way the managed `try`/`catch` did.
    ///
    /// Migration is best effort, as it was in the managed host: a failure there is
    /// logged and does not fail initialisation, because an unreadable legacy
    /// directory must not stop a fresh install from working.
    pub fn initialize(&self) -> Result<(), PlatformError> {
        fs::create_dir_all(&self.profiles_dir)
            .map_err(|err| map_io("create profile directory", err))?;

        match self.migrate_legacy_profiles() {
            Ok(0) => {}
            Ok(copied) => tracing::debug!(
                "copied {copied} legacy profile(s) from {}",
                self.legacy_profiles_dir.display()
            ),
            Err(err) => tracing::debug!(
                "legacy profile migration from {} failed: {}",
                self.legacy_profiles_dir.display(),
                err.user_facing()
            ),
        }
        Ok(())
    }

    /// Copies `*.json` files from [`ProfileStore::legacy_profiles_dir`] into the live
    /// directory and returns how many were written.
    ///
    /// `ProfileManager.CopyLegacyProfilesIfNeeded`: nothing happens when there is no
    /// legacy directory, or when the live directory already holds a profile, so a
    /// user who has been using the new app never has old files pushed back in.
    ///
    /// # Errors
    /// A classified [`PlatformError`] when the legacy directory cannot be read or a
    /// copy fails.
    pub fn migrate_legacy_profiles(&self) -> Result<usize, PlatformError> {
        if !self.legacy_profiles_dir.is_dir() || self.has_profiles() {
            return Ok(0);
        }

        fs::create_dir_all(&self.profiles_dir)
            .map_err(|err| map_io("create profile directory", err))?;

        let entries = fs::read_dir(&self.legacy_profiles_dir)
            .map_err(|err| map_io("read legacy profile directory", err))?;

        let mut copied = 0;
        for entry in entries {
            let entry = entry.map_err(|err| map_io("read legacy profile directory", err))?;
            let source = entry.path();
            if !is_json_file(&source) {
                continue;
            }
            let Some(file_name) = source.file_name() else {
                continue;
            };
            let target = self.profiles_dir.join(file_name);
            // `File.Copy(overwrite: false)`: an existing file wins.
            if target.exists() {
                continue;
            }
            fs::copy(&source, &target).map_err(|err| map_io("copy legacy profile", err))?;
            copied += 1;
        }

        Ok(copied)
    }

    /// Reads a profile as text, for the caller to parse with
    /// `aoko_protocol::profile::load_profile_from_json`.
    ///
    /// # Errors
    /// - [`PlatformError::InvalidArgument`] when `name` sanitises to nothing.
    /// - [`PlatformError::NotFound`] when there is no such file.
    /// - [`PlatformError::PermissionDenied`] when the file exists but cannot be read.
    /// - [`PlatformError::Os`] for anything else, including a directory in the way.
    pub fn read_profile(&self, name: &str) -> Result<String, PlatformError> {
        let path = self.profile_path(name).ok_or_else(|| invalid_name(name))?;
        read_text(&path)
    }

    /// Writes a profile, creating the directory if needed, atomically.
    ///
    /// The bytes are written to a sibling temporary file, flushed, and renamed over
    /// the target, so a crash mid-write cannot truncate an existing profile.
    ///
    /// # Errors
    /// - [`PlatformError::InvalidArgument`] when `name` sanitises to nothing.
    /// - [`PlatformError::PermissionDenied`] / [`PlatformError::Os`] when the
    ///   directory or file cannot be written.
    pub fn write_profile(&self, name: &str, json: &str) -> Result<(), PlatformError> {
        let path = self.profile_path(name).ok_or_else(|| invalid_name(name))?;
        write_atomically(&path, json)
    }

    /// Whether a readable profile file exists for `name`.
    ///
    /// A directory with the profile's name is not a profile, matching
    /// `File.Exists`.
    #[must_use]
    pub fn profile_exists(&self, name: &str) -> bool {
        self.profile_path(name).is_some_and(|path| path.is_file())
    }

    /// User-facing profile names, excluding the internal auto-save slot, sorted
    /// case-insensitively.
    ///
    /// `ProfileManager.GetConfigNames`. A missing directory is an empty list rather
    /// than an error, because a first launch has none. File names that are not UTF-8
    /// are skipped: they cannot round-trip through a `String` profile name.
    ///
    /// # Errors
    /// A classified [`PlatformError`] when the directory exists but cannot be read.
    pub fn list_profiles(&self) -> Result<Vec<String>, PlatformError> {
        let entries = match fs::read_dir(&self.profiles_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(map_io("read profile directory", err)),
        };

        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| map_io("read profile directory", err))?;
            let path = entry.path();
            if !is_json_file(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if stem.eq_ignore_ascii_case(AUTO_SAVE_CONFIG_NAME) {
                continue;
            }
            names.push(stem.to_owned());
        }

        // `StringComparer.OrdinalIgnoreCase`, with the raw name as a tie-break so the
        // order is stable when two names differ only by case. The key is computed
        // once per name rather than once per comparison.
        names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
        Ok(names)
    }

    fn has_profiles(&self) -> bool {
        match fs::read_dir(&self.profiles_dir) {
            Ok(entries) => entries.flatten().any(|entry| is_json_file(&entry.path())),
            Err(_) => false,
        }
    }
}

/// Resolves the app-data root for this platform.
///
/// Windows reads `%APPDATA%` and falls back to `%USERPROFILE%\AppData\Roaming`;
/// Linux reads `$XDG_DATA_HOME` (when absolute) and falls back to
/// `$HOME/.local/share`. Both are implemented next to the rest of that platform's
/// code and are re-exported here so callers have one path to remember.
///
/// # Errors
/// [`PlatformError::Os`] when no variable names a location, which is the honest
/// answer: there is nowhere to put a profile, and guessing the working directory
/// would lose the file.
#[cfg(target_os = "linux")]
pub use crate::linux::app_data_directory;
#[cfg(windows)]
pub use crate::windows::app_data_directory;

/// Targets with no profile support: resolution always reports why.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn app_data_directory() -> Result<PathBuf, PlatformError> {
    Err(PlatformError::os(
        "resolve app data directory",
        "this target has no app-data location",
    ))
}

fn read_text(path: &Path) -> Result<String, PlatformError> {
    let bytes = fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            return PlatformError::not_found(format!("{} does not exist", path.display()));
        }
        map_io("read profile", err)
    })?;
    // `File.ReadAllText` decodes as UTF-8 with a replacement fallback, so invalid
    // bytes become U+FFFD and the parse decides whether the document is usable;
    // failing here instead would turn a recoverable file into an I/O error.
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn write_atomically(target: &Path, contents: &str) -> Result<(), PlatformError> {
    let directory = target.parent().ok_or_else(|| {
        PlatformError::invalid_argument(format!("{} has no directory", target.display()))
    })?;
    fs::create_dir_all(directory).map_err(|err| map_io("create profile directory", err))?;

    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            PlatformError::invalid_argument(format!("{} is not a file name", target.display()))
        })?;

    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        ".{file_name}.{}.{sequence}.tmp",
        std::process::id()
    ));

    let written = (|| -> Result<(), PlatformError> {
        let mut file =
            fs::File::create(&temporary).map_err(|err| map_io("create profile temp file", err))?;
        file.write_all(contents.as_bytes())
            .map_err(|err| map_io("write profile temp file", err))?;
        // The rename is what makes the swap atomic; only a flushed file makes the
        // data durable, and a crash between an unsynced rename and the write-back
        // would otherwise leave an empty profile where a good one used to be.
        file.sync_all()
            .map_err(|err| map_io("sync profile temp file", err))?;
        // Windows cannot rename a file that is still open.
        drop(file);
        fs::rename(&temporary, target).map_err(|err| map_io("replace profile file", err))
    })();

    if written.is_err() {
        // Never leave a partial temporary behind: it would list as clutter and could
        // confuse a later save.
        let _ = fs::remove_file(&temporary);
    }
    written
}

fn is_json_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

fn invalid_name(name: &str) -> PlatformError {
    PlatformError::invalid_argument(format!(
        "{name:?} is not a usable profile name: it is empty or contains only rejected characters"
    ))
}

/// Shared mapping for the managed `IOException` / `UnauthorizedAccessException`
/// pair: a denied write or read has one obvious remedy, so it gets its own variant.
fn map_io(operation: &'static str, err: std::io::Error) -> PlatformError {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        return PlatformError::permission_denied(format!("{operation}: {err}"));
    }
    PlatformError::os(operation, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store() -> (TempDir, ProfileStore) {
        let root = tempfile::tempdir().expect("a scratch app-data root");
        let store = ProfileStore::new(root.path());
        (root, store)
    }

    fn file_names(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("list directory")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_directories_come_from_the_protocol_helpers() {
        let (root, store) = store();
        assert_eq!(
            store.profiles_dir(),
            root.path().join("Aoko").join("profiles")
        );
        assert_eq!(
            store.legacy_profiles_dir(),
            root.path().join("LegoClicker").join("profiles")
        );
    }

    #[test]
    fn initialize_creates_the_missing_directory_and_not_the_legacy_one() {
        let (_root, store) = store();
        assert!(!store.profiles_dir().exists());
        store.initialize().expect("a writable scratch root");
        assert!(store.profiles_dir().is_dir());
        // Inventing the legacy directory would leave state the old app never wrote.
        assert!(!store.legacy_profiles_dir().exists());
    }

    #[test]
    fn a_write_creates_parents_and_reads_back_unchanged() {
        let (_root, store) = store();
        // No `initialize`: the write must create the directory itself.
        store
            .write_profile("Daily", r#"{"name":"Daily"}"#)
            .expect("write into a fresh root");
        assert_eq!(
            store.read_profile("Daily").expect("read back"),
            r#"{"name":"Daily"}"#
        );
        assert!(store.profile_exists("Daily"));
    }

    #[test]
    fn an_existing_file_is_replaced_whole_rather_than_appended_to() {
        let (_root, store) = store();
        store.write_profile("slot", "0123456789").expect("first");
        store.write_profile("slot", "ab").expect("second");
        assert_eq!(store.read_profile("slot").expect("read"), "ab");
    }

    #[test]
    fn an_absent_profile_is_not_found() {
        let (_root, store) = store();
        let err = store.read_profile("nope").expect_err("nothing on disk");
        assert!(matches!(err, PlatformError::NotFound { .. }), "{err:?}");
        assert!(!store.profile_exists("nope"));
    }

    #[test]
    fn a_directory_where_the_file_should_be_is_an_error_not_a_miss() {
        let (_root, store) = store();
        store.initialize().expect("initialize");
        let path = store.profile_path("trap").expect("a usable name");
        fs::create_dir(&path).expect("a directory in the file's place");

        let err = store
            .read_profile("trap")
            .expect_err("a directory is not a profile");
        assert!(
            !matches!(err, PlatformError::NotFound { .. }),
            "the file exists, it is just the wrong kind: {err:?}"
        );
        // `File.Exists` is false for a directory, and so is this.
        assert!(!store.profile_exists("trap"));

        let err = store
            .write_profile("trap", "{}")
            .expect_err("a directory cannot be replaced by a rename");
        assert!(
            !matches!(err, PlatformError::InvalidArgument { .. }),
            "{err:?}"
        );
        // A failed write must not leave a temporary behind either.
        assert!(
            file_names(store.profiles_dir())
                .iter()
                .all(|name| !name.ends_with(".tmp")),
            "temporary file left behind: {:?}",
            file_names(store.profiles_dir())
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_profile_reports_permission_denied() {
        use std::os::unix::fs::PermissionsExt;

        let (_root, store) = store();
        store.write_profile("secret", "{}").expect("write");
        let path = store.profile_path("secret").expect("a usable name");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");

        match store.read_profile("secret") {
            Ok(_) => {
                // Running as root, where mode bits do not apply: nothing to assert.
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
            Err(err) => assert!(
                matches!(err, PlatformError::PermissionDenied { .. }),
                "{err:?}"
            ),
        }
    }

    #[test]
    fn a_successful_write_leaves_exactly_one_file_and_no_temporary() {
        let (_root, store) = store();
        store.write_profile("Only", "{}").expect("write");
        assert_eq!(file_names(store.profiles_dir()), vec!["Only.json"]);
    }

    #[test]
    fn a_name_with_a_separator_cannot_escape_the_profile_directory() {
        let (_root, store) = store();
        let path = store
            .profile_path("../../escape")
            .expect("sanitises to something usable");
        // The result stays one file name inside the profile directory: the `..`
        // survives only as literal characters in that name, never as a path component.
        assert_eq!(path.parent(), Some(store.profiles_dir()));
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a file name");
        assert!(
            !file_name.contains(['/', '\\']),
            "{file_name:?} must not carry a separator"
        );
    }

    #[test]
    fn a_name_that_sanitises_away_is_an_invalid_argument() {
        let (_root, store) = store();
        for name in ["", "   ", ".", ".."] {
            assert!(store.profile_path(name).is_none(), "{name:?}");
            assert!(!store.profile_exists(name));
            assert!(matches!(
                store.read_profile(name),
                Err(PlatformError::InvalidArgument { .. })
            ));
            assert!(matches!(
                store.write_profile(name, "{}"),
                Err(PlatformError::InvalidArgument { .. })
            ));
        }
    }

    #[test]
    fn listing_is_sorted_ignoring_case_and_hides_the_auto_save_slot() {
        let (_root, store) = store();
        store.initialize().expect("initialize");
        for name in ["beta", "Alpha", "gamma"] {
            store.write_profile(name, "{}").expect("write");
        }
        store
            .write_profile(AUTO_SAVE_CONFIG_NAME, "{}")
            .expect("auto-save slot");

        assert_eq!(
            store.list_profiles().expect("list"),
            ["Alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn listing_a_missing_directory_is_empty_rather_than_an_error() {
        let (_root, store) = store();
        assert!(store.list_profiles().expect("list").is_empty());
    }

    #[test]
    fn listing_ignores_anything_that_is_not_a_json_file() {
        let (_root, store) = store();
        store.initialize().expect("initialize");
        fs::write(store.profiles_dir().join("notes.txt"), "x").expect("write");
        fs::create_dir(store.profiles_dir().join("folder.json")).expect("directory");
        assert!(store.list_profiles().expect("list").is_empty());
    }

    #[test]
    fn legacy_profiles_are_copied_once_into_an_empty_directory() {
        let (_root, store) = store();
        let legacy = store.legacy_profiles_dir();
        fs::create_dir_all(legacy).expect("legacy directory");
        fs::write(legacy.join("Old.json"), r#"{"legacy":true}"#).expect("legacy profile");
        fs::write(legacy.join("notes.txt"), "ignored").expect("non-profile");

        store.initialize().expect("initialize");
        assert_eq!(
            store.read_profile("Old").expect("migrated"),
            r#"{"legacy":true}"#
        );
        assert!(!store.profiles_dir().join("notes.txt").exists());

        // A second initialise must not push the legacy copy back over a live edit.
        store
            .write_profile("Old", r#"{"edited":true}"#)
            .expect("edit");
        store.initialize().expect("initialize again");
        assert_eq!(
            store.read_profile("Old").expect("kept"),
            r#"{"edited":true}"#
        );
    }

    #[test]
    fn migration_does_nothing_when_the_live_directory_already_has_a_profile() {
        let (_root, store) = store();
        let legacy = store.legacy_profiles_dir();
        fs::create_dir_all(legacy).expect("legacy directory");
        fs::write(legacy.join("Old.json"), "{}").expect("legacy profile");
        store.write_profile("Mine", "{}").expect("live profile");

        assert_eq!(store.migrate_legacy_profiles().expect("migrate"), 0);
        assert!(!store.profile_path("Old").expect("path").exists());
    }

    #[test]
    fn an_existing_live_file_wins_over_its_legacy_twin() {
        let (_root, store) = store();
        let legacy = store.legacy_profiles_dir();
        fs::create_dir_all(legacy).expect("legacy directory");
        fs::write(legacy.join("Same.json"), r#"{"from":"legacy"}"#).expect("legacy profile");
        fs::create_dir_all(store.profiles_dir()).expect("live directory");
        fs::write(store.profiles_dir().join("Same.json"), r#"{"from":"live"}"#)
            .expect("live profile");

        // The live directory already holds a profile, so migration is a no-op.
        assert_eq!(store.migrate_legacy_profiles().expect("migrate"), 0);
        assert_eq!(
            store.read_profile("Same").expect("read"),
            r#"{"from":"live"}"#
        );
    }

    #[test]
    fn no_legacy_directory_migrates_nothing() {
        let (_root, store) = store();
        assert_eq!(store.migrate_legacy_profiles().expect("migrate"), 0);
    }

    #[test]
    fn from_system_lands_on_an_absolute_profile_directory() {
        match ProfileStore::from_system() {
            Ok(store) => assert!(store.profiles_dir().is_absolute()),
            Err(err) => assert!(!err.user_facing().is_empty()),
        }
    }
}
