//! Opening a path in the OS file manager.
//!
//! The Settings tab has an "Open Configs Folder" affordance; this is the OS half of
//! it. The platform's launcher (`xdg-open` on Linux, `ShellExecuteW` on Windows) sits
//! behind [`PathLauncher`] so the shared rules below — a missing path is
//! [`PlatformError::NotFound`], a launcher failure is propagated unchanged — are
//! tested without launching anything.
//!
//! The seam is crate-internal on purpose: [`open_path`] is the whole public surface,
//! so a caller cannot accidentally acquire a code path that never touches the OS.

use std::path::Path;

use crate::error::PlatformError;

/// One way of handing a path to the desktop's launcher.
pub(crate) trait PathLauncher {
    /// Asks the desktop to open `path`.
    ///
    /// Implementations assume the path exists; [`open_with`] checks first so both
    /// platforms give the same answer for the same mistake.
    fn launch(&self, path: &Path) -> Result<(), PlatformError>;
}

/// Opens `path` in the platform's file manager.
///
/// A directory is revealed in the file manager; a file is opened by whatever handles
/// its type. The configs-folder button passes a directory.
///
/// # Errors
/// - [`PlatformError::NotFound`] when `path` does not exist, checked before anything
///   is launched so a folder that was deleted gets a clear message rather than a
///   launcher-specific one.
/// - [`PlatformError::Unsupported`] when the platform has no usable launcher (no
///   `xdg-open` on Linux).
/// - [`PlatformError::PermissionDenied`] / [`PlatformError::Os`] when the launcher
///   exists but refuses.
#[cfg(target_os = "linux")]
pub use crate::linux::open_path;
#[cfg(windows)]
pub use crate::windows::open_path;

/// Targets with no launcher: report instead of pretending.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn open_path(path: &Path) -> Result<(), PlatformError> {
    Err(PlatformError::unsupported(format!(
        "this target cannot open {}",
        path.display()
    )))
}

/// The rules both platforms share: the path must exist, and the launcher's verdict is
/// passed through rather than re-labelled.
pub(crate) fn open_with(path: &Path, launcher: &dyn PathLauncher) -> Result<(), PlatformError> {
    if !path.exists() {
        return Err(PlatformError::not_found(format!(
            "{} does not exist",
            path.display()
        )));
    }
    launcher.launch(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;

    /// Records what it was asked to open, and answers with a prepared verdict.
    struct FakeLauncher {
        /// `None` means "opened fine"; `Some` builds the failure to report.
        verdict: Option<fn() -> PlatformError>,
        opened: RefCell<Vec<PathBuf>>,
    }

    impl FakeLauncher {
        fn succeeding() -> Self {
            Self {
                verdict: None,
                opened: RefCell::new(Vec::new()),
            }
        }

        fn failing(verdict: fn() -> PlatformError) -> Self {
            Self {
                verdict: Some(verdict),
                opened: RefCell::new(Vec::new()),
            }
        }

        fn opened_count(&self) -> usize {
            self.opened.borrow().len()
        }
    }

    impl PathLauncher for FakeLauncher {
        fn launch(&self, path: &Path) -> Result<(), PlatformError> {
            self.opened.borrow_mut().push(path.to_path_buf());
            match self.verdict {
                Some(make) => Err(make()),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn a_missing_path_is_not_found_before_the_launcher_is_touched() {
        let launcher = FakeLauncher::succeeding();
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let missing = scratch.path().join("gone");

        let err = open_with(&missing, &launcher).expect_err("nothing to open");
        assert!(matches!(err, PlatformError::NotFound { .. }), "{err:?}");
        assert!(
            err.user_facing().contains("does not exist"),
            "{err:?} must name the problem"
        );
        assert_eq!(launcher.opened_count(), 0, "no launcher was launched");
    }

    #[test]
    fn an_existing_directory_is_handed_over_unchanged() {
        let launcher = FakeLauncher::succeeding();
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let folder = scratch.path().join("profiles");
        std::fs::create_dir(&folder).expect("create the folder");

        open_with(&folder, &launcher).expect("an existing directory opens");
        assert_eq!(launcher.opened_count(), 1);
    }

    #[test]
    fn an_existing_file_is_accepted_too() {
        // The function is generic over paths; a file is opened by its handler.
        let launcher = FakeLauncher::succeeding();
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let file = scratch.path().join("config.json");
        std::fs::write(&file, "{}").expect("write the file");

        open_with(&file, &launcher).expect("an existing file opens");
        assert_eq!(launcher.opened_count(), 1);
    }

    #[test]
    fn a_launcher_failure_is_propagated_without_relabelling() {
        let launcher =
            FakeLauncher::failing(|| PlatformError::unsupported("xdg-open is not installed"));
        let scratch = tempfile::tempdir().expect("a scratch directory");

        let err = open_with(scratch.path(), &launcher).expect_err("no launcher");
        assert!(matches!(err, PlatformError::Unsupported { .. }), "{err:?}");
        assert!(err.user_facing().contains("xdg-open"));
    }

    #[test]
    fn a_permission_denial_from_the_launcher_survives() {
        let launcher =
            FakeLauncher::failing(|| PlatformError::permission_denied("the shell refused"));
        let scratch = tempfile::tempdir().expect("a scratch directory");

        let err = open_with(scratch.path(), &launcher).expect_err("denied");
        assert!(
            matches!(err, PlatformError::PermissionDenied { .. }),
            "{err:?}"
        );
    }
}
