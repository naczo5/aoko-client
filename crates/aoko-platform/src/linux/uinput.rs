//! `/dev/uinput` synthesis, for sessions where XTest is not available.
//!
//! A Wayland session has no global input API a plain client may call, but the
//! kernel's `uinput` interface is the same one the compositor itself uses, so a
//! virtual keyboard/pointer created here is delivered everywhere. It needs write
//! access to `/dev/uinput`, which normally means membership of the `input` group
//! or a udev rule.
//!
//! Linux `input_event` layout assumed below is the 64-bit one: two 8-byte
//! `timeval` fields (16 bytes), then `u16 type`, `u16 code`, `i32 value`.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::unix::io::AsRawFd as _;

use crate::error::PlatformError;

pub(crate) const EV_SYN: u16 = 0;
pub(crate) const EV_KEY: u16 = 1;
pub(crate) const EV_REL: u16 = 2;

pub(crate) const SYN_REPORT: u16 = 0;
pub(crate) const REL_X: u16 = 0;
pub(crate) const REL_Y: u16 = 1;

pub(crate) const BTN_LEFT: u16 = 0x110;
pub(crate) const BTN_RIGHT: u16 = 0x111;
pub(crate) const BTN_MIDDLE: u16 = 0x112;

/// Device node. Overridable through `AOKO_UINPUT_PATH` for tests and containers.
fn device_path() -> std::path::PathBuf {
    std::env::var_os("AOKO_UINPUT_PATH").map_or_else(|| "/dev/uinput".into(), Into::into)
}

/// `_IOC(dir, type, nr, size)` from `asm-generic/ioctl.h`.
const fn ioc(direction: u32, kind: u8, number: u8, size: u32) -> u64 {
    ((direction as u64) << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | (number as u64)
}

const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

/// `_IOC(_IOC_READ, kind, number, size)`, used by the evdev capability query.
pub(crate) const fn ioc_read(kind: u8, number: u8, size: u32) -> u64 {
    ioc(IOC_READ, kind, number, size)
}

const UI_DEV_CREATE: u64 = ioc(0, b'U', 1, 0);
const UI_DEV_DESTROY: u64 = ioc(0, b'U', 2, 0);
const UI_SET_EVBIT: u64 = ioc(IOC_WRITE, b'U', 100, 4);
const UI_SET_KEYBIT: u64 = ioc(IOC_WRITE, b'U', 101, 4);
const UI_SET_RELBIT: u64 = ioc(IOC_WRITE, b'U', 102, 4);

const UINPUT_MAX_NAME_SIZE: usize = 80;
const ABS_CNT: usize = 64;

/// `struct uinput_user_dev` from `linux/uinput.h`.
#[repr(C)]
struct UinputUserDev {
    name: [u8; UINPUT_MAX_NAME_SIZE],
    bus_type: u16,
    vendor: u16,
    product: u16,
    version: u16,
    ff_effects_max: u32,
    absmax: [i32; ABS_CNT],
    absmin: [i32; ABS_CNT],
    absfuzz: [i32; ABS_CNT],
    absflat: [i32; ABS_CNT],
}

/// `struct input_event` for the 64-bit layout.
#[repr(C)]
pub(crate) struct RawInputEvent {
    pub tv_sec: i64,
    pub tv_usec: i64,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

/// Whether `/dev/uinput` can be opened read-write, which is the only permission
/// that matters for creating a virtual device.
#[must_use]
pub fn is_available() -> bool {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(device_path())
        .is_ok()
}

/// A created virtual input device. Dropping it destroys the device node.
pub(crate) struct UinputDevice {
    file: File,
    created: bool,
}

impl UinputDevice {
    /// Creates a named virtual device able to send `keys`, relative motion and
    /// any of `buttons`.
    pub fn create(name: &str, keys: &[u16], buttons: &[u16]) -> Result<Self, PlatformError> {
        let path = device_path();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| {
                let detail = format!("{}: {err}", path.display());
                if err.kind() == std::io::ErrorKind::PermissionDenied {
                    // Only a real `EACCES`/`EPERM` earns the group remedy; a missing
                    // node means the module is not loaded, which is `Os`, not a
                    // permission problem.
                    PlatformError::permission_denied(format!(
                        "{detail} (add your user to the 'input' group, or install a udev rule \
                         granting rw-rw---- on /dev/uinput)"
                    ))
                } else {
                    PlatformError::os(
                        "open /dev/uinput",
                        format!("{detail} (load the module with `modprobe uinput`)"),
                    )
                }
            })?;

        let descriptor = file.as_raw_fd();
        for event_type in [EV_KEY, EV_REL, EV_SYN] {
            enable(&file, UI_SET_EVBIT, u64::from(event_type))?;
        }
        for key in keys.iter().chain(buttons) {
            enable(&file, UI_SET_KEYBIT, u64::from(*key))?;
        }
        enable(&file, UI_SET_RELBIT, u64::from(REL_X))?;
        enable(&file, UI_SET_RELBIT, u64::from(REL_Y))?;

        let mut definition = UinputUserDev {
            name: [0; UINPUT_MAX_NAME_SIZE],
            // BUS_VIRTUAL, so the compositor logs it as a virtual device.
            bus_type: 0x06,
            vendor: 0x1,
            product: 0x1,
            version: 1,
            ff_effects_max: 0,
            absmax: [0; ABS_CNT],
            absmin: [0; ABS_CNT],
            absfuzz: [0; ABS_CNT],
            absflat: [0; ABS_CNT],
        };
        let name_bytes = name.as_bytes();
        let length = name_bytes.len().min(UINPUT_MAX_NAME_SIZE - 1);
        definition.name[..length].copy_from_slice(&name_bytes[..length]);

        // SAFETY: `definition` is a live, correctly laid-out `uinput_user_dev`
        // and the descriptor comes from the `File` we own for this call.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(&definition).cast::<u8>(),
                std::mem::size_of::<UinputUserDev>(),
            )
        };
        file.write_all(bytes)
            .map_err(|err| PlatformError::os("write uinput_user_dev", err.to_string()))?;

        // SAFETY: `descriptor` is the uinput node this `File` still owns, and
        // UI_DEV_CREATE takes no pointer argument.
        let created = unsafe { libc::ioctl(descriptor, UI_DEV_CREATE as libc::c_ulong) } == 0;
        if !created {
            return Err(PlatformError::os(
                "UI_DEV_CREATE",
                std::io::Error::last_os_error().to_string(),
            ));
        }

        Ok(Self {
            file,
            created: true,
        })
    }

    /// Emits a key press/release or button press/release.
    pub fn emit_key(&mut self, code: u16, pressed: bool) -> Result<(), PlatformError> {
        self.emit(EV_KEY, code, i32::from(pressed))?;
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    pub fn emit_relative(&mut self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        if dx != 0 {
            self.emit(EV_REL, REL_X, dx)?;
        }
        if dy != 0 {
            self.emit(EV_REL, REL_Y, dy)?;
        }
        self.emit(EV_SYN, SYN_REPORT, 0)
    }

    fn emit(&mut self, type_: u16, code: u16, value: i32) -> Result<(), PlatformError> {
        let event = RawInputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_,
            code,
            value,
        };
        // SAFETY: `event` is a live, correctly laid-out `input_event` and the
        // kernel copies exactly `size_of::<RawInputEvent>()` bytes from it.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(&event).cast::<u8>(),
                std::mem::size_of::<RawInputEvent>(),
            )
        };
        self.file
            .write_all(bytes)
            .map_err(|err| PlatformError::os("write input_event", err.to_string()))
    }
}

