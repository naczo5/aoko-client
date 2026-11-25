//! Global input observation through `/dev/input/event*`.
//!
//! One background thread `poll(2)`s every readable keyboard/pointer node and
//! translates `input_event` records into [`InputEvent`]s. Reading evdev needs
//! access to the device nodes, which normally means membership of the `input`
//! group; when no node is readable the observer reports
//! [`PlatformError::PermissionDenied`] with the fix rather than going quiet.
//!
//! Pointer motion arrives as relative deltas, so absolute positions are
//! accumulated from a seed: the X11 pointer when a display is available, the
//! origin otherwise. Both facts are reported through the type, not guessed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::error::PlatformError;
use crate::evdev_table::key_from_code;
use crate::input::{InputEvent, MouseButton};

use super::uinput::RawInputEvent;
use super::x11::X11;

pub(crate) const EV_SYN: u16 = 0;
pub(crate) const EV_KEY: u16 = 1;
pub(crate) const EV_REL: u16 = 2;
pub(crate) const EV_ABS: u16 = 3;

pub(crate) const SYN_DROPPED: u16 = 3;

pub(crate) const BTN_LEFT: u16 = 0x110;
pub(crate) const BTN_RIGHT: u16 = 0x111;
pub(crate) const BTN_MIDDLE: u16 = 0x112;

const QUEUE_CAPACITY: usize = 4096;
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

/// One readable event device that carries keys or relative motion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub path: String,
    pub has_keys: bool,
    pub has_relative: bool,
}

/// Lists `/dev/input/event*` nodes that can be opened and look like a keyboard or
/// a pointer.
///
/// Devices that report absolute axes are skipped: they are gamepads, touchpads and
/// tablets, whose button noise the automation loop does not want.
#[must_use]
pub fn enumerate_devices() -> Vec<InputDevice> {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return Vec::new();
    };

    let mut devices: Vec<InputDevice> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !name.starts_with("event") {
                return None;
            }
            let path = entry.path();
            let file = std::fs::File::open(&path).ok()?;
            let (has_keys, has_relative, has_absolute) = capability_bits(&file)?;
            if has_absolute || (!has_keys && !has_relative) {
                return None;
            }
            Some(InputDevice {
                path: path.to_string_lossy().into_owned(),
                has_keys,
                has_relative,
            })
        })
        .collect();
    devices.sort_by(|left, right| left.path.cmp(&right.path));
    devices
}

/// `EVIOCGBIT(0, 8)` yields a bitmap of supported event types.
fn capability_bits(file: &std::fs::File) -> Option<(bool, bool, bool)> {
    use std::os::unix::io::AsRawFd as _;

    let mut bits = [0u8; 8];
    // SAFETY: `bits` is a live 8-byte buffer and `EVIOCGBIT(0, 8)` writes exactly
    // 8 bytes into it, which is the ABI documented in `linux/input.h`.
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            super::uinput::ioc_read(b'E', 0x20, 8) as libc::c_ulong,
            bits.as_mut_ptr(),
        )
    };
    if result < 0 {
        return None;
    }
    let has = |event_type: u16| bits[usize::from(event_type) / 8] & (1 << (event_type % 8)) != 0;
    Some((has(EV_KEY), has(EV_REL), has(EV_ABS)))
}

/// A running observer. Dropping it stops the reader thread and joins it.
pub(crate) struct EvdevObserver {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    receiver: Receiver<InputEvent>,
}

impl EvdevObserver {
    /// Starts one reader thread over every usable device.
    pub fn start(x11: Option<&X11>) -> Result<Self, PlatformError> {
        let devices = enumerate_devices();
        if devices.is_empty() {
            return Err(PlatformError::permission_denied(
                "no readable input device under /dev/input/event*: add your user to the 'input' \
                 group (sudo usermod -aG input $USER) and re-login, or run the host with \
                 CAP_DAC_READ_SEARCH to observe global input",
            ));
        }

        let seed = x11.and_then(X11::pointer).map(|(x, y, _)| (x, y));
        let (sender, receiver) = crossbeam_channel::bounded(QUEUE_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let paths: Vec<String> = devices.iter().map(|device| device.path.clone()).collect();

        let handle = std::thread::Builder::new()
            .name("aoko-evdev".to_string())
            .spawn(move || read_loop(&paths, seed, sender, &thread_stop))
            .map_err(|err| PlatformError::os("spawn evdev reader", err.to_string()))?;

        Ok(Self {
            stop,
            handle: Some(handle),
            receiver,
        })
    }

    /// Drains queued events into `out`. Never blocks.
    pub fn drain_into(&self, out: &mut Vec<InputEvent>) -> usize {
        let mut drained = 0usize;
        while let Ok(event) = self.receiver.try_recv() {
            out.push(event);
            drained += 1;
        }
        drained
    }
}

impl Drop for EvdevObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                tracing::debug!("evdev reader thread ended with a panic");
            }
        }
    }
}

/// Signed descriptor set for `poll(2)`.
struct PollSet {
    fds: Vec<libc::pollfd>,
    paths: Vec<String>,
    buffers: Vec<Vec<u8>>,
}

