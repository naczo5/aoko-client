//! Handing a path or a URL to the OS launcher.
//!
//! Two entry points, one mechanism: [`open_path`] for the Settings tab's "Open
//! Configs Folder" control, [`open_url`] for the update control's press, which opens
//! the releases page because this host ships no HTTP client. The platform's launcher
//! (`xdg-open` on Linux, `ShellExecuteW` on Windows) sits behind [`PathLauncher`] and
//! [`UrlLauncher`], so the shared rules — a missing path is
//! [`PlatformError::NotFound`], a non-`https` URL is [`PlatformError::InvalidArgument`],
//! a launcher failure is propagated unchanged — are tested without launching anything.
//!
//! The seams are crate-internal on purpose: the two functions are the whole public
//! surface, so a caller cannot accidentally acquire a code path that never touches the
//! OS, or hand the shell a scheme this host would not open itself.

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

/// One way of handing a web address to the same launcher.
///
/// Separate from [`PathLauncher`] rather than widening it: a URL is not a path, and
/// making `open_path` accept one would drop the existence check that is the whole
/// point of that function. Implementations assume the scheme was already checked by
/// [`open_url_with`], so both platforms refuse the same strings.
pub(crate) trait UrlLauncher {
    /// Asks the desktop to open `url`, which starts with `https://`.
    fn launch_url(&self, url: &str) -> Result<(), PlatformError>;
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

/// Opens `url` in the user's browser.
///
/// This is the update control's press: the host ships no HTTP client, so a new
/// release is announced as a link and the OS opens it. The launcher is the same one
/// [`open_path`] uses - `ShellExecuteW(open, …)` on Windows, `xdg-open` on Linux -
/// because both take a URL as readily as a file.
///
/// Only `https://` is accepted. That check is the reason this is a second entry point
/// rather than a URL passed to [`open_path`]: the shell treats whatever it is given as
/// a target, so `file:`, `javascript:` and a bare path would all reach it with the
/// same trust as a web address. `http://` is refused too, deliberately, because the
/// one caller has no reason to downgrade the link it just built.
///
/// Reachability is not checked and not implied: whether the address resolves, whether
/// a browser is registered and whether the user is online are all the desktop's
/// business, and failure to open is reported, not guessed at.
///
/// # Errors
/// - [`PlatformError::InvalidArgument`] when `url` is not an `https://` address,
///   naming the string it refused.
/// - [`PlatformError::Unsupported`] when the platform has no usable launcher (no
///   `xdg-open` on Linux).
/// - [`PlatformError::PermissionDenied`] / [`PlatformError::Os`] when the launcher
///   exists but refuses.
#[cfg(target_os = "linux")]
pub use crate::linux::open_url;
#[cfg(windows)]
pub use crate::windows::open_url;

/// Targets with no launcher: report instead of pretending.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn open_url(url: &str) -> Result<(), PlatformError> {
    Err(PlatformError::unsupported(format!(
        "this target cannot open {url}"
    )))
}

/// The rules both platforms share: the scheme is checked before anything is launched,
/// and the launcher's verdict is passed through rather than re-labelled.
pub(crate) fn open_url_with(url: &str, launcher: &dyn UrlLauncher) -> Result<(), PlatformError> {
    check_url(url)?;
    launcher.launch_url(url)
}

