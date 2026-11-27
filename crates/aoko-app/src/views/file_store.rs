//! The real [`StoreBridge`]: `accounts.json` and `servers.json` under `<app data>/Aoko`.
//!
//! `stores.rs` gave the two data tabs a seam and shipped
//! [`crate::views::stores::NoStore`] behind it, so every add reached the managed failure line
//! and nothing was ever written. This is the bridge that end of the seam was waiting for: the
//! same two files, the same schemas and the same load rules as
//! `Aoko/Core/Accounts/AccountStore.cs` and `Aoko/Core/Servers/ServerStore.cs`, so an existing
//! install's `%AppData%\Aoko` keeps loading and what this store writes is what the managed app
//! would read back.
//!
//! The root is not invented here. [`aoko_platform::app_data_directory`] is the root
//! `ProfileStore` resolves (`aoko-platform/src/profile.rs:262-285`), and `Aoko` is
//! appended to it the way `get_profiles_directory` appends its own segment
//! (`aoko-protocol/src/profile.rs:1544-1546`). The two file names come from
//! [`crate::views::store_recovery`], which already owns the wording for a quarantined copy,
//! so the name this module writes and the name the notice matches cannot drift apart.
//!
//! # The two schemas
//!
//! * `accounts.json` is `StoredAccount[]` (`StoredAccount.cs:31-40`), camelCase and
//!   indented (`AccountStore.cs:20-24`). The type, the field names and the reader are
//!   `aoko_protocol::accounts`, which is the managed shape already, so this module decides
//!   only when to read and write the file. A new offline account takes the same local key
//!   (`Guid.NewGuid().ToString("N")`, `StoredAccount.cs:33`), the same UUID derivation the
//!   caller already ran through `OfflineAccount.TryCreate` (`OfflineAccount.cs:27-43`) and
//!   the same conventional `"0"` access token (`OfflineAccount.cs:18`).
//! * `servers.json` is `SavedServer[]` (`SavedServer.cs:9-16`): `id`, `name`, `address`,
//!   `addedAt`, `lastJoinedAt`. What is stored is the parsed display text, so `host` and
//!   `host:25565` stay one entry, and a blank name falls back to that address
//!   (`ServerStore.cs:132-135`). Address validation is `aoko_protocol::servers`'
//!   `ServerAddress::try_parse`, the port of the same parser the managed `Add` calls.
//!
//! # A failed write never touches the file
//!
//! Every save writes a sibling `<file>.tmp` in the same directory, flushes it, and renames
//! it over the target (`AccountStore.cs:301-335`, `ServerStore.cs:254-281`). A failure
//! half way leaves the old bytes as the file and deletes the temporary, so a crash or a
//! full disk cannot truncate saved credentials. The in-memory list is adopted only after
//! the rename succeeded: what the tabs show is always what the file holds, and a write that
//! failed is reported as a failure (`add_failed`, `delete_failed`) instead of a success.
//!
//! # A file that cannot be read is moved aside, never deleted
//!
//! An unparsable payload is renamed to `<file>.corrupt-<yyyyMMdd-HHmmss>`
//! (`AccountStore.cs:350-357`) and the store starts empty, exactly like a fresh install;
//! [`FileStore::accounts_notice`] and [`FileStore::servers_notice`] hand that path to
//! [`crate::views::store_recovery`] so the tab can say where the copy went. The rename keeps
//! the `accounts.json.corrupt-` prefix `store_recovery::find_quarantined` matches on, which
//! is why the stamp goes after the full file name rather than before the extension: the
//! notice reads the name, so the name is not this module's to spell differently.
//!
//! # Reads never panic and never lie about what they read
//!
//! A missing file, an empty one, `null`, invalid JSON, a number out of range, a row of the
//! wrong type, a file larger than `MAXIMUM_FILE_BYTES`: none of these can reach a panic
//! or a defaulted date. Rows are parsed one at a time, so one bad row is skipped while the
//! rest of the list still loads, and a row that is missing a field keeps the managed
//! property initializers' value instead of failing the file.
//!
//! # Deviations from managed, and why
//!
//! * **A re-add reuses the existing row.** `ServerStore.Add` (`:140-164`) and
//!   `AccountStore.Upsert` (`:136-159`) both match before inserting, so pressing Add twice
//!   for `play.hypixel.net` or for the same cracked name leaves one row and reports the
//!   managed success line. Refusing instead would have to answer `None`, which the tabs
//!   paint as `The server could not be saved. Try again.` for an operation that saved
//!   nothing and lost nothing. `a_re_added_offline_name_keeps_one_row` and
//!   `re_adding_one_address_keeps_one_row` pin this.
//! * **A blocked store refuses writes.** When the file exists but cannot be read (locked,
//!   denied, or a directory on the path), managed skipped the save silently and still
//!   reported the add as saved (`ServerStore.cs:109-121`, `AccountStore.cs:301-304`). Here
//!   the write answers `None`, so the tab shows its failure line, which is the rule this
//!   migration exists to keep. The file itself is never touched, and
//!   [`crate::views::store_recovery::READ_BLOCKED_MESSAGE`] explains it to the user.
//! * **An oversized file is treated as unparsable.** Managed had no ceiling and would try
//!   to materialize whatever was on disk. A file above `MAXIMUM_FILE_BYTES` is quarantined
//!   instead, which is the same outcome as corrupt JSON: the bytes are kept, the store starts
//!   empty, and no allocation is sized by a file this app could not have written.
//! * **One stamp format for both files.** `AccountStore` stamps `yyyyMMdd-HHmmss` (`:354`)
//!   and `ServerStore` stamps `yyyyMMddTHHmmssfffZ` (`:287`). One rule for both keeps the
//!   quarantine name a single implementation; `store_recovery` matches the `.corrupt-`
//!   marker, not the stamp, so nothing downstream can tell which file used which.
//! * **`MarkJoined` is not here yet.** It is the only other writer the managed store has
//!   (`ServerStore.cs:167-172`); the ping service that would call it does not exist in this
//!   crate, and [`StoreBridge`] has no method for it, so the field is read and preserved but
//!   never stamped. `MIGRATION_PARITY.md` assigned the store to `aoko-core`; this module is
//!   the `aoko-app`-side bridge over the same files, and it can be swapped for an `aoko-core`
//!   store later without either tab changing a line.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use aoko_core::SplitMix64;
use aoko_platform::app_data_directory;
use aoko_protocol::accounts::offline::{self, OFFLINE_TOKEN, SOURCE_NAME};
use aoko_protocol::accounts::StoredAccount;
use aoko_protocol::servers::ServerAddress;
use serde_json::{json, Value};