fn read_loop(
    paths: &[String],
    seed: Option<(i32, i32)>,
    sender: Sender<InputEvent>,
    stop: &AtomicBool,
) {
    let mut set = PollSet {
        fds: Vec::with_capacity(paths.len()),
        paths: Vec::with_capacity(paths.len()),
        buffers: Vec::with_capacity(paths.len()),
    };

    for path in paths {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        let descriptor = {
            use std::os::unix::io::IntoRawFd as _;
            file.into_raw_fd()
        };
        set.fds.push(libc::pollfd {
            fd: descriptor,
            events: libc::POLLIN,
            revents: 0,
        });
        set.paths.push(path.clone());
        set.buffers
            .push(vec![0u8; std::mem::size_of::<RawInputEvent>() * 64]);
    }

    let mut position = seed.unwrap_or((0, 0));

    while !stop.load(Ordering::Acquire) {
        if set.fds.is_empty() {
            break;
        }
        // SAFETY: `fds` is a live slice of `pollfd` and the length passed matches
        // it, which is `poll(2)`'s contract.
        let ready = unsafe { libc::poll(set.fds.as_mut_ptr(), set.fds.len() as libc::nfds_t, 100) };
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            tracing::debug!("evdev poll failed: {err}");
            break;
        }
        if ready == 0 {
            continue;
        }

        for index in 0..set.fds.len() {
            if set.fds[index].revents & libc::POLLIN == 0 {
                continue;
            }
            let buffer = &mut set.buffers[index];
            // SAFETY: `buffer` is a live, initialised byte buffer and `read` fills
            // at most its length.
            let read = unsafe {
                libc::read(
                    set.fds[index].fd,
                    buffer.as_mut_ptr().cast::<libc::c_void>(),
                    buffer.len(),
                )
            };
            if read <= 0 {
                continue;
            }
            let byte_count = read as usize;
            let event_size = std::mem::size_of::<RawInputEvent>();
            let mut offset = 0usize;
            let mut moved = false;

            while offset + event_size <= byte_count {
                let Some(bytes) = buffer.get(offset..offset + event_size) else {
                    break;
                };
                offset += event_size;
                // SAFETY: `RawInputEvent` is `repr(C)` with only integer fields, so
                // any bit pattern of the right size is a valid value; the slice has
                // exactly that size and alignment is guaranteed by the bounds check.
                let event =
                    unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<RawInputEvent>()) };
                match (event.type_, event.code) {
                    (EV_KEY, BTN_LEFT) => {
                        handle_button(MouseButton::Left, event.value, &sender, stop);
                    }
                    (EV_KEY, BTN_RIGHT) => {
                        handle_button(MouseButton::Right, event.value, &sender, stop);
                    }
                    (EV_KEY, BTN_MIDDLE) => {
                        handle_button(MouseButton::Middle, event.value, &sender, stop);
                    }
                    (EV_KEY, code) => {
                        let key = key_from_code(code);
                        let event = if event.value == 0 {
                            InputEvent::KeyUp(key)
                        } else {
                            InputEvent::KeyDown(key)
                        };
                        send(&sender, event, stop);
                    }
                    (EV_REL, 0) => {
                        position.0 = position.0.saturating_add(event.value);
                        moved = true;
                    }
                    (EV_REL, 1) => {
                        position.1 = position.1.saturating_add(event.value);
                        moved = true;
                    }
                    (EV_SYN, SYN_DROPPED) => {
                        // Kernel dropped events; the accumulated position is no
                        // longer trustworthy, so the next seed is left to the next
                        // X11 sample in `poll_events`.
                        tracing::debug!("evdev reported SYN_DROPPED; pointer position may drift");
                    }
                    _ => {}
                }
            }

            if moved {
                send(
                    &sender,
                    InputEvent::MouseMove {
                        x: position.0,
                        y: position.1,
                    },
                    stop,
                );
            }
        }
    }

    for descriptor in set.fds.drain(..) {
        // SAFETY: the descriptor was taken from `File::into_raw_fd` above and is
        // still owned by this set, so it is valid and not double-closed.
        unsafe { libc::close(descriptor.fd) };
    }
}

fn handle_button(button: MouseButton, value: i32, sender: &Sender<InputEvent>, stop: &AtomicBool) {
    let event = if value == 0 {
        InputEvent::ButtonUp(button)
    } else {
        InputEvent::ButtonDown(button)
    };
    send(sender, event, stop);
}

fn send(sender: &Sender<InputEvent>, event: InputEvent, stop: &AtomicBool) {
    match sender.send_timeout(event, SEND_TIMEOUT) {
        Ok(()) => {}
        Err(crossbeam_channel::SendTimeoutError::Timeout(_)) => {
            // The consumer stalled for a second; dropping this event keeps the
            // reader responsive instead of wedging it.
            tracing::debug!("dropped an evdev event because the consumer stalled");
        }
        Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => {
            stop.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerating_devices_never_panics_without_permissions() {
        let devices = enumerate_devices();
        for device in &devices {
            assert!(device.path.starts_with("/dev/input/event"));
            assert!(device.has_keys || device.has_relative);
        }
    }

    #[test]
    fn starting_without_devices_reports_a_permission_error_with_a_fix() {
        if !enumerate_devices().is_empty() {
            return; // a developer machine with input permissions; nothing to assert
        }
        let err = EvdevObserver::start(None).err().expect("no devices");
        match err {
            PlatformError::PermissionDenied { detail } => {
                assert!(detail.contains("input"));
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }
}
