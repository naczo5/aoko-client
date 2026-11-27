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
//!
//! Button transitions are latched per device too, which is what
//! [`EvdevObserver::physical_mouse_state`] answers from: no X11 request can tell a physical
//! press from one XTest or `uinput` synthesised, so the reader is the only observer here that
//! sees the user's own buttons.
//!
//! Two nodes are deliberately not observed. Gamepads, touchpads and tablets are
//! skipped by their absolute axes, and this port's own `uinput` device - the one
//! synthesis falls back to without a display - is skipped by name: it carries keys
//! like a keyboard, so reading it would report every synthesised press back as the
//! user's and toggle the module that pressed it.
//!
//! A device that goes away mid-session - an unplugged keyboard, a destroyed `uinput`
//! device - is dropped from the watched set rather than retried: `poll(2)` keeps
//! reporting it readable with nothing to read, which is a spin rather than a wait.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::error::PlatformError;
use crate::evdev_table::key_from_code;
use crate::input::{InputEvent, MouseButton, MouseState};

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
pub(crate) const BTN_SIDE: u16 = 0x113;
pub(crate) const BTN_EXTRA: u16 = 0x114;

const QUEUE_CAPACITY: usize = 4096;
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

/// The name this port gives its own `/dev/uinput` device.
///
/// `linux/input.rs` creates the device with this name and [`enumerate_devices`] skips a node
/// reporting it, so the creator and the observer's filter cannot drift: a copy of the literal
/// in either place would let the reader observe its own synthesis.
pub(crate) const VIRTUAL_DEVICE_NAME: &str = "aoko virtual input";

/// One bit per `linux/input-event-codes.h` code, `KEY_CNT` (0x300) of them.
const KEY_BITMAP_LEN: usize = 0x300 / 8;

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
/// tablets, whose button noise the automation loop does not want. This port's own
/// virtual device is skipped by name: it is an ordinary `event*` node carrying keys
/// (`EVIOCGNAME`), so observing it would read every synthesised press back as the
/// user's and toggle the very module that pressed it.
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
            if device_name(&file).as_deref() == Some(VIRTUAL_DEVICE_NAME) {
                return None;
            }
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

/// `EVIOCGNAME(len)`: the name the kernel reports for one open device node, which is
/// what `evtest` prints as the device's name.
///
/// `None` when the ioctl fails, which is the case for a node that is not an input
/// device at all.
fn device_name(file: &std::fs::File) -> Option<String> {
    use std::os::unix::io::AsRawFd as _;

    // `UINPUT_MAX_NAME_SIZE` is 80 and the kernel copies at most that many bytes; asking
    // for one more keeps a maximal name from being cut into a different one.
    let mut bytes = [0u8; 81];
    let length = bytes.len() as u32;
    // SAFETY: `bytes` is a live buffer of `length` bytes, and `EVIOCGNAME(len)` writes at
    // most `len` bytes into it, which is the ABI documented in `linux/input.h`.
    let result = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            super::uinput::ioc_read(b'E', 0x06, length) as libc::c_ulong,
            bytes.as_mut_ptr(),
        )
    };
    if result < 0 {
        return None;
    }
    // The name is NUL-terminated in a buffer the kernel zero-pads.
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    Some(String::from_utf8_lossy(&bytes[..end]).into_owned())
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

/// The physical button state the reader maintains, read by
/// [`EvdevObserver::physical_mouse_state`].
///
/// Only devices that are not this process's own `uinput` node are observed, so every
/// transition latched here is the user's: a synthesised press of the clicker's is never one of
/// them, which is the distinction the X11 button mask cannot make.
#[derive(Debug, Default)]
struct ButtonState {
    left: AtomicBool,
    right: AtomicBool,
    /// Whether the reader is still watching a device. Without one nothing maintains the flags
    /// above, and they must not be read as if something did.
    live: AtomicBool,
}

/// Clears the state a reader maintained, however it stops: the last device retired, the
/// consumer disconnected, or the loop unwound. A button flag left set would tell the hold
/// guards the user is holding a button nobody is watching.
struct Watching<'a>(&'a ButtonState);

impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.0.left.store(false, Ordering::Release);
        self.0.right.store(false, Ordering::Release);
        self.0.live.store(false, Ordering::Release);
    }
}

/// A running observer. Dropping it stops the reader thread and joins it.
pub(crate) struct EvdevObserver {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    receiver: Receiver<InputEvent>,
    buttons: Arc<ButtonState>,
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
        // The reader maintains the flags from here on, so the first probe reads them rather
        // than the X11 mask.
        let buttons = Arc::new(ButtonState::default());
        buttons.live.store(true, Ordering::Release);
        let thread_buttons = Arc::clone(&buttons);