use crate::views::servers_logic::ServerRowSource;
use crate::views::store_recovery::{
    StoreNotice, ACCOUNTS_FILE_NAME, QUARANTINE_MARKER, SERVERS_FILE_NAME,
};
use crate::views::stores::StoreBridge;

/// The folder inside the app-data root that holds this client's files
/// (`ServerStore.cs:17`, `AccountStore.cs:71-73`), the same one
/// [`crate::views::palette_store::APP_DIR`] names.
pub const APP_DIR: &str = "Aoko";

/// Suffix of the sibling a save writes before the swap (`AccountStore.cs:307`,
/// `ServerStore.cs:19`).
///
/// A fixed name rather than a pid-and-sequence one, because that is the name the managed
/// store leaves behind in the worst case and the name its failure tests steer at. The
/// rename is still what makes the swap atomic, so a stale temporary can never become the
/// file.
const TEMP_SUFFIX: &str = ".tmp";

/// Ceiling on a store file's size before the payload counts as unparsable.
///
/// Eight mebibytes is about forty thousand saved accounts, which is three orders of
/// magnitude past a real list. The check runs on the file's metadata, before the read, so a
/// runaway file cannot size an allocation.
const MAXIMUM_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// One minute in milliseconds, for the two stamp formatters.
const MINUTE_MS: i64 = 60_000;
/// One hour in milliseconds.
const HOUR_MS: i64 = 3_600_000;
/// One day in milliseconds.
const DAY_MS: i64 = 86_400_000;

/// Both store files, and the bridge the two data tabs write through.
///
/// The lists are loaded once, when the store is opened, because that is what the managed
/// constructors do (`AccountStore.cs:60-69`, `ServerStore.cs:40-44`): the tabs paint from
/// memory and every mutation rewrites the whole file. [`FileStore::reload`] re-reads both
/// for the case the managed `Load` existed for, a file another process changed.
#[derive(Debug, Clone)]
pub struct FileStore {
    /// `<app data>/Aoko`, the folder both files live in.
    directory: PathBuf,
    accounts: JsonFile<StoredAccount>,
    servers: JsonFile<SavedServer>,
}

impl FileStore {
    /// Opens the store over an explicit app-data root.
    ///
    /// `root` is the platform root, not the `Aoko` folder: `Aoko` is appended here, the way
    /// `ProfileStore::new` appends its own profile segment. This is the entry point a test
    /// points at a scratch directory, and nothing is created by it: a read never writes, so
    /// the folder appears with the first save (`AccountStore.cs:65-67` created it eagerly;
    /// the write path does the same job where it is actually needed).
    #[must_use]
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let directory = root.into().join(APP_DIR);
        Self {
            accounts: JsonFile::open(directory.join(ACCOUNTS_FILE_NAME), parse_accounts),
            servers: JsonFile::open(directory.join(SERVERS_FILE_NAME), parse_servers),
            directory,
        }
    }

    /// The store in the platform's app-data directory, which is `%APPDATA%\Aoko` on Windows
    /// and `$XDG_DATA_HOME/Aoko` on Linux — the directory the profile store already uses.
    ///
    /// `None` when the session names no app-data location. There is deliberately no fallback
    /// to the working directory: a store beside the executable is lost on the next launch
    /// (`aoko-platform/src/profile.rs:262-285`).
    #[must_use]
    pub fn at_default_root() -> Option<Self> {
        app_data_directory().ok().map(Self::open)
    }

    /// The `Aoko` folder both files live in, for the tab's `Open folder` press and its
    /// storage line (`AccountStore.cs:39-40`).
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Where the saved accounts live (`AccountStore.DefaultPath`, `:75`).
    #[must_use]
    pub fn accounts_path(&self) -> &Path {
        &self.accounts.path
    }

    /// Where the saved servers live (`ServerStore.GetStorePath`, `:80-81`).
    #[must_use]
    pub fn servers_path(&self) -> &Path {
        &self.servers.path
    }

    /// What the Accounts tab should say about its file: the copy this session moved aside,
    /// or that saving is paused (`AccountStore.RecoveredFrom` / `PersistenceBlocked`,
    /// `:32-54`). Fed to [`crate::views::store_recovery::notice`].
    #[must_use]
    pub fn accounts_notice(&self) -> StoreNotice {
        self.accounts.notice()
    }

    /// What the Servers tab should say about its file (`ServerStore.cs:56-76`).
    #[must_use]
    pub fn servers_notice(&self) -> StoreNotice {
        self.servers.notice()
    }

    /// Re-reads both files (`AccountStore.Load`, `:97-116`).
    ///
    /// A read that fails for an environmental reason keeps the rows already loaded, so a
    /// transient lock cannot make the next save rewrite the file from an empty list.
    pub fn reload(&mut self) {
        self.accounts.reload(parse_accounts);
        self.servers.reload(parse_servers);
    }
}

impl StoreBridge for FileStore {
    fn accounts(&self) -> Vec<StoredAccount> {
        self.accounts.rows.clone()
    }

