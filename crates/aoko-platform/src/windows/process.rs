//! Process enumeration, PEB command-line reads and instance discovery.
//!
//! `ProcessCommandLine.TryGet` reads the target's PEB on x64: `PEB+0x20` is
//! `RTL_USER_PROCESS_PARAMETERS`, and `CommandLine` is the `UNICODE_STRING` at
//! `+0x70` (length) / `+0x78` (buffer). The offsets, the 32 KiB cap and the two
//! width guards are the managed values: a 32-bit host cannot use them at all, and
//! neither can a WOW64 target, whose PEB is the 32-bit one. Both cases report the
//! command line as unknown rather than reading it with the wrong layout.

use core::ffi::c_void;

use windows::Win32::Foundation::{BOOL, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    IsWow64Process, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ,
};

use crate::error::PlatformError;
use crate::process::{
    describe_target, is_java_process_name, select_best_window, sort_targets, DiscoveryTarget,
    InstanceCandidate, ProcessInfo, ProcessInspector,
};
use crate::window::{WindowInfo, WindowInspector};

use super::handles::OwnedHandle;
use super::raw::{NtQueryInformationProcess, ProcessBasicInformation, ReadProcessMemory};
use super::window::WindowsWindowInspector;

const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;
/// `PEB.ProcessParameters`, x64.
const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
/// `RTL_USER_PROCESS_PARAMETERS.CommandLine.Length`, x64.
const PROCESS_PARAMETERS_COMMAND_LINE_LENGTH_OFFSET: usize = 0x70;
/// `RTL_USER_PROCESS_PARAMETERS.CommandLine.Buffer`, x64.
const PROCESS_PARAMETERS_COMMAND_LINE_BUFFER_OFFSET: usize = 0x78;
/// `ProcessCommandLine` refuses anything longer than this many UTF-16 units.
const MAX_COMMAND_LINE_CHARS: usize = 32_768;

pub struct WindowsProcessInspector;

impl WindowsProcessInspector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsProcessInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessInspector for WindowsProcessInspector {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError> {
        // SAFETY: a process snapshot takes a flags value and a pid, needs no
        // buffer, and returns an owned handle or an error.
        let snapshot =
            OwnedHandle::from_result(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
                .map_err(|err| PlatformError::os("CreateToolhelp32Snapshot", err.to_string()))?;

        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(core::mem::size_of::<PROCESSENTRY32W>())
                .map_err(|_| PlatformError::os("PROCESSENTRY32W", "size does not fit a u32"))?,
            ..Default::default()
        };

        // SAFETY: `entry` is a live, correctly sized `PROCESSENTRY32W` with `dwSize`
        // set, and `snapshot` is an owned toolhelp handle.
        if unsafe { Process32FirstW(snapshot.get(), &mut entry) }.is_err() {
            return Err(PlatformError::os(
                "Process32FirstW",
                windows::core::Error::from_win32().to_string(),
            ));
        }

        let mut processes = Vec::new();
        loop {
            let name = wide_array_to_string(&entry.szExeFile);
            let pid = entry.th32ProcessID;
            if pid != 0 {
                // The command line is read only for a JVM. `InjectionTargetDiscovery.ListTargets`
                // filtered by `JavaProcessNames` before it touched the PEB
                // (`InjectionTargetDiscovery.cs:66-84`), and the PEB read is the whole cost of this
                // function: `OpenProcess` plus `NtQueryInformationProcess` plus three
                // `ReadProcessMemory` calls per PID. Doing that for every process on the machine is
                // several hundred syscalls per enumeration, and it fails outright for every
                // protected process.
                //
                // A non-JVM entry therefore reports `command_line: None`, the same "not readable"
                // answer a denied PEB read gives; see [`ProcessInfo::command_line`].
                let command_line = if is_java_process_name(&name) {
                    self.command_line(pid).ok()
                } else {
                    None
                };
                processes.push(ProcessInfo {
                    pid,
                    name,
                    command_line,
                });
            }

            // SAFETY: same invariants as the first call; `entry` is reused.
            if unsafe { Process32NextW(snapshot.get(), &mut entry) }.is_err() {
                break;
            }
        }

        Ok(processes)
    }

    fn command_line(&self, pid: u32) -> Result<String, PlatformError> {
        read_command_line(pid)
    }

    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError> {
        Ok(self
            .discover_targets()?
            .iter()
            .map(DiscoveryTarget::to_candidate)
            .collect())
    }

