//! Raw bindings for the four Win32/ntdll calls the `windows` crate's enabled
//! feature set does not expose.
//!
//! `windows` 0.58 puts `ReadProcessMemory`/`WriteProcessMemory` behind
//! `Win32_System_Diagnostics_Debug`, `ClientToScreen` behind `Win32_Graphics_Gdi`
//! and `NtQueryInformationProcess` in the WDK metadata. The workspace pins the
//! feature list, so those four are declared here instead of widening it.
//!
//! They are declared, not re-invented: signatures follow
//! `winternl.h`/`memoryapi.h`/`winuser.h` exactly.

#![allow(non_snake_case)]

use core::ffi::c_void;

use windows::Win32::Foundation::{BOOL, HANDLE, HWND, NTSTATUS, POINT};

/// `PROCESS_BASIC_INFORMATION` from `winternl.h`, x64 layout.
#[repr(C)]
pub(crate) struct ProcessBasicInformation {
    pub reserved1: usize,
    pub peb_base_address: usize,
    pub reserved2_0: usize,
    pub reserved2_1: usize,
    pub unique_process_id: usize,
    pub reserved3: usize,
}

#[link(name = "ntdll")]
extern "system" {
    pub fn NtQueryInformationProcess(
        process_handle: HANDLE,
        process_information_class: u32,
        process_information: *mut c_void,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> NTSTATUS;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn ReadProcessMemory(
        process: HANDLE,
        base_address: *const c_void,
        buffer: *mut c_void,
        size: usize,
        bytes_read: *mut usize,
    ) -> BOOL;

    pub fn WriteProcessMemory(
        process: HANDLE,
        base_address: *mut c_void,
        buffer: *const c_void,
        size: usize,
        bytes_written: *mut usize,
    ) -> BOOL;
}

#[link(name = "user32")]
extern "system" {
    pub fn ClientToScreen(window: HWND, point: *mut POINT) -> BOOL;
}