    /// `AddOfflineAccount` (`AccountsView.xaml.cs:649-684`) reaching
    /// `AccountStore.Upsert(profile, Offline)` (`:124-161`).
    ///
    /// The name and UUID arrive already derived, because the tab validates with
    /// `OfflineAccount.TryCreate` before it calls a store (`AccountsView.xaml.cs:662-671`).
    /// Matching is by UUID only: `Upsert`'s name arm is guarded by `!offline`
    /// (`AccountStore.cs:135-138`), so a cracked name can never take over a
    /// Microsoft- or cookie-sourced row that happens to share it.
    fn add_offline_account(&mut self, name: &str, uuid: &str) -> Option<StoredAccount> {
        let name = name.trim();
        // A blank UUID is derived from the name rather than stored: every later lookup goes
        // through `SameUuid` (`AccountStore.cs:243-248`), which refuses a blank pair, so
        // such a row could never match itself again.
        let uuid = if uuid.trim().is_empty() {
            offline::compute_offline_uuid(name).replace('-', "")
        } else {
            uuid.trim().to_owned()
        };

        let mut next = self.accounts.rows.clone();
        let existing = next
            .iter()
            .position(|account| same_uuid(&account.uuid, &uuid));

        let stored = match existing {
            Some(index) => {
                let account = &mut next[index];
                account.name = name.to_owned();
                account.uuid = uuid;
                account.access_token = OFFLINE_TOKEN.to_owned();
                // `if (incomingHasRefresh || !existing.HasRefresh) existing.Source = source;`
                // (`:152-156`): an offline add carries no refresh token, so a refresh-backed
                // row reached through the same UUID keeps the source it had.
                if !account.has_refresh() {
                    account.source = SOURCE_NAME.to_owned();
                }
                account.clone()
            }
            None => {
                let mut account = StoredAccount::new(new_local_id(), name, uuid);
                account.access_token = OFFLINE_TOKEN.to_owned();
                account.source = SOURCE_NAME.to_owned();
                // `AddedAt`'s initializer (`StoredAccount.cs:39`).
                account.added_at = Some(iso8601_utc(now_millis()));
                next.push(account.clone());
                account
            }
        };

        if !self.accounts.commit(next, accounts_json) {
            return None;
        }
        Some(stored)
    }

    /// `AccountStore.Remove` (`:230-241`), `Ordinal` on the local key.
    fn remove_account(&mut self, id: &str) -> bool {
        // Every row carries an id (`StoredAccount.cs:33`, and `parse_accounts` replaces a
        // blank one), so an empty key matches nothing and `removed` would be zero.
        if id.is_empty() {
            return false;
        }

        let mut next = self.accounts.rows.clone();
        let before = next.len();
        next.retain(|account| account.id != id);
        if next.len() == before {
            return false;
        }

        self.accounts.commit(next, accounts_json)
    }

    fn servers(&self) -> Vec<ServerRowSource> {
        self.servers.rows.iter().map(row_source).collect()
    }

    /// `ServerStore.Add` (`:128-165`).
    ///
    /// The address is parsed here even though the tab already validated it, because `Add`
    /// stores the parsed display text and the dedupe compares parsed host and port: that is
    /// what makes `play.hypixel.net` and `play.hypixel.net:25565` one bookmark.
    fn add_server(&mut self, name: Option<&str>, address: &str) -> Option<ServerRowSource> {
        let trimmed = address.trim();
        let target = ServerAddress::try_parse(Some(trimmed));
        let stored = target
            .as_ref()
            .map_or_else(|| trimmed.to_owned(), ServerAddress::display);
        let resolved_name = match name.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => stored.clone(),
        };

        let mut next = self.servers.rows.clone();
        let mut matched = None;
        for (index, existing) in next.iter().enumerate() {
            // `FindMatch` (`:190-208`), in list order and with both arms per row.
            if existing.address.eq_ignore_ascii_case(&stored) {
                matched = Some(index);
                break;
            }

            let Some(target) = target.as_ref() else {
                continue;
            };
            let Some(other) = ServerAddress::try_parse(Some(existing.address.as_str())) else {
                continue;
            };
            if other.port == target.port && other.host.eq_ignore_ascii_case(&target.host) {
                matched = Some(index);
                break;
            }
        }

        let saved = match matched {
            Some(index) => {
                let existing = &mut next[index];
                // A match keeps the bookmark the user saved: a blank name means "join this
                // address", not "rename my bookmark", and the stored address is left alone
                // so re-adding through the address box cannot rewrite it (`:153-157`).
                if name.is_some_and(|name| !name.trim().is_empty())
                    || existing.name.trim().is_empty()
                {
                    existing.name = resolved_name;
                }
                existing.clone()
            }
            None => {
                let server = SavedServer {
                    id: new_local_id(),
                    name: resolved_name,
                    address: stored,
                    // `AddedAt`'s initializer (`SavedServer.cs:14`).
                    added_at: Some(iso8601_utc(now_millis())),
                    last_joined_at: None,
                };
                next.push(server.clone());
                server
            }
        };

        if !self.servers.commit(next, servers_json) {
            return None;
        }
        Some(row_source(&saved))
    }

    /// `ServerStore.Remove` (`:174-188`), `Ordinal` on the local key.
    fn remove_server(&mut self, id: &str) -> bool {
        // `if (string.IsNullOrEmpty(id)) return false;` (`:176-177`).
        if id.is_empty() {
            return false;
        }

        let mut next = self.servers.rows.clone();
        let before = next.len();
        next.retain(|server| server.id != id);
        if next.len() == before {
            return false;
        }

        self.servers.commit(next, servers_json)
    }
}

/// One saved server row, the port of `Aoko/Core/Servers/SavedServer.cs:9-16`.
///
/// Private on purpose: the bridge hands the tab a [`ServerRowSource`], so the file's record
/// stays an implementation detail until `aoko-core` owns the store.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SavedServer {
    /// Local key, `Guid.NewGuid().ToString("N")` (`:11`).
    id: String,
    /// Saved name, or the address when the user left the box blank (`:12`).
    name: String,
    /// Address as the user saved it, canonicalised once on add (`:13`).
    address: String,
    /// `addedAt`, ISO-8601 text (`:14`).
    ///
    /// Optional where managed is not: a hand-edited row without the key keeps the managed
    /// type's initializer there, which would claim the row was added *now* and sort it to
    /// the top. Absent stays absent, and `sort_for_display` then sorts it last.
    added_at: Option<String>,
    /// `lastJoinedAt`, ISO-8601 text (`:15`).
    last_joined_at: Option<String>,
}

