//! Resolving the native bridge library that the injector loads.
//!
//! Two halves of the managed host meet here. `GameStateClient.ResolveBridgeDllName`
//! picks `bridge_261.dll` for `26.*` and `1.21*` and `bridge.dll` for everything
//! else, and the build shipped both beside the executable (`Aoko.csproj:30` and
//! `:35`), which is where `DllImport("bridge.dll")` resolved them from and where
//! this crate looks.
//!
//! [`bridge_library_path`] is a pure join, so the name mapping is testable with
//! no game, no DLL and no filesystem. [`resolve_bridge_library`] is the one that
//! touches the disk, and it fails with the path it tried: a missing library used
//! to mean no injection at all while the UI looked connected, which is the same
//! class of failure as a state that never leaves `Connecting`.
//! [`inject_bridge`] is the pair the shell needs - resolve, then attach - so the
//! check cannot be skipped on the way to [`crate::NativeBridgeInjector::inject`].

use std::path::{Path, PathBuf};

use crate::error::PlatformError;
use crate::version::resolve_bridge_dll_name;
use crate::{InjectionReport, NativeBridgeInjector};

/// The bridge library that matches `version`, as `exe_dir / name`.
///
/// `GameStateClient.ResolveBridgeDllName` joined onto the directory the host's
/// own executable lives in, which is where the managed build shipped the two
/// DLLs. No filesystem access: the caller supplies `exe_dir`, so this answers
/// for a launcher on another drive, a directory that does not exist yet, or a
/// test, with the same code.
///
/// An absent, empty, whitespace-only or unrecognised version falls back to
/// `bridge.dll`. That is the managed fallback twice over: `ResolveBridgeDllName`
/// returns `bridge.dll` for null or whitespace, and its caller's own fallback
/// version (`1.8.9`) maps to the same name, so an undetected version reaches the
/// legacy bridge either way.
#[must_use]
pub fn bridge_library_path(version: Option<&str>, exe_dir: &Path) -> PathBuf {
    exe_dir.join(bridge_library_name(version))
}

/// [`bridge_library_path`], checked against the filesystem.
///
/// Exists because the interesting failure is the file not being there, and the
/// error has to say which path was tried: "no game state" is a symptom a user
/// cannot act on, while `C:\Program Files\Aoko\bridge_261.dll` is.
///
/// # Errors
///
/// [`PlatformError::NotFound`] naming the path and the version that selected it
/// when the file is absent or is not a file.
pub fn resolve_bridge_library(
    version: Option<&str>,
    exe_dir: &Path,
) -> Result<PathBuf, PlatformError> {
    let name = bridge_library_name(version);
    let path = exe_dir.join(name);
    if path.is_file() {
        return Ok(path);
    }
    Err(missing_library(&path, name, version))
}

/// The directory this host's own executable lives in.
///
/// The `exe_dir` argument of the two functions above, resolved once by the
/// shell. It is the host's directory, not the game's: the bridge DLL ships
/// beside the host, and the managed injector relied on that same layout.
///
/// # Errors
///
/// [`PlatformError::Os`] when the OS will not say where the executable is, and
/// [`PlatformError::NotFound`] when the answer has no parent directory.
pub fn host_executable_dir() -> Result<PathBuf, PlatformError> {
    let executable =
        std::env::current_exe().map_err(|err| PlatformError::os("current_exe", err.to_string()))?;
    executable.parent().map(Path::to_path_buf).ok_or_else(|| {
        PlatformError::not_found(format!(
            "{} has no parent directory to look for a bridge library in",
            executable.display()
        ))
    })
}

/// Resolves the library for `version` and loads it into `pid`.
///
/// The whole attach, in one call: a shell that uses this cannot inject a name
/// that is not on disk, and the error it gets back names the path. On Linux the
/// resolution still succeeds when the file exists, and the injector then refuses
/// with [`PlatformError::Unsupported`] - a typed answer, not a panic and not a
/// silent success.
///
/// # Errors
///
/// Whatever [`resolve_bridge_library`] or
/// [`NativeBridgeInjector::inject`] returns.
pub fn inject_bridge(
    injector: &dyn NativeBridgeInjector,
    pid: u32,
    version: Option<&str>,
    exe_dir: &Path,
) -> Result<InjectionReport, PlatformError> {
    let library = resolve_bridge_library(version, exe_dir)?;
    injector.inject(pid, &library)
}

