//! `LoadLibrary` injection, ported from `Aoko/Core/NativeInjector.cs`.
//!
//! The sequence is the managed one, with two additions it lacked: refuse a target
//! that already holds the module, then open the process with the three rights the
//! injector's own calls need, allocate `path + NUL` in the target, write it, resolve
//! `LoadLibraryW` from `kernel32` (`LoadLibraryA` only where W cannot be resolved),
//! create a remote thread on it, wait for the thread, read the module handle it
//! returned, free the buffer, close everything.
//!
//! The path is written as UTF-16, which is the encoding that can name a library the
//! host can open and the target cannot spell in ANSI: a profile directory with an
//! accent, a Cyrillic or a CJK user name is one `?` per character through
//! `LoadLibraryA`, and the game then reports a missing file that is on disk.
//!
//! Every handle and the remote allocation are RAII-owned, so an early return frees
//! them in the same order the C# `finally` block did. The one deliberate exception
//! is a remote thread that is still running: it owns the path it is about to read,
//! so that block stays allocated (see [`RemoteAllocation::forget`]).
//!
//! The wait is bounded ([`REMOTE_THREAD_WAIT_MS`]) where the managed injector
//! passed `INFINITE`, and its answer is checked: a zero exit code is the target's
//! `LoadLibrary` having failed, which the managed version reported as success. The
//! press that reaches this is a synchronous click handler, so the bound is also what
//! keeps a wedged game from taking the window with it.

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject,
    LPTHREAD_START_ROUTINE, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};

use crate::{BridgeSupport, InjectionReport, NativeBridgeInjector, PlatformError};

use super::handles::OwnedHandle;
use super::raw::WriteProcessMemory;

/// How long the remote thread is given to return from `LoadLibrary`.
///
/// The managed injector waited `INFINITE`, so a target that never ran the entry point
/// froze the press that started it for the rest of the session. Five seconds is longer
/// than a module load takes on a running game and short enough that a wedged target
/// costs a bounded wait and an error line instead of a dead window.
const REMOTE_THREAD_WAIT_MS: u32 = 5_000;

/// `ERROR_INVALID_PARAMETER`: `OpenProcess`'s answer for a pid that is not running.
const ERROR_INVALID_PARAMETER: u32 = 87;

/// `ERROR_ACCESS_DENIED`: the target is at a higher integrity level, or protected.
const ERROR_ACCESS_DENIED: u32 = 5;

pub struct WindowsBridgeInjector;

