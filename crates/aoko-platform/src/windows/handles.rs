//! RAII wrappers for Win32 handles.
//!
//! Every handle the platform opens is owned by one of these, so an early return
//! on an error path still closes it. Nothing in this crate calls `CloseHandle`
//! directly.

use windows::Win32::Foundation::{CloseHandle, HANDLE};

/// A kernel handle that closes itself.
pub(crate) struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Takes ownership of `handle`, or reports `None` for a null/invalid one.
    pub fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_invalid()).then_some(Self(handle))
    }

    /// Takes ownership of a `Result`-returning Win32 call's handle.
    pub fn from_result(
        result: windows::core::Result<HANDLE>,
    ) -> Result<Self, windows::core::Error> {
        let handle = result?;
        Self::new(handle).ok_or_else(windows::core::Error::from_win32)
    }

    /// The raw handle, still owned by `self`.
    pub fn get(&self) -> HANDLE {
        self.0
    }

    /// Gives up ownership, handing the raw handle to another owner (e.g.
    /// `std::fs::File`). The caller becomes responsible for closing it.
    pub fn into_raw(self) -> HANDLE {
        let handle = self.0;
        core::mem::forget(self);
        handle
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0.is_invalid() {
            return;
        }
        // SAFETY: `self.0` is a live handle this wrapper owns and has not closed;
        // closing it once is the whole contract of `Drop`.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl core::fmt::Debug for OwnedHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "OwnedHandle({:?})", self.0 .0)
    }
}
