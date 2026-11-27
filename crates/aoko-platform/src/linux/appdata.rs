//! App-data root on Linux: `$XDG_DATA_HOME`, else `~/.local/share`.
//!
//! This is the XDG Base Directory rule, which is what every other tool on the
//! desktop follows. `$XDG_DATA_HOME` is ignored unless it is absolute, exactly as
//! the specification requires: a relative value is worse than useless as a profile
//! directory because it moves with the working directory.
//!
//! A session that sets neither variable has no answer, and this reports that rather
//! than defaulting to the current directory.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::PlatformError;

/// Variable that overrides the data directory.
pub const XDG_DATA_HOME: &str = "XDG_DATA_HOME";
/// Variable the fallback is built from.
pub const HOME: &str = "HOME";

/// The app-data root for this process.
///
/// # Errors
/// [`PlatformError::Os`] when neither `$XDG_DATA_HOME` nor `$HOME` names a usable
/// directory for this user.
pub fn app_data_directory() -> Result<PathBuf, PlatformError> {
    app_data_from_env(|name| std::env::var_os(name))
}

/// The resolution rule, over an injectable environment.
///
/// Split out so the preference order and the failure case are testable without
/// mutating the process environment (which is global, and racy under a parallel
/// test runner).
fn app_data_from_env(get: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf, PlatformError> {
    if let Some(data_home) = get(XDG_DATA_HOME) {
        let path = Path::new(&data_home);
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
    }

    if let Some(home) = get(HOME) {
        let path = Path::new(&home);
        if path.is_absolute() {
            return Ok(path.join(".local").join("share"));
        }
    }

    Err(PlatformError::os(
        "resolve app data directory",
        "neither XDG_DATA_HOME nor HOME names an absolute directory",
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
    fn xdg_data_home_wins_when_absolute() {
        let resolved = app_data_from_env(env(&[(XDG_DATA_HOME, "/srv/data"), (HOME, "/home/a")]))
            .expect("resolvable");
        assert_eq!(resolved, PathBuf::from("/srv/data"));
    }

    #[test]
    fn a_relative_xdg_data_home_is_ignored() {
        // The XDG spec requires an absolute value; honouring a relative one would
        // move the profile directory with the working directory.
        let resolved =
            app_data_from_env(env(&[(XDG_DATA_HOME, "relative/dir"), (HOME, "/home/a")]))
                .expect("resolvable");
        assert_eq!(resolved, PathBuf::from("/home/a/.local/share"));
    }

    #[test]
    fn an_empty_xdg_data_home_is_ignored() {
        let resolved =
            app_data_from_env(env(&[(XDG_DATA_HOME, ""), (HOME, "/home/a")])).expect("resolvable");
        assert_eq!(resolved, PathBuf::from("/home/a/.local/share"));
    }

    #[test]
    fn home_is_the_fallback() {
        let resolved = app_data_from_env(env(&[(HOME, "/home/a")])).expect("resolvable");
        assert_eq!(resolved, PathBuf::from("/home/a/.local/share"));
    }

    #[test]
    fn nothing_set_is_an_error_not_the_working_directory() {
        let err = app_data_from_env(env(&[])).expect_err("no location");
        assert!(matches!(err, PlatformError::Os { .. }));
        assert!(err.user_facing().contains("XDG_DATA_HOME"));
    }

    #[test]
    fn a_relative_home_is_refused_too() {
        assert!(app_data_from_env(env(&[(HOME, "home/a")])).is_err());
    }

    #[test]
    fn the_live_environment_resolves_or_reports_honestly() {
        // Whichever this box has, the answer is a path or a classified error.
        match app_data_directory() {
            Ok(path) => assert!(path.is_absolute(), "{path:?} must be absolute"),
            Err(err) => assert!(!err.user_facing().is_empty()),
        }
    }
}
