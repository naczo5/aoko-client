//! Client-only preferences: settings this client owns and the `Profile` wire
//! shape must not grow.
//!
//! Section 12 decision 3 requires an in-app animation toggle, and `Profile` is a
//! bridge wire type pinned by its own tests — adding a UI-only boolean there would
//! change what the managed side reads. The value therefore lives in its own file
//! beside the profile directory, `client-prefs.json`, and a failure to read or
//! write it is reported in the Settings card rather than dropped.
//!
//! # The write is queued, the failure is kept
//!
//! A switch press used to write this file inside the click handler, which is a `create_dir_all`,
//! a create, a flush and a rename on whatever volume the app-data directory lives on — a stall in
//! the frame the switch was pressed in. [`PrefsFile::save`] now queues the bytes on the process's
//! background writer (`crate::views::file_store::submit`) and answers `Ok` for "queued"; the
//! write itself is the same `crate::views::file_store::write_atomically` swap as before.
//!
//! The failure cannot go with a detached task, so it lands in [`PrefsFile::status`]: the card
//! calls [`PrefsFile::poll`] once a frame, and a write that failed paints the same
//! `Could not save client preferences: …` line the synchronous save returned. A success is only
//! reported after the rename, because the writer posts its outcome from behind it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aoko_platform::{app_data_directory, PlatformError};

use crate::views::file_store::{flush_writes, reply, settled, submit, Reply, WriteKind};

/// The file name inside the app-data directory.
pub const FILE_NAME: &str = "client-prefs.json";

/// What the client remembers about itself between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientPrefs {
    /// Decision 3: one switch for every animated site.
    pub animations: bool,
}

impl Default for ClientPrefs {
    fn default() -> Self {
        // The C# default is `ClientAreaAnimation = true`, and `theme::motion`'s
        // process default agrees.
        Self { animations: true }
    }
}

/// The preferences file, or the reason there is none.
#[derive(Debug, Clone)]
pub struct PrefsFile {
    path: PathBuf,
    /// Where the queued write reports, shared by every handle on this file.
    ///
    /// One mailbox for the file rather than one per save: a preference is last-write-wins, so the
    /// line the card paints is the one the last queued write reported.
    reply: Reply,
    /// The line the card shows for the last write that failed, empty while the last one landed.
    failure: String,
}

impl PrefsFile {
    /// The file inside the platform's app-data directory.
    pub fn from_system() -> Result<Self, PlatformError> {
        app_data_directory().map(|dir| Self {
            path: dir.join(FILE_NAME),
        })
    }

    /// A file at an explicit path, for tests.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            reply: reply(),
            failure: String::new(),
        }
    }

    /// Where this file lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file. A missing file is [`ClientPrefs::default`], not an error: a
    /// first run has none. A file that exists and does not parse is an error, so a
    /// corrupted preference cannot silently become "animations on".
    ///
    /// A write queued a moment ago is the newest state of this file, so the queue is drained
    /// before the read: the switch the user just pressed is what a load answers, and with nothing
    /// in flight this waits on nothing.
    pub fn load(&self) -> Result<ClientPrefs, String> {
        flush_writes(&self.path);

        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ClientPrefs::default())
            }
            Err(error) => return Err(error.to_string()),
        };

        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", self.path.display()))?;
        match value.get("animations").and_then(serde_json::Value::as_bool) {
            Some(animations) => Ok(ClientPrefs { animations }),
            None => Err(format!(
                "{} has no 'animations' boolean",
                self.path.display()
            )),
        }
    }

    /// Queues the file, creating the directory when it is the first run.
    ///
    /// The bytes go through a sibling temporary that is renamed over the file, the same swap
    /// `accounts.json`, `servers.json` and the profiles use, and it runs on the background writer
    /// so a slow volume cannot hold the frame the switch was pressed in. A plain write would
    /// truncate first, so a crash or a full disk half way through would leave a file
    /// [`PrefsFile::load`] has to reject — which the user sees as "Could not read client
    /// preferences" on the next launch and an animation setting that fell back to the default.
    ///
    /// `Ok` means the write was queued, not that the rename happened: the outcome is
    /// [`PrefsFile::poll`]'s, and a failure reaches the card through [`PrefsFile::status`]. The
    /// alternative — answering `Ok` for a swap nobody has seen — is the success-before-the-write
    /// this port exists to remove.
    ///
    /// # Errors
    /// Reserved for a request that could not be queued at all. Nothing is one today; the signature
    /// stays because the caller already handles the line.
    pub fn save(&self, prefs: ClientPrefs) -> Result<(), String> {
        let text = format!("{{\"animations\": {}}}", prefs.animations);
        submit(
            self.path.clone(),
            WriteKind::Save(text),
            Arc::clone(&self.reply),
        );
        Ok(())
    }

    /// Moves a finished write's outcome into [`PrefsFile::status`], answering whether it moved.
    ///
    /// `true` is the card's cue to repaint; `false` — the common case, with no write in flight —
    /// means there is nothing to show.
    pub fn poll(&mut self) -> bool {
        let Some(outcome) = settled(&self.reply) else {
            return false;
        };

        let next = outcome.error.map_or_else(String::new, |error| {
            format!("Could not save client preferences: {error}")
        });
        if next == self.failure {
            return false;
        }
        self.failure = next;
        true
    }

    /// The line the card paints for the last failed write, `None` while the last one landed.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        (!self.failure.is_empty()).then_some(self.failure.as_str())
    }
}