/// `GameStateClient.ResolveBridgeDllName`, with the absent version handled here.
fn bridge_library_name(version: Option<&str>) -> &'static str {
    version.map_or("bridge.dll", resolve_bridge_dll_name)
}

/// The error for a library that is not on disk, naming the path and the choice.
fn missing_library(path: &Path, name: &str, version: Option<&str>) -> PlatformError {
    let selection = match version.map(str::trim) {
        Some(version) if !version.is_empty() => format!("version {version} selects {name}"),
        _ => format!("no detected version falls back to {name}"),
    };
    PlatformError::not_found(format!(
        "bridge library not found at {} ({selection}); the host must ship it beside its own \
         executable, which is what the managed build did",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BridgeSupport;
    use std::sync::Mutex;

    /// A stand-in injector that records what it was asked to load.
    #[derive(Default)]
    struct RecordingInjector {
        seen: Mutex<Vec<PathBuf>>,
    }

    impl RecordingInjector {
        fn calls(&self) -> usize {
            self.seen.lock().expect("lock").len()
        }

        fn last(&self) -> Option<PathBuf> {
            self.seen.lock().expect("lock").last().cloned()
        }
    }

    impl NativeBridgeInjector for RecordingInjector {
        fn inject(&self, pid: u32, library: &Path) -> Result<InjectionReport, PlatformError> {
            self.seen.lock().expect("lock").push(library.to_path_buf());
            Ok(InjectionReport {
                pid,
                library: library.display().to_string(),
                remote_base: None,
            })
        }

        fn support(&self) -> BridgeSupport {
            BridgeSupport::Supported
        }
    }

    // The mapping itself is `resolve_bridge_dll_name`'s and is tested there; this
    // asserts it arrives through the resolver as a joined path.
    #[test]
    fn bridge_library_path_maps_the_version_to_the_managed_name() {
        let exe_dir = Path::new("/opt/aoko");
        let cases: [(Option<&str>, &str); 12] = [
            (Some("26.3"), "bridge_261.dll"),
            (Some("26.2"), "bridge_261.dll"),
            (Some("26.1"), "bridge_261.dll"),
            (Some("1.21"), "bridge_261.dll"),
            (Some("1.21.4"), "bridge_261.dll"),
            // Padded values still select the modern bridge: the trim in
            // `resolve_bridge_dll_name` is this port's deliberate deviation from
            // managed, which matched the untrimmed string and answered
            // `bridge.dll` for `" 26.3 "`.
            (Some(" 26.3 "), "bridge_261.dll"),
            (Some(" 26.3x "), "bridge_261.dll"),
            (Some(" 1.21 "), "bridge_261.dll"),
            (Some("1.8.9"), "bridge.dll"),
            (None, "bridge.dll"),
            (Some(""), "bridge.dll"),
            (Some("   "), "bridge.dll"),
        ];

        for (version, name) in cases {
            assert_eq!(
                bridge_library_path(version, exe_dir),
                exe_dir.join(name),
                "bridge_library_path({version:?})"
            );
        }
    }

    #[test]
    fn an_unrecognised_version_takes_the_legacy_bridge() {
        // None of these is a version. `26` has no dot, so managed's own
        // `StartsWith("26.")` was false for it too, and the port agrees with
        // managed on all three. Managed's caller-level fallback - "1.8.9", used
        // when parsing is inconclusive - lands on the same name, so both routes
        // reach the legacy bridge.
        //
        // A padded `26.*` is deliberately *not* here: the trim makes it modern,
        // which is the recorded deviation on `resolve_bridge_dll_name`.
        for version in [Some("auto"), Some("26"), Some("latest")] {
            assert_eq!(
                bridge_library_path(version, Path::new("/opt/aoko")),
                Path::new("/opt/aoko").join("bridge.dll"),
                "bridge_library_path({version:?})"
            );
        }
    }

    #[test]
    fn bridge_library_path_joins_onto_the_executable_directory() {
        let exe_dir = Path::new("/opt/aoko");
        let path = bridge_library_path(Some("26.3"), exe_dir);
        assert_eq!(path.parent(), Some(exe_dir));
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("bridge_261.dll")
        );

        // A relative directory is joined, never resolved against the working
        // directory: this function does no I/O.
        assert_eq!(
            bridge_library_path(None, Path::new("relative-bin")),
            Path::new("relative-bin").join("bridge.dll")
        );
    }

    #[test]
    fn a_missing_library_names_the_path_it_tried() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = resolve_bridge_library(Some("26.3"), dir.path()).expect_err("nothing shipped");

        assert!(matches!(&err, PlatformError::NotFound { .. }));
        let expected = dir.path().join("bridge_261.dll");
        let message = err.user_facing();
        assert!(
            message.contains(&expected.display().to_string()),
            "the message must name the path it tried: {message}"
        );
        assert!(
            message.contains("26.3"),
            "and the version that selected it: {message}"
        );
    }

    #[test]
    fn an_undetected_version_says_it_fell_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = resolve_bridge_library(None, dir.path()).expect_err("nothing shipped");
        assert!(err.user_facing().contains("falls back to bridge.dll"));
        assert!(err.user_facing().contains("bridge.dll"));
    }

    #[test]
    fn a_shipped_library_resolves_and_the_other_name_still_does_not() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("bridge.dll"), b"stub").expect("write legacy bridge");

        assert_eq!(
            resolve_bridge_library(Some("1.8.9"), dir.path()).expect("legacy bridge"),
            dir.path().join("bridge.dll")
        );
        // The modern name is absent, so it is still an error rather than a guess.
        assert!(resolve_bridge_library(Some("26.3"), dir.path()).is_err());
    }

    #[test]
    fn a_directory_named_like_the_library_is_not_a_library() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("bridge.dll")).expect("mkdir");

        let err = resolve_bridge_library(None, dir.path()).expect_err("a directory is not a file");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }

    #[test]
    fn inject_bridge_refuses_before_opening_a_process_when_the_library_is_absent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let injector = RecordingInjector::default();

        let err = inject_bridge(&injector, 4242, Some("1.8.9"), dir.path())
            .expect_err("no library to load");

        assert!(matches!(err, PlatformError::NotFound { .. }));
        assert_eq!(injector.calls(), 0, "the injector must not be reached");
    }

    #[test]
    fn inject_bridge_hands_the_resolved_path_to_the_injector() {
        let dir = tempfile::tempdir().expect("temp dir");
        let library = dir.path().join("bridge_261.dll");
        std::fs::write(&library, b"stub").expect("write bridge");
        let injector = RecordingInjector::default();

        let report = inject_bridge(&injector, 77, Some("26.3"), dir.path()).expect("inject");

        assert_eq!(report.pid, 77);
        assert_eq!(injector.calls(), 1);
        assert_eq!(injector.last(), Some(library));
        assert_eq!(
            report.library,
            dir.path().join("bridge_261.dll").display().to_string()
        );
    }

    #[test]
    fn host_executable_dir_answers_a_directory() {
        let dir = host_executable_dir().expect("the test binary has a path");
        assert!(dir.is_dir(), "{} must be a directory", dir.display());
    }

    // The composition stays honest on the platform without a bridge: resolution
    // succeeds, the platform refuses with a typed error, nothing panics.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_injector_refuses_a_resolved_library_with_a_typed_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("bridge.dll"), b"stub").expect("write bridge");
        let injector = crate::LinuxBridgeInjector::new();

        let err = inject_bridge(&injector, 1, None, dir.path()).expect_err("no linux bridge");

        assert!(matches!(&err, PlatformError::Unsupported { .. }));
        assert!(err.user_facing().contains("bridge.dll"));
        assert_eq!(injector.support(), BridgeSupport::UnsupportedOnLinuxYet);
    }
}