    fn discover_targets(&self) -> Result<Vec<DiscoveryTarget>, PlatformError> {
        let windows = WindowsWindowInspector::new()
            .enumerate()
            .unwrap_or_default();
        let self_pid = std::process::id();

        let mut targets = Vec::new();
        for process in self.list_processes()? {
            if process.pid == self_pid || !is_java_process_name(&process.name) {
                continue;
            }

            let own_windows: Vec<WindowInfo> = windows
                .iter()
                .filter(|window| window.pid == Some(process.pid))
                .cloned()
                .collect();
            let window = select_best_window(&own_windows);

            targets.push(describe_target(
                process.pid,
                window.map(|info| info.handle),
                window.map_or("", |info| info.title.as_str()),
                &process.name,
                process.command_line.as_deref(),
                super::window::process_image_name(process.pid).as_deref(),
            ));
        }

        sort_targets(&mut targets);
        Ok(targets)
    }
}

/// `ProcessCommandLine.TryGet`: the full command line from the target's PEB.
pub fn read_command_line(pid: u32) -> Result<String, PlatformError> {
    if pid == 0 {
        return Err(PlatformError::not_found("pid 0 is the idle process"));
    }
    if !cfg!(target_pointer_width = "64") {
        return Err(PlatformError::unsupported(
            "the PEB command-line layout is the x64 one; build the host as x64",
        ));
    }

    let process = open_for_inspection(pid)?;

    if is_wow64(&process) {
        return Err(PlatformError::unsupported(format!(
            "pid {pid} is a 32-bit process running under WOW64, so its PEB uses the 32-bit \
             layout these x64 offsets do not describe; the command line is reported as \
             unknown rather than read from the wrong offsets"
        )));
    }

    let mut info = ProcessBasicInformation {
        reserved1: 0,
        peb_base_address: 0,
        reserved2_0: 0,
        reserved2_1: 0,
        unique_process_id: 0,
        reserved3: 0,
    };
    let mut return_length = 0u32;
    // SAFETY: `info` is a live, correctly sized `PROCESS_BASIC_INFORMATION` and the
    // length passed matches it exactly, which is what the API requires.
    let status = unsafe {
        NtQueryInformationProcess(
            process.get(),
            PROCESS_BASIC_INFORMATION_CLASS,
            std::ptr::from_mut(&mut info).cast::<c_void>(),
            u32::try_from(core::mem::size_of::<ProcessBasicInformation>()).unwrap_or(0),
            &mut return_length,
        )
    };
    if status.0 != 0 || info.peb_base_address == 0 {
        return Err(PlatformError::os(
            "NtQueryInformationProcess",
            format!("status {:#x}", status.0),
        ));
    }

    let process_parameters = read_pointer(
        process.get(),
        info.peb_base_address + PEB_PROCESS_PARAMETERS_OFFSET,
    )?;

    let mut length_bytes = [0u8; 2];
    read_memory(
        process.get(),
        process_parameters + PROCESS_PARAMETERS_COMMAND_LINE_LENGTH_OFFSET,
        &mut length_bytes,
    )?;
    let length_bytes = usize::from(u16::from_le_bytes(length_bytes));
    if length_bytes == 0 {
        return Err(PlatformError::not_found(format!(
            "pid {pid} has an empty command line"
        )));
    }

    let char_count = length_bytes / 2;
    if char_count == 0 || char_count > MAX_COMMAND_LINE_CHARS {
        return Err(PlatformError::os(
            "ReadProcessMemory",
            format!("pid {pid} reports {char_count} command-line characters"),
        ));
    }

    let buffer = read_pointer(
        process.get(),
        process_parameters + PROCESS_PARAMETERS_COMMAND_LINE_BUFFER_OFFSET,
    )?;
    if buffer == 0 {
        return Err(PlatformError::not_found(format!(
            "pid {pid} has no command-line buffer"
        )));
    }

    let mut raw = vec![0u8; length_bytes];
    read_memory(process.get(), buffer, &mut raw)?;

    let (pairs, _remainder) = raw.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let command_line = String::from_utf16_lossy(&units);
    let trimmed = command_line.trim_end_matches('\0');
    if trimmed.is_empty() {
        return Err(PlatformError::not_found(format!(
            "pid {pid} has an empty command line"
        )));
    }
    Ok(trimmed.to_string())
}

