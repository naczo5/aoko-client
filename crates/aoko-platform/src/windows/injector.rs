//! `LoadLibrary` injection, ported from `Aoko/Core/NativeInjector.cs`.
//!
//! The sequence is the managed one: open the process with the five rights the
//! injector needs, allocate `path + NUL` in the target as ASCII, write it, resolve
//! `LoadLibraryA` from `kernel32`, create a remote thread on it, wait for the
//! thread, free the buffer, close everything.
//!
//! Every handle and the remote allocation are RAII-owned, so the error paths free
//! them in the same order the C# `finally` block did.

use std::path::Path;

use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, LPTHREAD_START_ROUTINE,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE,
};

use crate::{BridgeSupport, InjectionReport, NativeBridgeInjector, PlatformError};

use super::handles::OwnedHandle;
use super::raw::WriteProcessMemory;

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

        // The remote side receives an ANSI path because the thread entry point is
        // `LoadLibraryA`, exactly as the managed injector sends it.
        let remote_path = library.to_string_lossy();
        let path_bytes = ascii_path_bytes(library);

        let process = open_target(pid)?;
        let allocation = RemoteAllocation::new(&process, path_bytes.len())?;

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

        let start = load_library_a_address()?;

        // SAFETY: `start` is `LoadLibraryA`'s address inside kernel32, which is
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
        let wait = unsafe { WaitForSingleObject(thread.get(), u32::MAX) };
        if wait != WAIT_OBJECT_0 {
            tracing::debug!(
                "remote thread did not signal completion (wait result {:#x}); the module may \
                 still have loaded",
                wait.0
            );
        }

        Ok(InjectionReport {
            pid,
            library: remote_path.into_owned(),
            // The managed injector never read the module base back; reporting
            // `None` keeps the same information content instead of inventing one.
            remote_base: None,
        })
    }

    fn support(&self) -> BridgeSupport {
        BridgeSupport::Supported
    }
}

fn open_target(pid: u32) -> Result<OwnedHandle, PlatformError> {
    let access = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;
    // SAFETY: `OpenProcess` takes the pid and access mask by value and returns an
    // owned handle or an error.
    let handle = unsafe { OpenProcess(access, false, pid) };
    OwnedHandle::from_result(handle).map_err(|err| {
        PlatformError::permission_denied(format!(
            "OpenProcess({pid}) failed ({err}); run the host at the same integrity level as the \
             game, usually as the same user without elevation mismatch"
        ))
    })
}

/// `GetProcAddress(GetModuleHandleA("kernel32.dll"), "LoadLibraryA")`.
fn load_library_a_address() -> Result<LPTHREAD_START_ROUTINE, PlatformError> {
    // SAFETY: the module name is a static NUL-terminated wide string that outlives
    // the call, and kernel32 is always loaded in our own process.
    let kernel32 = unsafe { GetModuleHandleW(windows::core::w!("kernel32.dll")) }
        .map_err(|err| PlatformError::os("GetModuleHandleW(kernel32.dll)", err.to_string()))?;

    // SAFETY: `kernel32` is a live module handle and the export name is a static
    // NUL-terminated ANSI string.
    let address = unsafe { GetProcAddress(kernel32, windows::core::s!("LoadLibraryA")) };
    let address =
        address.ok_or_else(|| PlatformError::os("GetProcAddress", "LoadLibraryA was not found"))?;

    // `address` is what `FARPROC` points at: a bare `unsafe extern "system" fn()
    // -> isize`. `LoadLibraryA` has the `THREAD_START_ROUTINE` shape, so the
    // transmute only fixes up the declared signature of the same code address.
    type ProcAddress = unsafe extern "system" fn() -> isize;
    type LoadLibraryA = unsafe extern "system" fn(*mut core::ffi::c_void) -> u32;
    // SAFETY: both types are plain function pointers of identical size and
    // alignment; only the parameter list and return width differ, which is the
    // ABI `CreateRemoteThread` calls the entry point with.
    let start: LoadLibraryA = unsafe { core::mem::transmute::<ProcAddress, LoadLibraryA>(address) };
    Ok(LPTHREAD_START_ROUTINE::Some(start))
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
/// UTF-8 bytes would differ for any non-ASCII path, and `LoadLibraryA` reads the
/// buffer in the ANSI code page, so the managed encoding is the one to match.
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

    #[test]
    fn a_missing_library_is_reported_before_any_process_is_opened() {
        let err = WindowsBridgeInjector::new()
            .inject(1, Path::new(r"C:\definitely\not\here.dll"))
            .expect_err("missing library");
        assert!(matches!(err, PlatformError::NotFound { .. }));
    }
}