/// One JSON list file: where it lives, what was read from it, and how that read ended.
///
/// Both files go through this, so the atomic write, the quarantine rule and the blocked
/// gate have one implementation instead of two that can disagree.
#[derive(Debug, Clone)]
struct JsonFile<T> {
    path: PathBuf,
    /// The rows in file order; the tabs sort for display themselves.
    rows: Vec<T>,
    /// `RecoveredFrom` (`AccountStore.cs:37`, `ServerStore.cs:60`): the copy this session
    /// moved aside because it could not be parsed.
    recovered_from: Option<PathBuf>,
    /// `PersistenceBlocked` (`AccountStore.cs:47`, `ServerStore.cs:69`): the file could not
    /// be read for an environmental reason, so saving is paused rather than overwriting what
    /// could not be seen.
    blocked: bool,
    /// `_loadedOnce` (`AccountStore.cs:30`): a failed first read means memory cannot be
    /// trusted, a failed later read leaves the loaded list intact.
    loaded_once: bool,
}

impl<T> JsonFile<T> {
    /// Reads the file once, exactly like the managed constructors do
    /// (`AccountStore.cs:68`, `ServerStore.cs:43`).
    fn open(path: PathBuf, parse: fn(&str) -> Option<Vec<T>>) -> Self {
        let mut file = Self {
            path,
            rows: Vec::new(),
            recovered_from: None,
            blocked: false,
            loaded_once: false,
        };
        file.reload(parse);
        file
    }

    /// Re-reads the file, keeping what is already loaded when the read failed for a reason
    /// that says nothing about the payload (`AccountStore.cs:97-116`).
    fn reload(&mut self, parse: fn(&str) -> Option<Vec<T>>) {
        match read_json_file(&self.path, parse) {
            ReadOutcome::Rows(rows) => {
                self.rows = rows;
                self.blocked = false;
                self.loaded_once = true;
            }
            ReadOutcome::Quarantined(quarantined) => {
                self.rows = Vec::new();
                self.recovered_from = quarantined;
                self.blocked = false;
                self.loaded_once = true;
            }
            ReadOutcome::Blocked => {
                // Only a failed first read means memory cannot be trusted
                // (`AccountStore.cs:103-107`).
                if !self.loaded_once {
                    self.blocked = true;
                }
            }
        }
    }

    /// What the tab should say about this file.
    fn notice(&self) -> StoreNotice {
        StoreNotice {
            recovered_from: self
                .recovered_from
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            persistence_blocked: self.blocked,
        }
    }

    /// Writes `next` whole and adopts it only after the file really was replaced.
    ///
    /// `false` means nothing was written: either saving is paused (`AccountStore.cs:303-304`)
    /// or the swap failed. The caller turns that into the tab's failure line, and the rows in
    /// memory stay the rows on disk.
    fn commit(&mut self, next: Vec<T>, to_json: fn(&[T]) -> Option<String>) -> bool {
        if self.blocked {
            return false;
        }

        let Some(json) = to_json(&next) else {
            // A serialization failure must not become an empty file.
            tracing::warn!("{} could not be serialized", self.path.display());
            return false;
        };

        if let Err(error) = write_atomically(&self.path, &json) {
            tracing::warn!("{} could not be written: {error}", self.path.display());
            return false;
        }

        self.rows = next;
        true
    }
}

/// How one read of a store file ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadOutcome<T> {
    /// The payload parsed; rows that did not are already skipped, and a missing or `null`
    /// file is an empty list (`AccountStore.cs:262-263`, `:289-290`).
    Rows(Vec<T>),
    /// The payload did not parse. The file was moved aside to this path, or left where it is
    /// when the rename itself failed (`AccountStore.cs:350-363`).
    Quarantined(Option<PathBuf>),
    /// The file exists but could not be read (locked, denied, or a directory on the path), so
    /// the caller keeps what it already had (`AccountStore.cs:259-274`).
    Blocked,
}

/// Reads one list file, deciding between "parsed", "unparsable" and "unreadable".
fn read_json_file<T>(path: &Path, parse: fn(&str) -> Option<Vec<T>>) -> ReadOutcome<T> {
    // `if (Directory.Exists(_path)) return false;` (`AccountStore.cs:259-260`,
    // `ServerStore.cs:215-216`): a directory where the file belongs is a read failure, not an
    // empty list.
    if path.is_dir() {
        return ReadOutcome::Blocked;
    }

    // The ceiling is checked on the metadata, before anything is read.
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAXIMUM_FILE_BYTES) {
        return ReadOutcome::Quarantined(quarantine(path));
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        // A first launch has no file (`AccountStore.cs:262-263`).
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ReadOutcome::Rows(Vec::new())
        }
        // Locked, denied, unreadable: `PersistenceBlocked`.
        Err(_) => return ReadOutcome::Blocked,
    };

    // `File.ReadAllText` decodes with a replacement fallback, so invalid bytes become U+FFFD
    // and the parse decides whether the document is usable
    // (`aoko-platform/src/profile.rs:287-298`).
    let text = String::from_utf8_lossy(&bytes);

    match parse(text.as_ref()) {
        Some(rows) => ReadOutcome::Rows(rows),
        // Unreadable payload: keep the bytes for the user instead of discarding saved
        // credentials (`AccountStore.cs:276-287`).
        None => ReadOutcome::Quarantined(quarantine(path)),
    }
}

/// Moves an unreadable file aside and reports where it went.
///
/// The stamp is UTC `yyyyMMdd-HHmmss` (`AccountStore.cs:354`) and the marker is
/// [`crate::views::store_recovery::QUARANTINE_MARKER`], so the resulting name is
/// `accounts.json.corrupt-20240501-120000`: the shape `find_quarantined` matches and the
/// notice can name. A rename that fails leaves the file exactly where it was, which is the
/// managed fallback too (`:359-363`): the store starts empty either way.
///
/// Shared with [`crate::views::profiles`], which quarantines an auto-save slot that will not
/// parse: one implementation of the name, so a copy of either file is stamped the same way and
/// [`crate::views::store_recovery`] can describe both.
pub(crate) fn quarantine(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?;
    let mut quarantined = name.to_os_string();
    quarantined.push(QUARANTINE_MARKER);
    quarantined.push(quarantine_stamp(now_millis()));

    let target = path.with_file_name(quarantined);
    fs::rename(path, &target).ok().map(|()| target)
}

