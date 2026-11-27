//! Process inspection through `/proc`.
//!
//! `/proc/<pid>/cmdline` is NUL-separated argv, `/proc/<pid>/comm` the (15-byte
//! truncated) executable name and `/proc/<pid>/exe` a symlink to the image. Using
//! `cmdline` instead of the Windows PEB read is the whole port: the version
//! detection that consumes the command line is shared with the Windows path, so
//! a launcher is recognised identically on both.

use std::path::{Path, PathBuf};

use crate::error::PlatformError;
use crate::process::{
    describe_target, is_java_process_name, sort_targets, DiscoveryTarget, InstanceCandidate,
    ProcessInfo, ProcessInspector,
};
use crate::window::{WindowInfo, WindowInspector};

use super::window::LinuxWindowInspector;

/// Cap on how much of a `cmdline` is read, matching the Windows 32 KiB guard.
pub const MAX_CMDLINE_BYTES: u64 = 32 * 1024;

pub struct LinuxProcessInspector;

impl LinuxProcessInspector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxProcessInspector {
    fn default() -> Self {
        Self::new()
    }
}

/// Turns raw `cmdline` bytes into the single-line form the version parser
/// expects: arguments joined by one space, the trailing NUL dropped.
///
/// Invalid UTF-8 is replaced rather than rejected; a launcher argument containing
/// a stray byte must not hide a perfectly good `--version 1.8.9`.
#[must_use]
pub fn parse_proc_cmdline(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    for argument in raw.split(|byte| *byte == 0) {
        if argument.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&String::from_utf8_lossy(argument));
    }
    out
}

/// `/proc/<pid>/cmdline`, or `NotFound` for a pid that vanished or is a kernel
/// thread.
pub fn read_cmdline(pid: u32) -> Result<String, PlatformError> {
    let path = proc_path(pid, "cmdline");
    let bytes = read_bounded(&path, MAX_CMDLINE_BYTES).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            PlatformError::not_found(format!("pid {pid} has no /proc entry"))
        }
        std::io::ErrorKind::PermissionDenied => {
            PlatformError::permission_denied(format!("/proc/{pid}/cmdline is not readable"))
        }
        _ => PlatformError::os("read /proc/<pid>/cmdline", err.to_string()),
    })?;

    let parsed = parse_proc_cmdline(&bytes);
    if parsed.is_empty() {
        return Err(PlatformError::not_found(format!(
            "pid {pid} has an empty cmdline (kernel thread or zombie)"
        )));
    }
    Ok(parsed)
}

/// `/proc/<pid>/comm`, trimmed. Kernel threads keep their name between brackets.
#[must_use]
pub fn read_comm(pid: u32) -> Option<String> {
    let bytes = read_bounded(&proc_path(pid, "comm"), 64).ok()?;
    let name = String::from_utf8_lossy(&bytes).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// `/proc/<pid>/exe`, which is a symlink and therefore only readable by a process
/// that may inspect the target; `None` when denied.
#[must_use]
pub fn read_exe(pid: u32) -> Option<String> {
    let link = std::fs::read_link(proc_path(pid, "exe")).ok()?;
    Some(link.to_string_lossy().into_owned())
}

/// Best-effort basename of the executable, used to label windows and to widen the
/// JVM test to Wine-hosted launchers.
#[must_use]
pub fn process_name(pid: u32) -> Option<String> {
    if let Some(comm) = read_comm(pid) {
        return Some(comm);
    }
    let exe = read_exe(pid)?;
    Path::new(&exe)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Every numeric `/proc` entry, i.e. every live process visible to us.
#[must_use]
pub fn list_pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
        })
        .collect();
    pids.sort_unstable();
    pids
}

impl ProcessInspector for LinuxProcessInspector {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError> {
        let pids = list_pids();
        if pids.is_empty() && !Path::new("/proc").is_dir() {
            return Err(PlatformError::unsupported(
                "/proc is not mounted; process inspection needs procfs",
            ));
        }

        let mut processes = Vec::with_capacity(pids.len());
        for pid in pids {
            // A process that exits between readdir and the reads simply vanishes
            // from the list, which is what the managed host saw too.
            let Some(name) = process_name(pid) else {
                continue;
            };
            let command_line = read_cmdline(pid).ok();
            processes.push(ProcessInfo {
                pid,
                name,
                command_line,
            });
        }
        Ok(processes)
    }

    fn command_line(&self, pid: u32) -> Result<String, PlatformError> {
        read_cmdline(pid)
    }

    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError> {
        Ok(self
            .discover_targets()?
            .iter()
            .map(DiscoveryTarget::to_candidate)
            .collect())
    }

    fn discover_targets(&self) -> Result<Vec<DiscoveryTarget>, PlatformError> {
        let self_pid = std::process::id();
        let windows_by_pid = windows_by_pid();

        let mut targets = Vec::new();
        for pid in list_pids() {
            if pid == self_pid {
                continue;
            }
            // One `comm` read covers the common case; only a JVM whose `comm` is
            // truncated or renamed needs the `exe` symlink follow-up.
            let Some(name) = process_name(pid) else {
                continue;
            };
            if !is_java_process_name(&name) {
                continue;
            }

            let window: Option<&WindowInfo> = windows_by_pid
                .iter()
                .find(|(owner, _)| *owner == pid)
                .map(|(_, info)| info);
            let command_line = read_cmdline(pid).ok();
            let executable = read_exe(pid);

            targets.push(describe_target(
                pid,
                window.map(|info| info.handle),
                window.map_or("", |info| info.title.as_str()),
                &name,
                command_line.as_deref(),
                executable.as_deref(),
            ));
        }

        sort_targets(&mut targets);
        Ok(targets)
    }
}