impl PartialEq for PrefsFile {
    /// Two handles on one path are the same preferences file; the queued write is the file's, not
    /// the handle's.
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for PrefsFile {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let index = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("aoko-prefs-{}-{index}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    /// Waits for the queued write and moves its outcome into the card's line.
    ///
    /// The writer reports after the swap, so this is the only ordering a test can rely on: the
    /// queue drains, then the outcome is there for [`PrefsFile::poll`] to pick up.
    fn settle(file: &mut PrefsFile) {
        flush_writes(file.path());
        file.poll();
    }

    #[test]
    fn a_first_run_has_no_file_and_takes_the_default() {
        let root = scratch();
        let file = PrefsFile::at(root.join(FILE_NAME));
        assert_eq!(file.load(), Ok(ClientPrefs::default()));
        assert!(ClientPrefs::default().animations, "the C# default is on");
        assert_eq!(file.status(), None, "nothing failed");
    }

    #[test]
    fn a_round_trip_keeps_the_animation_setting() {
        let root = scratch();
        let file = PrefsFile::at(root.join(FILE_NAME));
        file.save(ClientPrefs { animations: false }).expect("save");
        // `load` drains the queue, which is what makes a save and the next launch agree.
        assert_eq!(file.load(), Ok(ClientPrefs { animations: false }));
    }

    #[test]
    fn a_queued_save_lands_and_reports_no_failure() {
        let root = scratch();
        let mut file = PrefsFile::at(root.join(FILE_NAME));

        file.save(ClientPrefs { animations: false }).expect("save");

        settle(&mut file);
        assert_eq!(
            file.status(),
            None,
            "the write landed, so the card has nothing to say"
        );
        assert_eq!(
            fs::read_to_string(file.path()).expect("written"),
            "{\"animations\": false}"
        );
    }

    /// Two presses in quick succession: the last one is the file, and neither line is a lie.
    #[test]
    fn two_quick_presses_end_on_the_last_value() {
        let root = scratch();
        let mut file = PrefsFile::at(root.join(FILE_NAME));

        file.save(ClientPrefs { animations: false }).expect("first");
        file.save(ClientPrefs { animations: true }).expect("second");

        settle(&mut file);
        assert_eq!(file.status(), None);
        assert_eq!(
            file.load(),
            Ok(ClientPrefs { animations: true }),
            "the last commit is what the file holds"
        );
    }

    /// A write that cannot land is the card's line, not a silent loss.
    #[test]
    fn a_failed_write_is_reported_after_the_attempt() {
        let root = scratch();
        let mut file = PrefsFile::at(root.join(FILE_NAME));

        // A directory where the sibling temporary belongs fails the write before the swap, which
        // is the shape a full or read-only volume takes here.
        let mut temporary = root.join(FILE_NAME).into_os_string();
        temporary.push(".tmp");
        fs::create_dir_all(PathBuf::from(&temporary)).expect("a blocking directory");

        file.save(ClientPrefs { animations: false })
            .expect("queued");

        settle(&mut file);
        let status = file
            .status()
            .expect("a failed write is reported")
            .to_owned();
        assert!(
            status.starts_with("Could not save client preferences:"),
            "{status}"
        );
    }

    #[test]
    fn a_corrupt_file_is_an_error_not_a_default() {
        let root = scratch();
        let path = root.join(FILE_NAME);
        fs::write(&path, "not json").expect("write");
        let error = PrefsFile::at(&path).load().expect_err("corrupt");
        assert!(error.contains("not valid JSON"), "{error}");

        fs::write(&path, "{\"other\": true}").expect("write");
        let error = PrefsFile::at(&path).load().expect_err("missing key");
        assert!(error.contains("no 'animations' boolean"), "{error}");
    }

    #[test]
    fn saving_creates_the_directory() {
        let root = scratch();
        let nested = root.join("does").join("not").join("exist");
        let mut file = PrefsFile::at(nested.join(FILE_NAME));
        file.save(ClientPrefs { animations: true }).expect("save");

        settle(&mut file);
        assert_eq!(file.status(), None);
        assert!(file.path().exists());
    }

    /// A name that is not valid UTF-8 is a path here, not a conversion to lose.
    #[cfg(unix)]
    #[test]
    fn a_preference_file_at_a_non_utf8_path_saves() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = scratch();
        let mut file = PrefsFile::at(root.join(OsStr::from_bytes(b"prefs\xFF.json")));

        file.save(ClientPrefs { animations: false }).expect("save");
        settle(&mut file);

        assert_eq!(file.status(), None);
        assert_eq!(file.load(), Ok(ClientPrefs { animations: false }));
    }
}