        let handle = std::thread::Builder::new()
            .name("aoko-evdev".to_string())
            .spawn(move || read_loop(&paths, seed, sender, &thread_stop, &thread_buttons))
            .map_err(|err| PlatformError::os("spawn evdev reader", err.to_string()))?;

        Ok(Self {
            stop,
            handle: Some(handle),
            receiver,
            buttons,
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

    /// The buttons the user is physically holding, from the transitions this reader saw, or
    /// `None` once it is watching no device and so is maintaining nothing.
    ///
    /// `None` and not a zeroed state: a caller that cannot observe must not read the answer as
    /// "nothing is held", which is the distinction [`InputBackend::probe_mouse_state`] makes.
    ///
    /// [`InputBackend::probe_mouse_state`]: crate::InputBackend::probe_mouse_state
    pub fn physical_mouse_state(&self) -> Option<MouseState> {
        if !self.buttons.live.load(Ordering::Acquire) {
            return None;
        }
        Some(MouseState {
            left_down: self.buttons.left.load(Ordering::Acquire),
            right_down: self.buttons.right.load(Ordering::Acquire),
        })
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
///
/// The four vectors are index-parallel - entry `i` of each describes the same device - so a
/// device is only ever added or removed through [`add_device`] and [`retire_device`], which keep
/// them in step.
struct PollSet {
    fds: Vec<libc::pollfd>,
    paths: Vec<String>,
    buffers: Vec<Vec<u8>>,
    /// The `linux/input-event-codes.h` numbers this device reported down and has not released.
    held: Vec<Vec<u16>>,
}

/// Opens one node and appends it to every vector in `set`.
///
/// A node that cannot be opened is skipped: it went away between the enumeration and here.
fn add_device(set: &mut PollSet, path: &str) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
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
    set.paths.push(path.to_owned());
    set.buffers
        .push(vec![0u8; std::mem::size_of::<RawInputEvent>() * 64]);
    set.held.push(Vec::new());
}

/// Removes one device from every vector in `set`, closing its descriptor, and answers its path
/// so the caller can say why.
///
/// The keys and buttons that device was still holding are released on the way out: nothing will
/// ever read that node again, so their releases are the ones that will never arrive - and a key
/// the consumer latches as held is a bind that stays dead for the rest of the session, while a
/// button latched as held stands the hold guards down for the same rest of it.
fn retire_device(
    set: &mut PollSet,
    index: usize,
    sender: &Sender<InputEvent>,
    stop: &AtomicBool,
) -> String {
    let descriptor = set.fds.remove(index).fd;
    let path = set.paths.remove(index);
    set.buffers.remove(index);
    for code in set.held.remove(index) {
        send(sender, release_event(code), stop);
    }
    // SAFETY: the descriptor came from `File::into_raw_fd` in `add_device`, and removing its
    // entry is what makes this the only close: no index refers to it any more.
    unsafe { libc::close(descriptor) };
    path
}

/// The key bitmap `EVIOCGKEY(len)` reports: one bit per code, set while that key is down.
///
/// `None` when the ioctl fails, which leaves a latch alone rather than guessing at it.
fn keys_down(descriptor: libc::c_int) -> Option<[u8; KEY_BITMAP_LEN]> {
    let mut bits = [0u8; KEY_BITMAP_LEN];
    // SAFETY: `bits` is a live `KEY_BITMAP_LEN`-byte buffer and `EVIOCGKEY(len)` writes at most
    // `len` bytes into it, which is the ABI documented in `linux/input.h`.
    let result = unsafe {
        libc::ioctl(
            descriptor,
            super::uinput::ioc_read(b'E', 0x18, KEY_BITMAP_LEN as u32) as libc::c_ulong,
            bits.as_mut_ptr(),
        )
    };
    if result < 0 {
        return None;
    }
    Some(bits)
}

/// The [`MouseButton`] one `BTN_*` code names, `None` for a key code.
///
/// The two side buttons are `BTN_SIDE` and `BTN_EXTRA`, which is what a desktop sends for the
/// back and forward buttons (X11 numbers them 8 and 9) and what `MouseButton::X1`/`X2` are.
fn button_for_code(code: u16) -> Option<MouseButton> {
    match code {
        BTN_LEFT => Some(MouseButton::Left),
        BTN_RIGHT => Some(MouseButton::Right),
        BTN_MIDDLE => Some(MouseButton::Middle),
        BTN_SIDE => Some(MouseButton::X1),
        BTN_EXTRA => Some(MouseButton::X2),
        _ => None,
    }
}

/// The event that reports `code` going down: a button's own `ButtonDown`, a key's `KeyDown`.
fn press_event(code: u16) -> InputEvent {
    match button_for_code(code) {
        Some(button) => InputEvent::ButtonDown(button),
        None => InputEvent::KeyDown(key_from_code(code)),
    }
}

/// The event that reports `code` coming up, the release half of [`press_event`].
fn release_event(code: u16) -> InputEvent {
    match button_for_code(code) {
        Some(button) => InputEvent::ButtonUp(button),
        None => InputEvent::KeyUp(key_from_code(code)),
    }
}

/// Records one `EV_KEY` transition in a device's latched set, and answers the event to queue.
///
/// A repeat (`value == 2`) is reported as another press, which the consumer drops because the
/// key is already in its held set.
///
/// Buttons are latched by code exactly like keys, so a device that goes away mid-click is
/// retired by the same path: one code names one thing, so a button and a key never share an
/// entry.
fn key_event(code: u16, value: i32, held: &mut Vec<u16>) -> InputEvent {
    let latched = held.iter().position(|latched| *latched == code);
    if value == 0 {
        if let Some(index) = latched {
            held.swap_remove(index);
        }
        release_event(code)
    } else {
        if latched.is_none() {
            held.push(code);
        }
        press_event(code)
    }
}

/// Republishes which physical buttons the watched devices are holding.
///
/// Recomputed from the latches rather than toggled from one event, so a device retired
/// mid-hold cannot leave its button reported down for the rest of the session.
fn publish_buttons(set: &PollSet, buttons: &ButtonState) {
    let held = |code: u16| set.held.iter().any(|latched| latched.contains(&code));
    buttons.left.store(held(BTN_LEFT), Ordering::Release);
    buttons.right.store(held(BTN_RIGHT), Ordering::Release);
}

/// Queues the release of every latched key or button the device's own bitmap now reports up.
///
/// `SYN_DROPPED` means the kernel ring overflowed and events were discarded, so a release may be
/// among them - and a release the consumer never sees is a key it holds for the rest of the
/// session, with that key's bind dead with it. The kernel still answers which keys are down, so
/// the latch is reconciled rather than believed: a key that is genuinely held stays latched, and
/// its own release still arrives. Buttons share the `EV_KEY` bitmap, so a dropped click is
/// reconciled by the same pass.
fn release_dropped(
    descriptor: libc::c_int,
    held: &mut Vec<u16>,
    sender: &Sender<InputEvent>,
    stop: &AtomicBool,
) {
    let Some(down) = keys_down(descriptor) else {
        return;
    };
    let mut index = 0;
    while index < held.len() {
        let code = held[index];
        let still_down = down
            .get(usize::from(code) / 8)
            .is_some_and(|byte| *byte & (1u8 << (code % 8)) != 0);
        if still_down {
            index += 1;
        } else {
            held.swap_remove(index);
            send(sender, release_event(code), stop);
        }
    }
}

fn read_loop(
    paths: &[String],
    seed: Option<(i32, i32)>,
    sender: Sender<InputEvent>,
    stop: &AtomicBool,
    buttons: &ButtonState,
) {
    let _watching = Watching(buttons);
    let mut set = PollSet {
        fds: Vec::with_capacity(paths.len()),
        paths: Vec::with_capacity(paths.len()),
        buffers: Vec::with_capacity(paths.len()),
        held: Vec::with_capacity(paths.len()),
    };

    for path in paths {
        add_device(&mut set, path);
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

        // Reverse order, so a device removed below cannot shift an index this pass has not
        // reached yet.
        for index in (0..set.fds.len()).rev() {
            let revents = set.fds[index].revents;
            if revents & libc::POLLIN == 0 {
                // A node that hung up reports itself ready for ever with nothing to read, so
                // leaving it in the set is a busy loop: an unplugged keyboard, and this process's
                // own `/dev/uinput` device once it is destroyed, both arrive here.
                if revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                    let path = retire_device(&mut set, index, &sender, stop);
                    tracing::debug!("stopped reading {path}: the device went away");
                }
                continue;
            }
            let read = {
                let buffer = &mut set.buffers[index];
                // SAFETY: `buffer` is a live, initialised byte buffer and `read` fills
                // at most its length.
                unsafe {
                    libc::read(
                        set.fds[index].fd,
                        buffer.as_mut_ptr().cast::<libc::c_void>(),
                        buffer.len(),
                    )
                }
            };
            if read < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                // Anything else is a device that keeps reporting itself readable while every
                // read of it fails - `ENODEV` after an unplug - which is a spin at full tilt
                // rather than an error worth retrying.
                let path = retire_device(&mut set, index, &sender, stop);
                tracing::debug!("stopped reading {path}: {err}");
                continue;
            }
            if read == 0 {
                let path = retire_device(&mut set, index, &sender, stop);
                tracing::debug!("stopped reading {path}: end of file");
                continue;
            }

            let byte_count = read as usize;
            let event_size = std::mem::size_of::<RawInputEvent>();
            let mut offset = 0usize;
            let mut moved = false;
            let mut resync = false;

            while offset + event_size <= byte_count {
                let Some(bytes) = set.buffers[index].get(offset..offset + event_size) else {
                    break;
                };
                offset += event_size;
                // SAFETY: `RawInputEvent` is `repr(C)` with only integer fields, so
                // any bit pattern of the right size is a valid value; the slice has
                // exactly that size and alignment is guaranteed by the bounds check.
                let event =
                    unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<RawInputEvent>()) };
                match (event.type_, event.code) {
                    (EV_KEY, code) => {
                        let event = key_event(code, event.value, &mut set.held[index]);
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
                        // The kernel discarded events: the rest of this buffer is stale and the
                        // accumulated position is no longer trustworthy. Reconcile what is held
                        // now; the next read carries the motion.
                        resync = true;
                        break;
                    }
                    _ => {}
                }
            }

            if resync {
                tracing::debug!("evdev reported SYN_DROPPED; re-reading the device's key state");
                release_dropped(set.fds[index].fd, &mut set.held[index], &sender, stop);
            }
            if moved {
                send_move(&sender, position.0, position.1, stop);
            }
        }