/// Writes `contents` through a sibling temporary and renames it over `path`.
///
/// The rename is the atomic step: either the old file or the complete new one is visible, and
/// a failure half way leaves the old one untouched (`AccountStore.cs:301-335`). Rust's
/// `fs::rename` replaces an existing destination on both platforms, so the managed
/// `File.Replace`-then-`File.Move` fallback (`:312-327`) has nothing left to do here.
///
/// Shared with [`crate::views::prefs`], whose `client-prefs.json` is written the same way: one
/// implementation of the swap, so a full disk can truncate none of the three files.
pub(crate) fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    // `<app data>/Aoko` appears with the first save (the managed `AccountStore` created it in
    // its constructor, `AccountStore.cs:65-67`); an empty parent is the working directory,
    // which needs no creating.
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(directory) = parent {
        fs::create_dir_all(directory)?;
    }

    let temporary = temporary_path(path);
    let written = (|| -> io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(contents.as_bytes())?;
        // Only a flushed file makes the bytes durable; a crash between an unsynced rename
        // and the write-back would otherwise leave an empty store where a good one was.
        file.sync_all()?;
        // Windows cannot rename a file that is still open.
        drop(file);
        fs::rename(&temporary, path)
    })();

    if written.is_err() {
        // A failed save must not leave the temporary behind for the next one to trip over
        // (`AccountStore.cs:329-334`).
        let _ = fs::remove_file(&temporary);
    }

    written
}

/// The sibling a write goes through before the swap: `<file>.tmp` (`AccountStore.cs:307`).
fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(TEMP_SUFFIX);
    PathBuf::from(temporary)
}

/// Parses `accounts.json`, keeping every row that parses.
///
/// `aoko_protocol::accounts::StoredAccount` is the managed shape field for field, so the row
/// itself needs no translation; what this function adds is the tolerance the brief requires.
/// `serde_json::from_value` per row means one wrong-typed or unparsable row is skipped instead
/// of failing the file, where `JsonSerializer.Deserialize<List<StoredAccount>>` threw and the
/// whole list was quarantined (`AccountStore.cs:276-287`).
fn parse_accounts(text: &str) -> Option<Vec<StoredAccount>> {
    match serde_json::from_str::<Value>(text).ok()? {
        // `loaded == null` is "no accounts", not a corrupt file (`:289-290`).
        Value::Null => Some(Vec::new()),
        Value::Array(items) => Some(
            items
                .into_iter()
                .filter_map(|item| {
                    let mut account = serde_json::from_value::<StoredAccount>(item).ok()?;
                    if account.id.trim().is_empty() {
                        // The managed initializer gives a row without an id a fresh local
                        // key (`StoredAccount.cs:33`), which is what keeps it addressable
                        // for `Remove`.
                        account.id = new_local_id();
                    }
                    Some(account)
                })
                .collect(),
        ),
        // An object, a bare string or a number is not a list of accounts.
        _ => None,
    }
}

/// Parses `servers.json`, keeping every row that parses.
fn parse_servers(text: &str) -> Option<Vec<SavedServer>> {
    match serde_json::from_str::<Value>(text).ok()? {
        Value::Null => Some(Vec::new()),
        Value::Array(items) => Some(items.iter().filter_map(server_from_value).collect()),
        _ => None,
    }
}

/// One `servers.json` row, or `None` when it is not a row this store can use.
///
/// A field the managed model says is a string has to be a string: a number or an object there
/// is a row written by something that is not this client, and guessing would put a
/// meaningless address in front of the user. An absent or `null` `id`/`name`/`address` takes
/// the managed initializer's value (`ServerStore.cs:231-233`).
fn server_from_value(value: &Value) -> Option<SavedServer> {
    if !value.is_object() {
        return None;
    }

    let text = |key: &str| -> Option<Option<String>> {
        match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(text)) => Some(Some(text.clone())),
            Some(_) => None,
        }
    };

    let mut server = SavedServer {
        id: text("id")?.unwrap_or_default(),
        name: text("name")?.unwrap_or_default(),
        address: text("address")?.unwrap_or_default(),
        added_at: text("addedAt")?,
        last_joined_at: text("lastJoinedAt")?,
    };

    if server.id.trim().is_empty() {
        // `server.Id = IsNullOrWhiteSpace(server.Id) ? Guid.NewGuid().ToString("N") : server.Id`
        // (`ServerStore.cs:231`).
        server.id = new_local_id();
    }

    Some(server)
}

/// Serialises the account list the way `AccountStore` does: camelCase and indented
/// (`AccountStore.cs:20-24`), with the field renames `StoredAccount` already carries.
fn accounts_json(accounts: &[StoredAccount]) -> Option<String> {
    serde_json::to_string_pretty(accounts).ok()
}

/// Serialises the server list: camelCase keys in the managed declaration order
/// (`SavedServer.cs:11-15`).
fn servers_json(servers: &[SavedServer]) -> Option<String> {
    let rows: Vec<Value> = servers
        .iter()
        .map(|server| {
            json!({
                "id": server.id,
                "name": server.name,
                "address": server.address,
                "addedAt": server.added_at,
                "lastJoinedAt": server.last_joined_at,
            })
        })
        .collect();

    serde_json::to_string_pretty(&rows).ok()
}

/// The store record as the row the tab paints.
fn row_source(server: &SavedServer) -> ServerRowSource {
    ServerRowSource {
        id: server.id.clone(),
        name: server.name.clone(),
        address: server.address.clone(),
        added_at: server.added_at.clone(),
        last_joined_at: server.last_joined_at.clone(),
    }
}

