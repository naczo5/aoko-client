//! `ShellExecuteW` with the `open` verb, the shell's own launcher.
//!
//! Going through the shell rather than spawning `explorer.exe` means a session with no
//! usable shell (a stripped container, a service account) comes back as a documented
//! `ShellExecuteW` code instead of a stray child process nobody reaps. The return value
//! is the shell's error code, not a `BOOL`, so the classification below is the shell's
//! documented ladder rather than a guess.

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::error::PlatformError;
use crate::open::{open_with, PathLauncher};

/// `ShellExecuteW` reports success as any value greater than 32.
const SHELL_SUCCESS_FLOOR: isize = 32;

/// `SE_ERR_FNF`: the file was not found.
const SE_ERR_FNF: isize = 2;
/// `SE_ERR_PNF`: the path was not found.
const SE_ERR_PNF: isize = 3;
/// `SE_ERR_ACCESSDENIED`: the shell was denied access.
const SE_ERR_ACCESSDENIED: isize = 5;
/// `SE_ERR_NOASSOC`: nothing is registered to open this type.
const SE_ERR_NOASSOC: isize = 31;

/// Opens `path` with the shell.
///
/// # Errors
/// See [`crate::open_path`]; a failing `ShellExecuteW` code maps to a classified error
/// naming what the shell refused.
pub fn open_path(path: &Path) -> Result<(), PlatformError> {
    open_with(path, &ShellOpen)
}

/// Whether the shell can be asked at all.
///
/// `shell32` is loaded in every interactive Windows session, and [open_path] still
/// reports a refusal per call, so this is `true` rather than a probe that could only
/// lie in one direction.
#[must_use]
pub fn opener_available() -> bool {
    true
}

struct ShellOpen;

impl PathLauncher for ShellOpen {
    fn launch(&self, path: &Path) -> Result<(), PlatformError> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: `wide` is a live, NUL-terminated UTF-16 buffer for the duration of
        // the call, the verb is a static NUL-terminated literal, and the window,
        // parameters and directory are the documented "none" values. `ShellExecuteW`
        // returns an error code rather than borrowing anything.
        let result = unsafe {
            ShellExecuteW(
                None,
                windows::core::w!("open"),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        classify_shell_result(result.0 as isize, path)
    }
}

/// Turns a `ShellExecuteW` return value into the caller's verdict.
fn classify_shell_result(code: isize, path: &Path) -> Result<(), PlatformError> {
    if code > SHELL_SUCCESS_FLOOR {
        return Ok(());
    }

    match code {
        // The path was already checked, so the shell losing it means it vanished
        // between the check and the call: still a not-found for the caller.
        SE_ERR_FNF | SE_ERR_PNF => Err(PlatformError::not_found(format!(
            "{} could not be opened: the shell reported it missing",
            path.display()
        ))),
        SE_ERR_ACCESSDENIED => Err(PlatformError::permission_denied(format!(
            "the shell is not allowed to open {}",
            path.display()
        ))),
        SE_ERR_NOASSOC => Err(PlatformError::unsupported(format!(
            "nothing is registered to open {}",
            path.display()
        ))),
        other => Err(PlatformError::os(
            "ShellExecuteW",
            format!("opening {} returned {other}", path.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> &'static Path {
        Path::new(r"C:\Users\a\AppData\Roaming\Aoko\profiles")
    }

    #[test]
    fn the_shell_always_claims_to_be_available() {
        assert!(opener_available());
    }

    #[test]
    fn any_code_above_32_is_success() {
        assert!(classify_shell_result(33, path()).is_ok());
        assert!(classify_shell_result(42, path()).is_ok());
    }

    #[test]
    fn a_vanished_path_stays_not_found() {
        for code in [SE_ERR_FNF, SE_ERR_PNF] {
            let err = classify_shell_result(code, path()).expect_err("missing");
            assert!(matches!(err, PlatformError::NotFound { .. }), "{err:?}");
        }
    }

    #[test]
    fn a_denied_open_is_a_permission_problem() {
        let err = classify_shell_result(SE_ERR_ACCESSDENIED, path()).expect_err("denied");
        assert!(
            matches!(err, PlatformError::PermissionDenied { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn no_registered_handler_is_unsupported() {
        let err = classify_shell_result(SE_ERR_NOASSOC, path()).expect_err("no handler");
        assert!(matches!(err, PlatformError::Unsupported { .. }), "{err:?}");
    }

    #[test]
    fn zero_and_other_codes_are_os_errors_naming_the_path() {
        for code in [0, 8, 26, 27, 28, 29, 30] {
            let err = classify_shell_result(code, path()).expect_err("failed");
            assert!(matches!(err, PlatformError::Os { .. }), "{code}: {err:?}");
            assert!(err.user_facing().contains("profiles"), "{err:?}");
        }
    }
}