impl WindowsBridgeInjector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsBridgeInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeBridgeInjector for WindowsBridgeInjector {
    fn inject(&self, pid: u32, library: &Path) -> Result<InjectionReport, PlatformError> {
        if !library.is_file() {
            return Err(PlatformError::not_found(format!(
                "{} does not exist",
                library.display()
            )));
        }

        // The remote side receives the path in the encoding of the entry point that is
        // resolved below: UTF-16 for `LoadLibraryW`, the managed ANSI bytes for the
        // `LoadLibraryA` fallback.
        let remote_path = library.to_string_lossy();

        // Refused before anything is allocated: `LoadLibrary` on a module the target
        // already holds does not load a second copy, it bumps the reference count and
        // returns the base it is already at - so a repeat press used to start a second
        // remote thread, a second `DllMain` and a second bridge listener in a game that
        // already had one. The check is the target's own module list, not a record of
        // earlier presses.
        let module_name = library_file_name(library);
        if let Some(name) = module_name.as_deref() {
            match loaded_module_base(pid, name) {
                Ok(Some(base)) => {
                    return Err(PlatformError::os(
                        "LoadLibrary",
                        format!(
                            "{name} is already loaded in pid {pid} at {base:#x}; the bridge is \
                             injected once per process"
                        ),
                    ));
                }
                Ok(None) => {}
                // A snapshot that cannot be taken is not an answer about the module, and
                // refusing on it would block the first injection into a target that is
                // still starting. The press continues without the check.
                Err(error) => tracing::debug!(
                    %error,
                    "cannot read the target's module list before injecting; injecting unchecked"
                ),
            }
        }

        // Resolved before the process is opened, so a kernel32 that exports neither entry
        // point costs an error line rather than an allocation in somebody's game.
        let entry = load_library_entry()?;
        let path_bytes = entry.path_bytes(library);

        let process = open_target(pid)?;
        let mut allocation = RemoteAllocation::new(&process, path_bytes.len())?;

        let mut written = 0usize;
        // SAFETY: `allocation` covers `path_bytes.len()` bytes of committed,
        // writable memory in the target process and `path_bytes` is a live slice of
        // exactly that length.
        let ok = unsafe {
            WriteProcessMemory(
                process.get(),
                allocation.address,
                path_bytes.as_ptr().cast(),
                path_bytes.len(),
                &mut written,
            )
        };
        if !ok.as_bool() || written != path_bytes.len() {
            return Err(PlatformError::os(
                "WriteProcessMemory",
                format!(
                    "{written} of {} bytes: {}",
                    path_bytes.len(),
                    windows::core::Error::from_win32()
                ),
            ));
        }

        let start = entry.start();

        // SAFETY: `start` is the resolved `LoadLibrary` address inside kernel32, which is
        // mapped at the same address in the target's address space, and
        // `allocation.address` points at the NUL-terminated path it expects.
        let thread = OwnedHandle::from_result(unsafe {
            CreateRemoteThread(
                process.get(),
                None,
                0,
                start,
                Some(allocation.address.cast()),
                0,
                None,
            )
        })
        .map_err(|err| PlatformError::os("CreateRemoteThread", err.to_string()))?;

        // SAFETY: `thread` is an owned thread handle for a thread this call created.
        let wait = unsafe { WaitForSingleObject(thread.get(), REMOTE_THREAD_WAIT_MS) };
        if wait != WAIT_OBJECT_0 {
            // The remote thread has not returned, so it may still be about to read the
            // path. Freeing the block now would pull the string out from under
            // `LoadLibrary`, which is how an injector crashes the game it was attaching
            // to; it stays allocated as one page in the target, and the module check at
            // the top of this function refuses the next press once the module is there.
            allocation.forget();
            let detail = if wait == WAIT_TIMEOUT {
                format!(
                    "the target did not finish {} within {REMOTE_THREAD_WAIT_MS} ms \
                     (pid {pid}); the module may still load, and a press after it does finds it \
                     already loaded",
                    entry.name()
                )
            } else {
                format!(
                    "the remote thread handle signalled no exit (wait result {:#x}, pid {pid})",
                    wait.0
                )
            };
            return Err(PlatformError::os("WaitForSingleObject", detail));
        }

        // `LoadLibrary` returns the module handle, which is a thread's exit code: zero is
        // the target having refused the module, and the managed injector never looked, so
        // a 32/64-bit mismatch or a missing dependency was reported as a completed
        // injection and left the app waiting fifteen seconds for a bridge that was never
        // loaded. The exit code is 32 bits, so it is a yes/no answer and not an address.
        let mut module_handle = 0u32;
        // SAFETY: `thread` is still open and has signalled, so its exit code is available.
        unsafe { GetExitCodeThread(thread.get(), &mut module_handle) }
            .map_err(|err| PlatformError::os("GetExitCodeThread", format!("pid {pid}: {err}")))?;
        if module_handle == 0 {
            return Err(PlatformError::os(
                entry.name(),
                format!(
                    "pid {pid} refused {}; the target's {} returned no module \
                     handle, which is a 32/64-bit mismatch, a dependency it cannot find, or a \
                     library the game blocks",
                    remote_path,
                    entry.name()
                ),
            ));
        }

        // The base the target mapped the module at, read back from its module list: the
        // exit code above carries only the low half of a 64-bit handle, so it cannot be
        // reported as the address. `None` when the list cannot be re-read.
        let remote_base = module_name
            .as_deref()
            .and_then(|name| loaded_module_base(pid, name).ok().flatten());

        Ok(InjectionReport {
            pid,
            library: remote_path.into_owned(),
            remote_base,
        })
    }

    fn support(&self) -> BridgeSupport {
        BridgeSupport::Supported
    }
}