/// A fresh local key: `Guid.NewGuid().ToString("N")` (`StoredAccount.cs:33`,
/// `SavedServer.cs:11`), which is 32 lowercase hex digits and no dashes.
///
/// No `uuid` or `rand` dependency is reachable from this crate, and the key never leaves the
/// app, so mixing the clock with a per-call sequence through the workspace's own
/// [`SplitMix64`] is enough: two ids would have to share both the nanosecond and the counter
/// value to collide.
fn new_local_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
    let seed = nanos ^ (u64::from(std::process::id()) << 32) ^ sequence.rotate_left(17);

    let mut rng = SplitMix64::new(seed);
    format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64())
}

/// `AccountStore.SameUuid` (`:243-248`): two spellings of the same profile, with or without
/// dashes and in either case, are the same account.
fn same_uuid(left: &str, right: &str) -> bool {
    !left.trim().is_empty()
        && !right.trim().is_empty()
        && left.replace('-', "").to_lowercase() == right.replace('-', "").to_lowercase()
}

/// Milliseconds since the Unix epoch, for the three stamps this module writes.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// The round-trip form `System.Text.Json` writes for a `DateTimeOffset`:
/// `yyyy-MM-ddTHH:mm:ss.fffffff+00:00`.
///
/// The shape matters more than the digits: a stamp this store writes has to parse with the
/// managed reader and with [`crate::views::relative_time::parse_iso8601_utc_millis`], which
/// the card's `Last used` and `Last joined` lines read. The system clock stops at
/// milliseconds, so the last four fraction digits are zeros rather than an invented value.
fn iso8601_utc(millis: i64) -> String {
    let (days, rest) = (millis.div_euclid(DAY_MS), millis.rem_euclid(DAY_MS));
    let (year, month, day) = civil_from_days(days);
    let hour = rest / HOUR_MS;
    let minute = (rest % HOUR_MS) / MINUTE_MS;
    let second = (rest % MINUTE_MS) / 1000;
    let fraction = rest % 1000;

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:03}0000+00:00"
    )
}

