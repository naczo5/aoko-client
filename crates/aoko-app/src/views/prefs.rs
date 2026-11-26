//! Client-only preferences: settings this client owns and the `Profile` wire
//! shape must not grow.
//!
//! Section 12 decision 3 requires an in-app animation toggle, and `Profile` is a
//! bridge wire type pinned by its own tests — adding a UI-only boolean there would
//! change what the managed side reads. The value therefore lives in its own file
//! beside the profile directory, `client-prefs.json`, and a failure to read or
//! write it is reported in the Settings card rather than dropped.

use std::fs;
use std::path::{Path, PathBuf};

use aoko_platform::{app_data_directory, PlatformError};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefsFile {
    path: PathBuf,
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
        Self { path: path.into() }
    }

    /// Where this file lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the file. A missing file is [`ClientPrefs::default`], not an error: a
    /// first run has none. A file that exists and does not parse is an error, so a
    /// corrupted preference cannot silently become "animations on".
    pub fn load(&self) -> Result<ClientPrefs, String> {
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

    /// Writes the file, creating the directory when it is the first run.
    pub fn save(&self, prefs: ClientPrefs) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text = format!("{{\"animations\": {}}}", prefs.animations);
        fs::write(&self.path, text).map_err(|error| error.to_string())
    }
}

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

    #[test]
    fn a_first_run_has_no_file_and_takes_the_default() {
        let root = scratch();
        let file = PrefsFile::at(root.join(FILE_NAME));
        assert_eq!(file.load(), Ok(ClientPrefs::default()));
        assert!(ClientPrefs::default().animations, "the C# default is on");
    }

    #[test]
    fn a_round_trip_keeps_the_animation_setting() {
        let root = scratch();
        let file = PrefsFile::at(root.join(FILE_NAME));
        file.save(ClientPrefs { animations: false }).expect("save");
        assert_eq!(file.load(), Ok(ClientPrefs { animations: false }));
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
        let file = PrefsFile::at(nested.join(FILE_NAME));
        file.save(ClientPrefs { animations: true }).expect("save");
        assert!(file.path().exists());
    }
}