/// Opens `pid` with the rights the injection itself needs.
///
/// Three rights, one per call the handle is used for: `VirtualAllocEx`/`VirtualFreeEx`
/// take `PROCESS_VM_OPERATION`, `WriteProcessMemory` takes `PROCESS_VM_WRITE` with it, and
/// `CreateRemoteThread` takes `PROCESS_CREATE_THREAD`. The managed injector also asked for
/// `PROCESS_QUERY_INFORMATION` and `PROCESS_VM_READ`, and neither is used by anything this
/// handle does: the module list comes from `CreateToolhelp32Snapshot`, which opens a handle
/// of its own, so the duplicate check gains nothing from them - while asking for them made a
/// target that denies query rights refuse an injection the three below allow.
fn open_target(pid: u32) -> Result<OwnedHandle, PlatformError> {
    let least = PROCESS_CREATE_THREAD | PROCESS_VM_OPERATION | PROCESS_VM_WRITE;
    // SAFETY: `OpenProcess` takes the pid and access mask by value and returns an
    // owned handle or an error.
    match OwnedHandle::from_result(unsafe { OpenProcess(least, false, pid) }) {
        Ok(handle) => return Ok(handle),
        // Only a denial is worth a second attempt: a pid that is gone answers
        // `ERROR_INVALID_PARAMETER` on the first call and would answer it again.
        Err(err) if win32_error(&err) != ERROR_ACCESS_DENIED => return Err(open_error(pid, &err)),
        Err(_) => {}
    }

    // The managed mask, kept for the one case that documents a use for it: MSDN lists
    // `PROCESS_QUERY_INFORMATION` and `PROCESS_VM_READ` among `CreateRemoteThread`'s required
    // rights "on certain platforms", so a target that refuses the least set is retried with
    // the set the managed injector opened. A protected process refuses this one too, and the
    // error it reports is the one worth showing.
    let managed = least | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ;
    // SAFETY: as above.
    OwnedHandle::from_result(unsafe { OpenProcess(managed, false, pid) })
        .map_err(|err| open_error(pid, &err))
}

/// The classified sentence for an `OpenProcess` failure.
fn open_error(pid: u32, err: &windows::core::Error) -> PlatformError {
    match win32_error(err) {
        // A pid that has gone away and a protected process are different problems with
        // different remedies, and the managed injector collapsed both into "failed to open
        // process": a user who closed the game was told to check their integrity level.
        ERROR_INVALID_PARAMETER => PlatformError::not_found(format!(
            "pid {pid} is not running, so there is nothing to inject into"
        )),
        ERROR_ACCESS_DENIED => PlatformError::permission_denied(format!(
            "OpenProcess({pid}) was denied ({err}); run the host at the same integrity level as \
             the game, usually as the same user without elevation mismatch"
        )),
        _ => PlatformError::os("OpenProcess", format!("pid {pid}: {err}")),
    }
}

/// The Win32 error behind a `windows` error: `HRESULT_FROM_WIN32(e)` is
/// `0x8007_0000 | e`, so the low half of the code is `e` itself.
fn win32_error(error: &windows::core::Error) -> u32 {
    (error.code().0 as u32) & 0xFFFF
}

