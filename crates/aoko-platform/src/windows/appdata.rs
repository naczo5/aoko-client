//! App-data root on Windows: `%APPDATA%`, else `%USERPROFILE%\AppData\Roaming`.
//!
//! `Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData)` is backed
//! by `SHGetKnownFolderPath(FOLDERID_RoamingAppData)`. The environment variable below
//! is what the shell exports for that same folder, and it follows shell redirection
//! in every case a user can configure through the profile UI; reading it needs no
//! COM or shell initialisation, so a headless host can resolve its data directory
//! before any window exists. That is the one deliberate difference from the managed
//! host, and `%USERPROFILE%\AppData\Roaming` covers the case where the variable is
//! missing.
//!
//! A session that sets neither has no answer, and this reports that rather than
//! defaulting to the current directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::PlatformError;

/// The roaming application-data variable.
pub const APPDATA: &str = "APPDATA";
/// Fallback source, per the documented profile layout.
pub const USERPROFILE: &str = "USERPROFILE";

/// The app-data root for this process.
///
/// # Errors
/// [`PlatformError::Os`] when neither `%APPDATA%` nor `%USERPROFILE%` names an
/// absolute directory.
pub fn app_data_directory() -> Result<PathBuf, PlatformError> {
    app_data_from_env(|name| std::env::var_os(name))
}

/// The resolution rule, over an injectable environment.
///
/// Split out so the preference order and the failure case are testable without
/// mutating the process environment (which is global, and racy under a parallel
/// test runner).
fn app_data_from_env(get: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, PlatformError> {
    if let Some(app_data) = get(APPDATA) {
        let path = Path::new(&app_data);
        // A drive-relative value (`APPDATA` unset to `\Users\...`) would produce a
        // path that depends on the current drive, so only a rooted one is accepted.
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
    }

    if let Some(profile) = get(USERPROFILE) {
        let path = Path::new(&profile);
        if path.is_absolute() {
            return Ok(path.join("AppData").join("Roaming"));
        }
    }

    Err(PlatformError::os(
        "resolve app data directory",
        "neither APPDATA nor USERPROFILE names an absolute directory",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn appdata_wins_when_rooted() {
        let resolved = app_data_from_env(env(&[
            (APPDATA, r"C:\Users\a\AppData\Roaming"),
            (USERPROFILE, r"C:\Users\a"),
        ]))
        .expect("resolvable");
        assert_eq!(resolved, PathBuf::from(r"C:\Users\a\AppData\Roaming"));
    }

    #[test]
    fn a_drive_relative_appdata_is_ignored() {
        let resolved = app_data_from_env(env(&[
            (APPDATA, r"\Users\a\AppData\Roaming"),
            (USERPROFILE, r"C:\Users\a"),
        ]))
        .expect("resolvable");
        assert_eq!(resolved, PathBuf::from(r"C:\Users\a\AppData\Roaming"));
    }

    #[test]
    fn userprofile_is_the_fallback() {
        let resolved = app_data_from_env(env(&[(USERPROFILE, r"C:\Users\a")])).expect("resolvable");
        assert_eq!(resolved, PathBuf::from(r"C:\Users\a\AppData\Roaming"));
    }

    #[test]
    fn an_empty_appdata_is_ignored() {
        let resolved = app_data_from_env(env(&[(APPDATA, ""), (USERPROFILE, r"C:\Users\a")]))
            .expect("resolvable");
        assert_eq!(resolved, PathBuf::from(r"C:\Users\a\AppData\Roaming"));
    }

    #[test]
    fn nothing_set_is_an_error_not_the_working_directory() {
        let err = app_data_from_env(env(&[])).expect_err("no location");
        assert!(matches!(err, PlatformError::Os { .. }));
        assert!(err.user_facing().contains("APPDATA"));
    }

    #[test]
    fn the_live_environment_resolves_or_reports_honestly() {
        // Wine provides `%APPDATA%`; a bare CI service might not. Either answer is
        // acceptable, a panic is not.
        match app_data_directory() {
            Ok(path) => assert!(path.is_absolute(), "{path:?} must be absolute"),
            Err(err) => assert!(!err.user_facing().is_empty()),
        }
    }
}