        // Once per wake, and after any retirement above: a button whose mouse was unplugged
        // reads up rather than latched for the rest of the session.
        publish_buttons(&set, buttons);
    }

    for descriptor in set.fds.drain(..) {
        // SAFETY: the descriptor was taken from `File::into_raw_fd` in `add_device` and is
        // still owned by this set, so it is valid and not double-closed.
        unsafe { libc::close(descriptor.fd) };
    }
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

/// Queues one pointer sample without waiting for the consumer.
///
/// A move is a sample and not a record: the next one supersedes it, and the consumer wants the
/// latest position rather than every step. Waiting on one would stop this reader draining its
/// device for as long as the consumer is behind, and the kernel's ring - where a *key* event
/// would then be lost - is not something a pointer sample may cost.
fn send_move(sender: &Sender<InputEvent>, x: i32, y: i32, stop: &AtomicBool) {
    if let Err(crossbeam_channel::TrySendError::Disconnected(_)) =
        sender.try_send(InputEvent::MouseMove { x, y })
    {
        stop.store(true, Ordering::Release);
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
    fn our_own_virtual_device_is_never_observed() {
        // The `uinput` device this port creates is an ordinary `/dev/input/event*` node
        // carrying keys, so it is the name `EVIOCGNAME` reports that keeps the observer from
        // reading every synthesised press back as the user's.
        for device in enumerate_devices() {
            let Ok(file) = std::fs::File::open(&device.path) else {
                continue;
            };
            assert_ne!(
                device_name(&file).as_deref(),
                Some(VIRTUAL_DEVICE_NAME),
                "{} is this port's own device",
                device.path
            );
        }
    }

    #[test]
    fn a_repeat_is_a_press_and_a_release_clears_the_latch() {
        let mut held = Vec::new();
        assert_eq!(
            key_event(30, 1, &mut held),
            InputEvent::KeyDown(key_from_code(30))
        );
        assert_eq!(held, vec![30]);
        // `value == 2` is the kernel's auto-repeat: reported as a press, latched once.
        assert_eq!(
            key_event(30, 2, &mut held),
            InputEvent::KeyDown(key_from_code(30))
        );
        assert_eq!(held, vec![30]);
        assert_eq!(
            key_event(30, 0, &mut held),
            InputEvent::KeyUp(key_from_code(30))
        );
        assert!(held.is_empty());
        // A release for a key that was never latched is still the release to report.
        assert_eq!(
            key_event(30, 0, &mut held),
            InputEvent::KeyUp(key_from_code(30))
        );
    }

    #[test]
    fn a_button_latches_like_a_key_and_releases_as_itself() {
        let mut held = Vec::new();
        assert_eq!(
            key_event(BTN_LEFT, 1, &mut held),
            InputEvent::ButtonDown(MouseButton::Left)
        );
        assert_eq!(held, vec![BTN_LEFT]);
        // A second press does not double-latch, the same rule as a key's auto-repeat.
        assert_eq!(
            key_event(BTN_LEFT, 1, &mut held),
            InputEvent::ButtonDown(MouseButton::Left)
        );
        assert_eq!(held, vec![BTN_LEFT]);
        assert_eq!(
            key_event(BTN_LEFT, 0, &mut held),
            InputEvent::ButtonUp(MouseButton::Left)
        );
        assert!(held.is_empty());

        // The side buttons are buttons too, and each keeps its own variant.
        assert_eq!(
            key_event(BTN_SIDE, 1, &mut held),
            InputEvent::ButtonDown(MouseButton::X1)
        );
        assert_eq!(
            key_event(BTN_EXTRA, 1, &mut held),
            InputEvent::ButtonDown(MouseButton::X2)
        );
        assert_eq!(
            key_event(BTN_SIDE, 0, &mut held),
            InputEvent::ButtonUp(MouseButton::X1)
        );
        assert_eq!(held, vec![BTN_EXTRA]);
    }

    #[test]
    fn an_unplugged_mouse_releases_the_button_it_was_holding() {
        // The latch, the published state and the queued release have to agree: a button left
        // down by a device that is gone is a hold guard standing down for ever.
        let stop = AtomicBool::new(false);
        let (sender, receiver) = crossbeam_channel::unbounded();
        let buttons = ButtonState::default();
        buttons.live.store(true, Ordering::Release);

        let mut set = PollSet {
            fds: Vec::new(),
            paths: Vec::new(),
            buffers: Vec::new(),
            held: Vec::new(),
        };
        add_device(&mut set, "/dev/null");
        assert_eq!(set.fds.len(), 1, "/dev/null opens as a plain descriptor");
        set.held[0].push(BTN_LEFT);
        publish_buttons(&set, &buttons);
        assert!(buttons.left.load(Ordering::Acquire));

        assert_eq!(retire_device(&mut set, 0, &sender, &stop), "/dev/null");
        assert!(set.fds.is_empty(), "the device left every vector");
        assert!(set.paths.is_empty() && set.buffers.is_empty() && set.held.is_empty());
        publish_buttons(&set, &buttons);
        assert!(
            !buttons.left.load(Ordering::Acquire),
            "the held button went with the device"
        );
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            vec![InputEvent::ButtonUp(MouseButton::Left)]
        );
    }

    #[test]
    fn the_physical_button_state_is_unknown_without_a_reader() {
        let (_, receiver) = crossbeam_channel::unbounded();
        let buttons = Arc::new(ButtonState::default());
        let observer = EvdevObserver {
            stop: Arc::new(AtomicBool::new(false)),
            handle: None,
            receiver,
            buttons: Arc::clone(&buttons),
        };

        // No reader claimed the flags, so there is nothing to report - and `None` is what tells
        // the backend to fall back rather than read it as "nothing is held".
        assert_eq!(observer.physical_mouse_state(), None);

        buttons.live.store(true, Ordering::Release);
        buttons.right.store(true, Ordering::Release);
        assert_eq!(
            observer.physical_mouse_state(),
            Some(MouseState {
                left_down: false,
                right_down: true,
            })
        );

        // A reader that stops clears what it maintained.
        drop(Watching(&buttons));
        assert_eq!(observer.physical_mouse_state(), None);
    }

    #[test]
    fn a_dropped_syn_never_invents_a_release_it_cannot_see() {
        // `EVIOCGKEY` on a descriptor that is not an input device fails, and the latch is then
        // left exactly as it was: releasing a key the kernel still reports down would free its
        // bind to fire again on the next auto-repeat.
        let stop = AtomicBool::new(false);
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut held = vec![30u16, 31];
        release_dropped(-1, &mut held, &sender, &stop);
        assert_eq!(held, vec![30, 31]);
        assert!(receiver.try_recv().is_err(), "nothing was queued");
    }

    #[test]
    fn a_pointer_sample_never_waits_for_the_consumer() {
        let stop = AtomicBool::new(false);
        let (sender, receiver) = crossbeam_channel::bounded(1);
        sender
            .send(InputEvent::ButtonDown(MouseButton::Left))
            .expect("there is room");

        send_move(&sender, 10, 20, &stop);
        assert!(
            !stop.load(Ordering::Acquire),
            "a full queue is not a disconnect, and the sample is dropped"
        );
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            vec![InputEvent::ButtonDown(MouseButton::Left)]
        );

        drop(receiver);
        send_move(&sender, 1, 2, &stop);
        assert!(
            stop.load(Ordering::Acquire),
            "a reader with no consumer stops"
        );
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