/// The name `LoadLibrary` registers the module under, which is its file name and not
/// its path - the target's module list holds the name, so that is what a duplicate
/// check compares. `None` for a path with no file name or one that is not UTF-8, which
/// leaves the duplicate to the OS.
fn library_file_name(library: &Path) -> Option<String> {
    library
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

/// The base address `pid` has `name` mapped at, from the target's own module list.
///
/// `Ok(None)` is the target not holding the module (or holding it under a name this
/// comparison does not match); an `Err` is a module snapshot that could not be taken at
/// all, which says nothing about the module either way.
fn loaded_module_base(pid: u32, name: &str) -> Result<Option<usize>, PlatformError> {
    // SAFETY: a module snapshot takes a flags value and a pid, needs no buffer, and
    // returns an owned handle or an error. `TH32CS_SNAPMODULE32` rides along because a
    // WOW64 target's 32-bit modules are only listed when it is asked for.
    let snapshot = OwnedHandle::from_result(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    })
    .map_err(|err| {
        PlatformError::os(
            "CreateToolhelp32Snapshot(modules)",
            format!("pid {pid}: {err}"),
        )
    })?;

    let mut entry = MODULEENTRY32W {
        dwSize: u32::try_from(core::mem::size_of::<MODULEENTRY32W>())
            .map_err(|_| PlatformError::os("MODULEENTRY32W", "size does not fit a u32"))?,
        ..Default::default()
    };

    // SAFETY: `entry` is a live, correctly sized `MODULEENTRY32W` with `dwSize` set, and
    // `snapshot` is the owned toolhelp handle above.
    if unsafe { Module32FirstW(snapshot.get(), &mut entry) }.is_err() {
        // A target that is still starting has no module list to walk yet.
        return Ok(None);
    }

    loop {
        if wide_name(&entry.szModule).eq_ignore_ascii_case(name) {
            return Ok(Some(entry.modBaseAddr as usize));
        }
        // SAFETY: same invariants as the first call; `entry` is reused.
        if unsafe { Module32NextW(snapshot.get(), &mut entry) }.is_err() {
            return Ok(None);
        }
    }
}

/// A Win32 fixed-size UTF-16 name, up to its terminator.
fn wide_name(units: &[u16]) -> String {
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(&units[..length])
}

/// The shape `GetProcAddress` hands back: a bare `unsafe extern "system" fn() -> isize`.
type ProcAddress = unsafe extern "system" fn() -> isize;

/// `LoadLibrary`'s own shape, which `CreateRemoteThread` calls an entry point with.
type LoadLibrary = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;

/// The `LoadLibrary` the remote thread runs, and with it the encoding of the path it reads.
///
/// `LoadLibraryW` is the one that can name a library under a profile directory the host can
/// open: it takes UTF-16, where `LoadLibraryA` goes through the target's ANSI code page and
/// turns every character outside it into `?` - so an accent, a Cyrillic or a CJK user name
/// reached the game as a path that does not exist. A stays as the fallback for a kernel32
/// that will not resolve W, which is what the managed injector always used.
#[derive(Debug, Clone, Copy)]
enum LibraryEntry {
    /// `LoadLibraryW`, reading a NUL-terminated UTF-16 path.
    Wide(LPTHREAD_START_ROUTINE),
    /// `LoadLibraryA`, reading a NUL-terminated ANSI path.
    Ansi(LPTHREAD_START_ROUTINE),
}

impl LibraryEntry {
    /// The export this entry point came from, for the lines that name it.
    fn name(self) -> &'static str {
        match self {
            Self::Wide(_) => "LoadLibraryW",
            Self::Ansi(_) => "LoadLibraryA",
        }
    }

    /// The address the remote thread starts on.
    fn start(self) -> LPTHREAD_START_ROUTINE {
        match self {
            Self::Wide(start) | Self::Ansi(start) => start,
        }
    }

    /// The path as this entry point has to read it, NUL-terminated.
    ///
    /// The length of what comes back is the remote allocation's size, so the two encodings
    /// cannot drift apart from the sizes they claim: a wide path is two bytes per code unit
    /// plus the two of its terminator, an ANSI path is one byte per character.
    fn path_bytes(self, path: &Path) -> Vec<u8> {
        match self {
            Self::Wide(_) => wide_path_bytes(path),
            Self::Ansi(_) => ascii_path_bytes(path),
        }
    }
}

/// `GetProcAddress(GetModuleHandleW("kernel32.dll"), "LoadLibraryW")`, falling back to A.
fn load_library_entry() -> Result<LibraryEntry, PlatformError> {
    // SAFETY: the module name is a static NUL-terminated wide string that outlives
    // the call, and kernel32 is always loaded in our own process.
    let kernel32 = unsafe { GetModuleHandleW(windows::core::w!("kernel32.dll")) }
        .map_err(|err| PlatformError::os("GetModuleHandleW(kernel32.dll)", err.to_string()))?;

    // SAFETY: `kernel32` is a live module handle and each export name is a static
    // NUL-terminated ANSI string.
    if let Some(address) = unsafe { GetProcAddress(kernel32, windows::core::s!("LoadLibraryW")) } {
        return Ok(LibraryEntry::Wide(thread_start_routine(address)));
    }

    // SAFETY: as above.
    let address = unsafe { GetProcAddress(kernel32, windows::core::s!("LoadLibraryA")) };
    let address = address.ok_or_else(|| {
        PlatformError::os(
            "GetProcAddress",
            "kernel32 exports neither LoadLibraryW nor LoadLibraryA",
        )
    })?;
    Ok(LibraryEntry::Ansi(thread_start_routine(address)))
}