/// One window per pid, preferring a title that names the game, like
/// `InjectionTargetDiscovery.SelectBestWindow`.
fn windows_by_pid() -> Vec<(u32, WindowInfo)> {
    let inspector = LinuxWindowInspector::new();
    let Ok(windows) = inspector.enumerate() else {
        return Vec::new();
    };

    let mut grouped: Vec<(u32, Vec<WindowInfo>)> = Vec::new();
    for info in windows {
        let Some(pid) = info.pid else {
            continue;
        };
        match grouped.iter_mut().find(|(owner, _)| *owner == pid) {
            Some((_, group)) => group.push(info),
            None => grouped.push((pid, vec![info])),
        }
    }

    grouped
        .into_iter()
        .filter_map(|(pid, group)| {
            crate::process::select_best_window(&group).map(|best| (pid, best.clone()))
        })
        .collect()
}

fn proc_path(pid: u32, leaf: &str) -> PathBuf {
    let mut path = PathBuf::with_capacity(24);
    path.push("/proc");
    path.push(pid.to_string());
    path.push(leaf);
    path
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.by_ref().take(limit).read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cmdline_joins_nul_separated_arguments_with_spaces() {
        let raw = b"java\0-cp\0/home/me/.minecraft/versions/1.8.9/1.8.9.jar\0net.minecraft.client.main.Main\0";
        assert_eq!(
            parse_proc_cmdline(raw),
            "java -cp /home/me/.minecraft/versions/1.8.9/1.8.9.jar net.minecraft.client.main.Main"
        );
    }

    #[test]
    fn parse_cmdline_drops_the_trailing_terminator_and_empty_arguments() {
        assert_eq!(parse_proc_cmdline(b"java\0\0\0"), "java");
        assert_eq!(parse_proc_cmdline(b""), "");
        assert_eq!(parse_proc_cmdline(b"\0\0"), "");
    }

    #[test]
    fn parse_cmdline_keeps_spaces_inside_a_single_argument() {
        let raw = b"javaw\0-Duser.name=Some Player\0-Dversion=1.8.9\0";
        assert_eq!(
            parse_proc_cmdline(raw),
            "javaw -Duser.name=Some Player -Dversion=1.8.9"
        );
    }

    #[test]
    fn parse_cmdline_is_lossy_for_invalid_utf8() {
        let raw = [b'j', b'a', 0xff, 0xfe, 0, b'x'];
        let parsed = parse_proc_cmdline(&raw);
        assert!(parsed.starts_with("ja"));
        assert!(parsed.ends_with(" x"));
    }

    #[test]
    fn parse_cmdline_feeds_the_shared_version_parser() {
        let raw = b"javaw\x00--version\x0026.2\x00com.moonsworth.lunar.genesis.Genesis\x00";
        let parsed = parse_proc_cmdline(raw);
        assert_eq!(crate::version::try_parse_version(&parsed), Some("26.2"));
    }

    #[test]
    fn parse_cmdline_keeps_a_prism_jar_path_intact() {
        let raw = b"java\0-cp\0/home/me/.local/share/PrismLauncher/libraries/com/mojang/minecraft/26.2/minecraft-26.2-client.jar\0org.prismlauncher.EntryPoint\0";
        let parsed = parse_proc_cmdline(raw);
        assert_eq!(crate::version::try_parse_version(&parsed), Some("26.2"));
    }

    #[test]
    fn an_exe_basename_of_java_is_recognised_through_the_name_helper() {
        // `process_name` prefers `comm` and falls back to the `exe` symlink, so a
        // Wine-hosted `java.exe` is recognised the same way a native `java` is.
        let name = process_name(std::process::id()).expect("own process name");
        assert!(!name.is_empty());
        assert!(!name.contains('/'));
    }

    #[test]
    fn our_own_cmdline_is_readable() {
        let pid = std::process::id();
        let command_line = read_cmdline(pid).expect("own cmdline");
        assert!(!command_line.is_empty());
        assert!(!command_line.contains('\0'));
    }

    #[test]
    fn command_line_for_a_missing_pid_is_not_found() {
        // Pids are bounded by the kernel's pid_max; this one is above any real pid.
        let err = read_cmdline(u32::MAX - 1).expect_err("no such pid");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }

    #[test]
    fn list_processes_contains_the_current_process() {
        let processes = LinuxProcessInspector::new()
            .list_processes()
            .expect("procfs");
        assert!(processes.iter().any(|info| info.pid == std::process::id()));
        assert!(processes.iter().all(|info| !info.name.is_empty()));
    }

    #[test]
    fn the_inspector_does_not_offer_its_own_process_as_a_target() {
        let targets = LinuxProcessInspector::new()
            .discover_targets()
            .expect("procfs");
        assert!(targets
            .iter()
            .all(|target| target.pid != std::process::id()));
    }
}
