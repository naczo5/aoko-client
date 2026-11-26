//! `xdg-open`, the desktop's own launcher.
//!
//! Spawning the freedesktop launcher rather than guessing at `nautilus`/`dolphin`/…
//! means the user's configured file manager opens, including under KDE, GNOME and a
//! plain X session. When `xdg-open` is not installed there is no launcher, and that is
//! reported instead of tried-and-failed per desktop.
//!
//! The child is reaped on a short-lived thread: `xdg-open` exits as soon as it has
//! handed the path to the file manager, and a process that is spawned but never waited
//! for would stay a zombie for the life of the app.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::PlatformError;
use crate::open::{open_with, PathLauncher};

/// Program the freedesktop specification names for "open this".
pub const XDG_OPEN: &str = "xdg-open";

/// Opens `path` with `xdg-open`.
///
/// # Errors
/// See [`crate::open_path`]; additionally [`PlatformError::Unsupported`] when
/// `xdg-open` is not on `PATH`.
pub fn open_path(path: &Path) -> Result<(), PlatformError> {
    open_with(path, &XdgOpen)
}

struct XdgOpen;

impl PathLauncher for XdgOpen {
    fn launch(&self, path: &Path) -> Result<(), PlatformError> {
        // The child's output would otherwise land in the host's terminal, where a file
        // manager's warnings are noise nobody asked for.
        let child = Command::new(XDG_OPEN)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(classify_spawn_error)?;

        std::thread::spawn(move || {
            // Only here to reap the child; the exit status carries nothing the caller
            // can act on once the launcher has started.
            let mut child = child;
            let _ = child.wait();
        });

        Ok(())
    }
}

/// Whether a usable `xdg-open` exists, without running it.
///
/// The result is cached after the first look: `PATH` does not change under a running
/// app, and the capability report asks once at startup.
#[must_use]
pub fn opener_available() -> bool {
    static AVAILABLE: AtomicBool = AtomicBool::new(false);
    static PROBED: AtomicBool = AtomicBool::new(false);

    if PROBED.load(Ordering::Acquire) {
        return AVAILABLE.load(Ordering::Acquire);
    }

    let found = find_on_path(XDG_OPEN).is_some();
    AVAILABLE.store(found, Ordering::Release);
    PROBED.store(true, Ordering::Release);
    found
}

/// `Some` when `program` is an executable file in one of the `PATH` entries.
///
/// Injectable so the lookup can be tested against a scratch directory instead of
/// whatever this machine happens to have installed.
fn find_on_path_in(program: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path = path?;
    std::env::split_paths(path)
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable(candidate))
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    find_on_path_in(program, std::env::var_os("PATH").as_deref())
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// A spawn failure of the launcher itself.
///
/// `NotFound` from `Command::spawn` means the *program* is missing, which is a
/// capability gap, not a broken path: the path was already checked.
fn classify_spawn_error(err: io::Error) -> PlatformError {
    match err.kind() {
        io::ErrorKind::NotFound => PlatformError::unsupported(format!(
            "{XDG_OPEN} is not installed, so this desktop has no launcher to reveal a folder; \
             install xdg-utils"
        )),
        io::ErrorKind::PermissionDenied => {
            PlatformError::permission_denied(format!("{XDG_OPEN} could not be executed: {err}"))
        }
        _ => PlatformError::os("spawn xdg-open", err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn scratch_with_executable(name: &str, mode: u32) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("a scratch directory");
        let program = directory.path().join(name);
        std::fs::write(&program, "#!/bin/sh\n").expect("write the program");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(mode))
            .expect("set the mode");
        directory
    }

    #[test]
    fn an_executable_on_the_path_is_found() {
        let directory = scratch_with_executable(XDG_OPEN, 0o755);
        let path = std::env::join_paths([directory.path()]).expect("a search path");
        assert_eq!(
            find_on_path_in(XDG_OPEN, Some(&path)),
            Some(directory.path().join(XDG_OPEN))
        );
    }

    #[test]
    fn a_file_without_the_execute_bit_is_not_a_launcher() {
        let directory = scratch_with_executable(XDG_OPEN, 0o644);
        let path = std::env::join_paths([directory.path()]).expect("a search path");
        assert_eq!(find_on_path_in(XDG_OPEN, Some(&path)), None);
    }

    #[test]
    fn an_empty_search_path_finds_nothing() {
        assert_eq!(find_on_path_in(XDG_OPEN, None), None);
    }

    #[test]
    fn a_missing_program_is_unsupported_and_actionable() {
        let err = classify_spawn_error(io::Error::from(io::ErrorKind::NotFound));
        assert!(matches!(err, PlatformError::Unsupported { .. }), "{err:?}");
        assert!(
            err.user_facing().contains("xdg-utils"),
            "{err:?} must name the remedy"
        );
    }

    #[test]
    fn an_unexecutable_launcher_is_permission_denied() {
        let err = classify_spawn_error(io::Error::from(io::ErrorKind::PermissionDenied));
        assert!(
            matches!(err, PlatformError::PermissionDenied { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn any_other_spawn_failure_is_an_os_error() {
        let err = classify_spawn_error(io::Error::from(io::ErrorKind::OutOfMemory));
        assert!(matches!(err, PlatformError::Os { .. }), "{err:?}");
    }

    #[test]
    fn the_live_probe_answers_without_running_anything() {
        // Whatever this box has installed, the answer is a bool and no launcher runs.
        let available = opener_available();
        assert_eq!(available, find_on_path(XDG_OPEN).is_some());
        // The cached second read must agree with the first.
        assert_eq!(opener_available(), available);
    }
}