impl Drop for UinputDevice {
    fn drop(&mut self) {
        if !self.created {
            return;
        }
        // SAFETY: the descriptor is still owned by `self.file`, so it is valid
        // here, and UI_DEV_DESTROY takes no pointer argument.
        let result = unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as libc::c_ulong) };
        if result != 0 {
            tracing::debug!("UI_DEV_DESTROY failed: {}", std::io::Error::last_os_error());
        }
        self.created = false;
    }
}

fn enable(file: &File, request: u64, bit: u64) -> Result<(), PlatformError> {
    // SAFETY: `request` is one of the UI_SET_*BIT ioctls, whose argument is
    // passed by value as an integer, which is the ABI documented in
    // `linux/uinput.h`.
    let result = unsafe { libc::ioctl(file.as_raw_fd(), request as libc::c_ulong, bit) };
    if result == 0 {
        Ok(())
    } else {
        Err(PlatformError::os(
            "UI_SET_*BIT",
            std::io::Error::last_os_error().to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_kernel_encodings() {
        // Hand-computed from asm-generic/ioctl.h for `U` = 0x55.
        assert_eq!(UI_DEV_CREATE, 0x0000_5501);
        assert_eq!(UI_DEV_DESTROY, 0x0000_5502);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(UI_SET_RELBIT, 0x4004_5566);
    }

    #[test]
    fn raw_input_event_is_the_64_bit_layout() {
        assert_eq!(std::mem::size_of::<RawInputEvent>(), 24);
        assert_eq!(std::mem::align_of::<RawInputEvent>(), 8);
    }

    #[test]
    fn uinput_user_dev_matches_the_header_size() {
        assert_eq!(std::mem::size_of::<UinputUserDev>(), 1116);
    }

    #[test]
    fn the_probe_reports_availability_without_creating_a_device() {
        // `is_available` only opens the node; no UI_DEV_CREATE is issued, so this
        // is safe to run in CI where /dev/uinput is usually absent.
        let available = is_available();
        if !available {
            assert!(UinputDevice::create("aoko-test", &[], &[]).is_err());
        }
    }
}