/// The UTC `yyyyMMdd-HHmmss` stamp inside a quarantine name (`AccountStore.cs:354`).
fn quarantine_stamp(millis: i64) -> String {
    let (days, rest) = (millis.div_euclid(DAY_MS), millis.rem_euclid(DAY_MS));
    let (year, month, day) = civil_from_days(days);
    let hour = rest / HOUR_MS;
    let minute = (rest % HOUR_MS) / MINUTE_MS;
    let second = (rest % MINUTE_MS) / 1000;

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

/// The date half of a Unix timestamp (Howard Hinnant's `civil_from_days`), the inverse of
/// `relative_time::days_from_civil`, so no date crate is pulled into a UI-only layer.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };

    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::views::relative_time;
    use std::sync::atomic::AtomicUsize;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// An app-data root of its own per test, the shape `ServerStoreTests.TempStore` uses.
    fn scratch() -> PathBuf {
        let index = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("aoko-file-store-{}-{index}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch root");
        path
    }

    /// A store over a fresh root, plus that root, so a test can reopen the same files.
    fn store() -> (PathBuf, FileStore) {
        let root = scratch();
        let store = FileStore::open(root.clone());
        (root, store)
    }

    /// Every `*.corrupt-*` copy of one store file, sorted for a stable assertion.
    fn quarantined(directory: &Path, file_name: &str) -> Vec<PathBuf> {
        let prefix = format!("{file_name}{QUARANTINE_MARKER}");
        let mut found: Vec<PathBuf> = fs::read_dir(directory)
            .expect("the store directory")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn an_offline_account_round_trips_through_the_managed_schema() {
        let (root, mut store) = store();

        let added = store
            .add_offline_account("Notch", "069a79f444e94726a5befca90e38aaf5")
            .expect("the first write succeeds");
        assert_eq!(added.name, "Notch");
        assert_eq!(added.uuid, "069a79f444e94726a5befca90e38aaf5");
        assert_eq!(added.id.len(), 32, "`Guid.NewGuid().ToString(\"N\")`");
        assert!(
            added
                .id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "lowercase hex, like `Guid.NewGuid().ToString(\"N\")`"
        );
        assert_eq!(added.source, SOURCE_NAME);
        assert_eq!(added.access_token, OFFLINE_TOKEN);
        assert!(added.added_at.is_some(), "`AddedAt` is stamped on creation");

        let text = fs::read_to_string(store.accounts_path()).expect("the file is written");
        for key in [
            "\"id\"",
            "\"name\"",
            "\"uuid\"",
            "\"accessToken\"",
            "\"refreshToken\"",
            "\"source\"",
            "\"addedAt\"",
            "\"lastUsedAt\"",
        ] {
            assert!(text.contains(key), "{key} is missing from {text}");
        }

        // What the next launch does: a second store over the same root.
        let reopened = FileStore::open(root);
        assert_eq!(reopened.accounts(), vec![added]);
    }

    #[test]
    fn a_server_round_trips_and_keeps_the_display_address() {
        let (root, mut store) = store();

        let saved = store
            .add_server(Some("Alpha"), "alpha.example.com:25566")
            .expect("the first write succeeds");
        assert_eq!(saved.name, "Alpha");
        assert_eq!(saved.address, "alpha.example.com:25566");
        assert_eq!(saved.id.len(), 32);
        assert!(saved.added_at.is_some());
        assert!(saved.last_joined_at.is_none());

        let text = fs::read_to_string(store.servers_path()).expect("the file is written");
        for key in [
            "\"id\"",
            "\"name\"",
            "\"address\"",
            "\"addedAt\"",
            "\"lastJoinedAt\"",
        ] {
            assert!(text.contains(key), "{key} is missing from {text}");
        }
        assert!(
            !temporary_path(store.servers_path()).exists(),
            "a successful save leaves no temporary behind"
        );

        let reopened = FileStore::open(root);
        assert_eq!(reopened.servers(), vec![saved]);
    }

    #[test]
    fn a_blank_server_name_falls_back_to_the_address() {
        let (_root, mut store) = store();

        let saved = store
            .add_server(None, " play.example.net ")
            .expect("a valid address saves");

        assert_eq!(saved.name, "play.example.net", "`resolvedName` (`:135`)");
        assert_eq!(saved.address, "play.example.net", "the text is trimmed");
    }

    #[test]
    fn re_adding_one_address_keeps_one_row() {
        let (_root, mut store) = store();

        let first = store
            .add_server(None, "play.hypixel.net")
            .expect("the first add saves");
        assert_eq!(first.name, "play.hypixel.net");

        // `Add_DedupesByParsedHostAndPort`: a named re-add renames, `host:25565` does not.
        let named = store
            .add_server(Some("Fancy"), "play.hypixel.net")
            .expect("a duplicate is still a save");
        assert_eq!(named.id, first.id);
        assert_eq!(named.name, "Fancy");
        assert_eq!(store.servers().len(), 1);

        let same_port = store
            .add_server(None, "play.hypixel.net:25565")
            .expect("the same host and port is the same bookmark");
        assert_eq!(same_port.id, first.id);
        assert_eq!(same_port.name, "Fancy", "a blank name never renames");
        assert_eq!(
            same_port.address, "play.hypixel.net",
            "nor rewrites the address"
        );
        assert_eq!(store.servers().len(), 1);

        let other = store
            .add_server(Some("Other"), "mc.example.com:25566")
            .expect("a different port is a different server");
        assert_ne!(other.id, first.id);
        assert_eq!(store.servers().len(), 2);
    }

    #[test]
    fn a_re_added_offline_name_keeps_one_row() {
        let (_root, mut store) = store();

        let first = store
            .add_offline_account("Notch", "069a79f444e94726a5befca90e38aaf5")
            .expect("the first add saves");
        let stamped = first.added_at.clone();

        // `Upsert` matches by UUID (`:136-138`), and the tab computes that UUID from the
        // name, so pressing Add twice is one row with the original `addedAt`.
        let again = store
            .add_offline_account("Notch", "069a79f4-44e9-4726-a5be-fca90e38aaf5")
            .expect("a duplicate is still a save");

        assert_eq!(again.id, first.id);
        assert_eq!(
            again.uuid, "069a79f4-44e9-4726-a5be-fca90e38aaf5",
            "the row takes the incoming form of the uuid, which is what `Upsert` writes back \
             (`AccountStore.cs:150`); the match is what ignored the dashes, not the value"
        );
        assert_eq!(again.added_at, stamped);
        assert_eq!(store.accounts().len(), 1);
    }

    #[test]
    fn a_corrupt_accounts_file_is_moved_aside_not_deleted() {
        let (root, mut store) = store();
        store
            .add_offline_account("Notch", "")
            .expect("the store starts working");
        let path = store.accounts_path().to_path_buf();
        fs::write(&path, "{ this is not json").expect("corrupting the file");

        let mut reopened = FileStore::open(root.clone());

        assert!(
            reopened.accounts().is_empty(),
            "a recovered store looks like a fresh install"
        );
        assert!(!path.exists(), "the unreadable file is moved, not deleted");

        let kept = quarantined(reopened.directory(), ACCOUNTS_FILE_NAME);
        assert_eq!(kept.len(), 1, "exactly one copy was made");
        assert_eq!(
            fs::read_to_string(&kept[0]).expect("the copy is readable"),
            "{ this is not json",
            "the bytes are kept for the user"
        );
        assert_eq!(
            reopened.accounts_notice().recovered_from,
            Some(kept[0].to_string_lossy().into_owned())
        );
        assert!(!reopened.accounts_notice().persistence_blocked);

        // The recovered store is writable: the copy is not the live file.
        assert!(reopened.add_offline_account("Notch", "").is_some());
        assert_eq!(FileStore::open(root).accounts().len(), 1);
    }

    #[test]
    fn a_missing_file_reads_as_empty_and_creates_nothing() {
        let (_root, mut store) = store();

        assert!(store.accounts().is_empty());
        assert!(store.servers().is_empty());
        assert!(
            !store.accounts_path().exists(),
            "reading must not create the file"
        );
        assert!(!store.servers_path().exists());
        assert!(!store.directory().exists());
        assert!(!store.remove_account("missing"));
        assert!(!store.remove_server("missing"));
        assert!(!store.accounts_notice().persistence_blocked);
    }

    #[test]
    fn an_existing_install_file_keeps_loading() {
        let (root, store) = store();
        fs::create_dir_all(store.directory()).expect("the store directory");
        // The exact text `AccountStore` writes: camelCase keys, two-space indent, and a
        // round-trip stamp with seven fraction digits and `+00:00`.
        let text = r#"[
  {
    "id": "5b2a1f6a4b1f4c0e8a2d9f3b7c6e5d40",
    "name": "Notch",
    "uuid": "069a79f444e94726a5befca90e38aaf5",
    "accessToken": "0",
    "refreshToken": "",
    "source": "offline",
    "addedAt": "2024-05-01T00:00:00.1234567+00:00",
    "lastUsedAt": null
  }
]"#;
        fs::write(store.accounts_path(), text).expect("a seeded file");

        let reopened = FileStore::open(root);
        let accounts = reopened.accounts();
        let account = accounts.first().expect("the row loads");
        assert_eq!(account.id, "5b2a1f6a4b1f4c0e8a2d9f3b7c6e5d40");
        assert_eq!(account.name, "Notch");
        assert_eq!(account.source, SOURCE_NAME);
        assert_eq!(account.access_token, "0");
        assert!(account.last_used_at.is_none());
        let stamp = account.added_at.as_deref().expect("a stamp");
        assert_eq!(
            relative_time::parse_iso8601_utc_millis(stamp),
            Some(1_714_521_600_123),
            "the stored stamp still parses with the card's reader"
        );
    }

    #[test]
    fn one_bad_row_is_skipped_while_the_rest_load() {
        let (root, store) = store();
        fs::create_dir_all(store.directory()).expect("the store directory");
        let text = r#"[
  {"id": 5, "name": "a number where the id belongs"},
  {"name": "NoId"},
  null,
  7,
  {"id": "kept", "name": "Kept", "uuid": "aa", "source": "offline"}
]"#;
        fs::write(store.accounts_path(), text).expect("a hand-edited file");

        let accounts = FileStore::open(root).accounts();

        assert_eq!(accounts.len(), 2, "the two usable rows survive");
        assert_eq!(accounts[0].name, "NoId");
        assert_eq!(accounts[0].id.len(), 32, "a missing id gets a local key");
        assert_eq!(
            accounts[0].source, "unknown",
            "and the managed default source"
        );
        assert_eq!(accounts[1].id, "kept");
        assert_eq!(accounts[1].name, "Kept");
    }

    #[test]
    fn a_servers_row_that_is_not_a_row_is_skipped() {
        let (root, store) = store();
        fs::create_dir_all(store.directory()).expect("the store directory");
        let text = r#"[
  {"id": "  ", "name": "", "address": "blank.example.net"},
  {"id": "wrong", "name": 5, "address": "typed.example.net"},
  "not a row",
  {"id": "kept", "name": "Kept", "address": "kept.example.net",
   "addedAt": "2024-05-01T00:00:00.0000000+00:00", "lastJoinedAt": null}
]"#;
        fs::write(store.servers_path(), text).expect("a hand-edited file");

        let servers = FileStore::open(root).servers();

        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].address, "blank.example.net");
        assert_eq!(servers[0].id.len(), 32, "`ServerStore.cs:231`");
        assert!(
            servers[0].added_at.is_none(),
            "an absent stamp stays absent"
        );
        assert_eq!(servers[1].name, "Kept");
    }

    #[test]
    fn payloads_that_are_not_lists_are_quarantined_and_never_panic() {
        let (root, store) = store();
        fs::create_dir_all(store.directory()).expect("the store directory");

        // `NaN` is not JSON, so the whole document is unparsable rather than one bad row.
        for payload in ["[{\"id\": NaN}]", "", "{\"accounts\": []}", "1e999"] {
            let mut reopened = FileStore::open(root.clone());
            fs::write(reopened.accounts_path(), payload).expect("a hostile file");

            reopened.reload();
            assert!(
                reopened.accounts().is_empty(),
                "{payload:?} loads as nothing"
            );
            assert!(
                !reopened.accounts_path().exists(),
                "{payload:?} is moved aside"
            );
            assert!(reopened.accounts_notice().recovered_from.is_some());
        }

        // `null` is the one payload that is not a corrupt file (`:289-290`).
        fs::write(store.accounts_path(), "null").expect("a null file");
        let reopened = FileStore::open(root);
        assert!(reopened.accounts().is_empty());
        assert!(
            reopened.accounts_path().exists(),
            "a null file is left alone"
        );
        assert!(reopened.accounts_notice().recovered_from.is_none());
    }

    #[test]
    fn a_failed_write_leaves_the_old_contents_intact() {
        let (root, mut store) = store();
        store
            .add_server(Some("Alpha"), "alpha.example.com:25566")
            .expect("the first write succeeds");
        let before = fs::read_to_string(store.servers_path()).expect("the file exists");

        // A directory where the temporary belongs makes the write fail before the swap,
        // the way `ServerStoreTests.Save_KeepsThePreviousFileWhenTheWriteFails` does it.
        fs::create_dir_all(temporary_path(store.servers_path())).expect("a blocking directory");

        assert!(
            store.add_server(Some("Beta"), "beta.example.com").is_none(),
            "a failed write is not a saved server"
        );
        assert_eq!(
            fs::read_to_string(store.servers_path()).expect("the old file survives"),
            before,
            "the file is not truncated"
        );
        assert_eq!(
            store.servers().len(),
            1,
            "the list in memory stays the list on disk"
        );
        assert_eq!(FileStore::open(root).servers().len(), 1);
    }

    #[test]
    fn a_blocked_store_refuses_writes_and_keeps_the_file() {
        let (root, mut store) = store();
        store
            .add_offline_account("Notch", "")
            .expect("the store starts working");

        // `Directory.Exists(_path)` (`AccountStore.cs:259-260`): a directory on the store path
        // is the managed "locked or denied" case, and it fails the write for every user.
        fs::remove_file(store.accounts_path()).expect("replacing the file with a directory");
        fs::create_dir_all(store.accounts_path()).expect("a directory on the path");
        fs::write(store.accounts_path().join("keep.txt"), "user data").expect("a file inside");

        let mut reopened = FileStore::open(root);

        assert!(reopened.accounts().is_empty());
        assert!(
            reopened.accounts_notice().persistence_blocked,
            "`PersistenceBlocked` (`:47`)"
        );
        assert!(
            reopened.add_offline_account("Notch", "").is_none(),
            "a blocked store never overwrites what it could not read"
        );
        assert!(!reopened.remove_account("anything"));
        assert_eq!(
            fs::read_to_string(reopened.accounts_path().join("keep.txt")).expect("still there"),
            "user data"
        );
    }

    #[test]
    fn the_stamps_are_the_managed_round_trip_form() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00.0000000+00:00");
        assert_eq!(
            iso8601_utc(1_700_000_000_123),
            "2023-11-14T22:13:20.1230000+00:00"
        );
        assert_eq!(
            relative_time::parse_iso8601_utc_millis(&iso8601_utc(1_700_000_000_123)),
            Some(1_700_000_000_123),
            "the card's reader agrees with the writer"
        );

        assert_eq!(quarantine_stamp(0), "19700101-000000");
        assert_eq!(quarantine_stamp(1_714_521_600_000), "20240501-000000");
        assert_eq!(civil_from_days(-1), (1969, 12, 31), "before the epoch");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn the_uuid_match_ignores_dashes_and_case() {
        let dashed = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        let compact = "069A79F444E94726A5BEFCA90E38AAF5";

        assert!(
            same_uuid(compact, dashed),
            "dashes and case do not distinguish"
        );
        assert!(!same_uuid("", ""));
        assert!(!same_uuid("aa", "bb"));
    }
}