/// Whether the target runs under WOW64, i.e. is a 32-bit process on a 64-bit
/// Windows. `ProcessCommandLine` returned `null` for these because the PEB offsets
/// it used are the x64 ones and do not describe a 32-bit PEB.
///
/// The managed reader wrote `IsWow64Process(handle, out isWow64) && isWow64`, so a
/// failed probe counted as "not WOW64" and the read continued; a failing probe here
/// means the handle lacks query rights, which the read below then reports on its own.
fn is_wow64(process: &OwnedHandle) -> bool {
    let mut result = BOOL(0);
    // SAFETY: `result` is a live, correctly sized `BOOL` and `process` is an owned
    // handle opened with query access, which is what `IsWow64Process` requires.
    let probed = unsafe { IsWow64Process(process.get(), &mut result) };
    probed.is_ok() && result.as_bool()
}

fn open_for_inspection(pid: u32) -> Result<OwnedHandle, PlatformError> {
    let desired = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;
    // SAFETY: `OpenProcess` takes a pid and access mask; it returns an owned handle
    // or fails, and `OwnedHandle` closes whatever comes back.
    if let Ok(handle) = OwnedHandle::from_result(unsafe { OpenProcess(desired, false, pid) }) {
        return Ok(handle);
    }

    // `PROCESS_QUERY_LIMITED_INFORMATION` works across integrity levels where the
    // full query right is denied, which is why the managed reader retried with it.
    // SAFETY: as above.
    let second = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
    };
    OwnedHandle::from_result(second).map_err(|err| {
        PlatformError::permission_denied(format!(
            "cannot open pid {pid} for inspection ({err}); the target may be protected or \
             run at a higher integrity level"
        ))
    })
}

fn read_pointer(process: HANDLE, address: usize) -> Result<usize, PlatformError> {
    let mut buffer = [0u8; core::mem::size_of::<usize>()];
    read_memory(process, address, &mut buffer)?;
    Ok(usize::from_le_bytes(buffer))
}

fn read_memory(process: HANDLE, address: usize, buffer: &mut [u8]) -> Result<(), PlatformError> {
    if buffer.is_empty() {
        return Ok(());
    }
    let mut read = 0usize;
    // SAFETY: `buffer` is live for `buffer.len()` bytes and the call writes at most
    // that many from `address` in the target; `read` reports how many arrived.
    let ok = unsafe {
        ReadProcessMemory(
            process,
            address as *const c_void,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len(),
            &mut read,
        )
    };
    if ok.as_bool() && read == buffer.len() {
        return Ok(());
    }
    Err(PlatformError::os(
        "ReadProcessMemory",
        format!(
            "{} of {} bytes at {address:#x}: {}",
            read,
            buffer.len(),
            windows::core::Error::from_win32()
        ),
    ))
}

fn wide_array_to_string(units: &[u16]) -> String {
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..length])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_command_line_is_readable() {
        let command_line = read_command_line(std::process::id()).expect("own command line");
        assert!(!command_line.is_empty());
    }

    #[test]
    fn the_wow64_guard_is_consulted_and_accepts_this_64_bit_process() {
        let process = open_for_inspection(std::process::id()).expect("own process handle");
        // This suite runs as a 64-bit binary, so it cannot itself be a WOW64 target.
        assert!(!is_wow64(&process), "a 64-bit test host is not under WOW64");
        // The guard must not reject a target it does not apply to: the very same
        // handle still feeds a successful PEB read.
        assert!(read_command_line(std::process::id()).is_ok());
    }

    #[test]
    fn pid_zero_is_rejected_without_touching_the_kernel() {
        let err = read_command_line(0).expect_err("pid 0");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }

    #[test]
    fn a_missing_pid_is_not_found_or_permission_denied() {
        // A pid this high cannot exist, so `OpenProcess` must fail.
        let err = read_command_line(0xFFFF_FFF0).expect_err("no such pid");
        assert!(matches!(
            err,
            PlatformError::PermissionDenied { .. } | PlatformError::NotFound { .. }
        ));
    }

    #[test]
    fn listing_processes_finds_this_process() {
        let processes = WindowsProcessInspector::new()
            .list_processes()
            .expect("toolhelp snapshot");
        assert!(processes.iter().any(|info| info.pid == std::process::id()));
        assert!(processes.iter().all(|info| !info.name.is_empty()));
    }

    #[test]
    fn the_inspector_does_not_offer_its_own_process_as_a_target() {
        let targets = WindowsProcessInspector::new()
            .discover_targets()
            .expect("toolhelp snapshot");
        assert!(targets
            .iter()
            .all(|target| target.pid != std::process::id()));
    }

    #[test]
    fn wide_array_conversion_stops_at_the_terminator() {
        let units = [b'j' as u16, b'a' as u16, 0, b'x' as u16];
        assert_eq!(wide_array_to_string(&units), "ja");
    }
}