/// The transmute that gives a resolved export the `THREAD_START_ROUTINE` shape.
fn thread_start_routine(address: ProcAddress) -> LPTHREAD_START_ROUTINE {
    // SAFETY: both types are plain function pointers of identical size and
    // alignment; only the parameter list and return width differ, which is the
    // ABI `CreateRemoteThread` calls the entry point with.
    let start: LoadLibrary = unsafe { core::mem::transmute::<ProcAddress, LoadLibrary>(address) };
    LPTHREAD_START_ROUTINE::Some(start)
}

/// Remote memory that frees itself with `MEM_RELEASE`, the C# `finally` block.
///
/// The process handle is borrowed rather than copied, so the borrow checker - not
/// the order of two `let` statements - guarantees the handle outlives the
/// allocation. A copy would make the `Drop` `VirtualFreeEx` a use-after-close the
/// moment the two declarations were swapped.
struct RemoteAllocation<'a> {
    process: &'a OwnedHandle,
    address: *mut core::ffi::c_void,
}

impl<'a> RemoteAllocation<'a> {
    fn new(process: &'a OwnedHandle, size: usize) -> Result<Self, PlatformError> {
        // SAFETY: a null requested address lets the OS choose, and the size and
        // protection flags are the documented pair for a writable scratch buffer.
        let address = unsafe {
            VirtualAllocEx(
                process.get(),
                None,
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if address.is_null() {
            return Err(PlatformError::os(
                "VirtualAllocEx",
                windows::core::Error::from_win32().to_string(),
            ));
        }
        Ok(Self { process, address })
    }

    /// Leaves the remote block allocated, so `Drop` does not free it.
    ///
    /// Only correct while the remote thread may still read the block - a module that has
    /// not returned from `LoadLibrary` yet is still going to read the path at
    /// `self.address`, and a freed block would be the string it loads.
    fn forget(&mut self) {
        self.address = core::ptr::null_mut();
    }
}

impl Drop for RemoteAllocation<'_> {
    fn drop(&mut self) {
        if self.address.is_null() {
            return;
        }
        // SAFETY: `address` came from `VirtualAllocEx` on `self.process`, whose
        // borrow the compiler enforces is still live here, and has not been freed;
        // `MEM_RELEASE` requires a zero size.
        unsafe {
            let _ = VirtualFreeEx(self.process.get(), self.address, 0, MEM_RELEASE);
        }
        self.address = core::ptr::null_mut();
    }
}

/// `Encoding.ASCII.GetBytes(path + "\0")`: one byte per char, with every
/// non-ASCII char replaced by `?`, which is how .NET's ASCII encoder behaves.
///
/// Only the [`LibraryEntry::Ansi`] fallback reads this. UTF-8 bytes would differ for
/// any non-ASCII path, and `LoadLibraryA` reads the buffer in the ANSI code page, so the
/// managed encoding is the one to match - and the reason a non-ASCII profile directory
/// cannot be injected through that entry point.
fn ascii_path_bytes(path: &Path) -> Vec<u8> {
    let text = path.to_string_lossy();
    let mut bytes = Vec::with_capacity(text.len() + 1);
    for character in text.chars() {
        let code = u32::from(character);
        bytes.push(if code < 0x80 { code as u8 } else { b'?' });
    }
    bytes.push(0);
    bytes
}

/// A NUL-terminated UTF-16 path, two little-endian bytes per code unit, which is what
/// `LoadLibraryW` reads.
///
/// `OsStr::encode_wide` is the path as the OS holds it, so this is not a `to_string_lossy`
/// round trip: a name with an unpaired surrogate - legal on NTFS - reaches the target as
/// itself. The length is two bytes per unit including the terminator, and the remote
/// allocation is sized from the same number the bytes were built with.
fn wide_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str()
        .encode_wide()
        .chain(core::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remote_path_is_ascii_with_a_terminator() {
        let bytes = ascii_path_bytes(Path::new(r"C:\bridge\aoko.dll"));
        assert_eq!(bytes, b"C:\\bridge\\aoko.dll\0");
    }

    #[test]
    fn a_non_ascii_path_is_question_marked_like_the_managed_encoder() {
        // .NET's ASCII encoder emits one `?` per non-ASCII char; UTF-8 would emit
        // two bytes here and `LoadLibraryA` would read a different path.
        let bytes = ascii_path_bytes(Path::new("C:/a/b\u{00ef}c.dll"));
        assert_eq!(bytes, b"C:/a/b?c.dll\0");
    }

    /// The path the wide entry point reads: every character, two bytes per code unit, and
    /// a terminator of its own — the allocation is sized from this length, so a short
    /// buffer would be `LoadLibraryW` reading past the string it was given.
    #[test]
    fn a_wide_path_keeps_every_character_and_terminates_itself() {
        let path = "C:/Users/\u{4e2d}\u{6587}/aoko.dll";
        let bytes = wide_path_bytes(Path::new(path));

        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(bytes.len() % 2, 0, "a UTF-16 buffer is whole code units");
        assert_eq!(bytes.len(), (path.encode_utf16().count() + 1) * 2);
        assert_eq!(*units.last().expect("a terminator unit"), 0);
        assert_eq!(
            String::from_utf16(&units[..units.len() - 1]).expect("valid UTF-16"),
            path,
            "the wide path is the path, not a question-marked approximation"
        );
    }

    /// The two encodings are sized by the same call that builds them, and the wide one is
    /// the one that carries the bytes: an accent is two bytes for W and a `?` for A.
    #[test]
    fn the_entry_point_picks_the_encoding_of_the_entry_point() {
        let path = Path::new("C:/a/b\u{00ef}c.dll");
        let ansi = LibraryEntry::Ansi(LPTHREAD_START_ROUTINE::None);
        let wide = LibraryEntry::Wide(LPTHREAD_START_ROUTINE::None);

        assert_eq!(ansi.name(), "LoadLibraryA");
        assert_eq!(wide.name(), "LoadLibraryW");
        assert_eq!(ansi.path_bytes(path), b"C:/a/b?c.dll\0");
        // Twelve code units for the path plus its terminator, two bytes each.
        assert_eq!(wide.path_bytes(path).len(), 13 * 2);
    }

    #[test]
    fn a_missing_library_is_reported_before_any_process_is_opened() {
        let err = WindowsBridgeInjector::new()
            .inject(1, Path::new(r"C:\definitely\not\here.dll"))
            .expect_err("missing library");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }

    /// The duplicate check compares against the target's module list, which holds the file
    /// name: a path that names the library in a directory still has to reduce to it.
    #[test]
    fn the_duplicate_check_uses_the_file_name_not_the_path() {
        assert_eq!(
            library_file_name(Path::new(r"C:\Program Files\Aoko\bridge.dll")).as_deref(),
            Some("bridge.dll")
        );
        assert_eq!(
            library_file_name(Path::new(r"C:\Aoko\bridge_261.dll")).as_deref(),
            Some("bridge_261.dll")
        );
        assert_eq!(library_file_name(Path::new("")), None);
    }

    /// `MODULEENTRY32W::szModule` is a fixed 256-unit array padded with zeros, and the
    /// comparison is only right when it stops at the terminator.
    #[test]
    fn a_wide_module_name_stops_at_its_terminator() {
        let mut units = [0u16; 256];
        for (slot, unit) in units.iter_mut().zip("bridge_261.dll".encode_utf16()) {
            *slot = unit;
        }
        assert_eq!(wide_name(&units), "bridge_261.dll");
        assert!(wide_name(&units).eq_ignore_ascii_case("BRIDGE_261.DLL"));
    }
}