/// Accepts only an `https://` address with a host.
///
/// Refusing here rather than in each backend is what makes the two platforms agree on
/// what they will open, and it is the only guard that matters: after this, the string
/// goes straight to a shell that will act on any scheme it understands.
fn check_url(url: &str) -> Result<(), PlatformError> {
    let refused = |reason: &str| {
        // `Display` inside explicit quotes, not `{:?}`: a detail is read by a user, and
        // Debug renders a Windows path with doubled separators, which is the runtime's
        // representation leaking into the sentence rather than the string the caller
        // passed. Paths elsewhere in this crate use `display()` for the same reason.
        PlatformError::invalid_argument(format!("refusing to open \"{url}\": {reason}"))
    };

    let Some((scheme, host)) = url.split_once("://") else {
        return Err(refused("it is not an absolute URL"));
    };
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(refused("only https:// URLs are handed to the launcher"));
    }
    if host.trim().is_empty() {
        return Err(refused("there is no host after the scheme"));
    }
    // A literal control character would truncate the Windows wide string at the NUL or
    // split the argument on a newline, and a literal space splits it too; neither
    // belongs in a URL, where they are percent-encoded.
    if url
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(refused("it contains a control character or whitespace"));
    }
    Ok(())
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

    /// Records the URLs it was asked to open, and answers with a prepared verdict.
    struct FakeUrlLauncher {
        verdict: Option<fn() -> PlatformError>,
        opened: RefCell<Vec<String>>,
    }

    impl FakeUrlLauncher {
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

        fn opened(&self) -> Vec<String> {
            self.opened.borrow().clone()
        }
    }

    impl UrlLauncher for FakeUrlLauncher {
        fn launch_url(&self, url: &str) -> Result<(), PlatformError> {
            self.opened.borrow_mut().push(url.to_string());
            match self.verdict {
                Some(make) => Err(make()),
                None => Ok(()),
            }
        }
    }

    #[test]
    fn a_refused_scheme_never_reaches_the_launcher() {
        // The property that matters: none of these can reach a shell that would act on
        // it. `file:` and a bare path are the reason `open_path` could not serve this,
        // and `http:` is refused deliberately rather than downgraded.
        let launcher = FakeUrlLauncher::succeeding();
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "/etc/passwd",
            r"C:\Windows\System32\calc.exe",
            "ftp://example.com/releases",
            "http://example.com/releases",
            "mailto:dev@example.com",
            "https://",
            "https://   ",
            "://example.com",
            "https:example.com",
        ] {
            let err = open_url_with(url, &launcher).expect_err("must be refused");
            assert!(
                matches!(&err, PlatformError::InvalidArgument { .. }),
                "{url}: {err:?}"
            );
            assert!(
                err.user_facing().contains(url),
                "{url} must be named in what it refused: {err:?}"
            );
        }
        assert!(
            launcher.opened().is_empty(),
            "the launcher was reached: {:?}",
            launcher.opened()
        );
    }

    #[test]
    fn an_accepted_url_reaches_the_launcher_unchanged() {
        let launcher = FakeUrlLauncher::succeeding();
        let url = "https://github.com/naczo5/aoko-client/releases";

        open_url_with(url, &launcher).expect("an https URL opens");

        assert_eq!(launcher.opened(), vec![url.to_string()]);
    }

    #[test]
    fn the_scheme_is_matched_without_regard_to_case_and_is_not_rewritten() {
        let launcher = FakeUrlLauncher::succeeding();
        let url = "HTTPS://Example.COM/Releases";

        open_url_with(url, &launcher).expect("HTTPS is still https");

        assert_eq!(
            launcher.opened(),
            vec![url.to_string()],
            "the string is passed through as given"
        );
    }

    #[test]
    fn a_control_character_or_space_is_refused_before_the_launcher() {
        // A NUL would truncate the Windows wide string and a newline or space would
        // split the argument: the value handed to a shell has to be inert.
        let launcher = FakeUrlLauncher::succeeding();
        for url in [
            "https://example.com/\u{0}evil",
            "https://example.com/\r\nHost: evil",
            "https://example.com/a b",
        ] {
            let err = open_url_with(url, &launcher).expect_err("must be refused");
            assert!(
                matches!(&err, PlatformError::InvalidArgument { .. }),
                "{url:?}: {err:?}"
            );
        }
        assert!(launcher.opened().is_empty());
    }

    #[test]
    fn a_launcher_failure_is_propagated_for_urls_too() {
        let launcher =
            FakeUrlLauncher::failing(|| PlatformError::unsupported("xdg-open is not installed"));

        let err = open_url_with("https://example.com", &launcher).expect_err("no launcher");

        assert!(matches!(&err, PlatformError::Unsupported { .. }), "{err:?}");
        assert!(err.user_facing().contains("xdg-open"));
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
