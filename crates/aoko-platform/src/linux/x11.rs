//! One X11 connection plus the atoms the EWMH queries need.
//!
//! Everything here is `&self`: `x11rb`'s connection serialises its own request
//! stream, so the window inspector and the input backend can share a connection
//! without an outer lock.

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use crate::error::PlatformError;

/// EWMH atoms, resolved once per connection.
pub(crate) struct Atoms {
    pub client_list: Atom,
    pub active_window: Atom,
    pub wm_pid: Atom,
    pub wm_name: Atom,
    pub wm_class: Atom,
    pub string: Atom,
}

pub(crate) struct X11 {
    pub conn: RustConnection,
    pub root: Window,
    pub atoms: Atoms,
}

impl X11 {
    pub fn connect() -> Result<Self, PlatformError> {
        let (conn, screen_num) = x11rb::connect(None).map_err(|err| {
            PlatformError::unsupported(format!(
                "no X11 display ({err}); set DISPLAY, or run under a session with XWayland"
            ))
        })?;

        let screen = conn.setup().roots.get(screen_num).ok_or_else(|| {
            PlatformError::os("x11::connect", format!("screen {screen_num} missing"))
        })?;
        let root = screen.root;

        let atoms = Atoms {
            client_list: intern(&conn, b"_NET_CLIENT_LIST")?,
            active_window: intern(&conn, b"_NET_ACTIVE_WINDOW")?,
            wm_pid: intern(&conn, b"_NET_WM_PID")?,
            wm_name: intern(&conn, b"_NET_WM_NAME")?,
            wm_class: intern(&conn, b"WM_CLASS")?,
            string: Atom::from(AtomEnum::STRING),
        };

        Ok(Self { conn, root, atoms })
    }

    #[must_use]
    pub fn window_from_handle(handle: crate::window::WindowHandle) -> Option<Window> {
        u32::try_from(handle.0).ok()
    }

    /// Reads a property as a list of 32-bit values.
    pub fn property_u32(&self, window: Window, property: Atom) -> Option<Vec<u32>> {
        let reply = self
            .conn
            .get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX)
            .ok()?
            .reply()
            .ok()?;
        Some(
            reply
                .value32()
                .map(|values| values.collect())
                .unwrap_or_default(),
        )
    }

    /// Reads a property as UTF-8 text, with a `STRING` fallback for clients that
    /// never set `_NET_WM_NAME`.
    pub fn property_text(
        &self,
        window: Window,
        property: Atom,
        fallback: Option<Atom>,
    ) -> Option<String> {
        let reply = self
            .conn
            .get_property(false, window, property, AtomEnum::ANY, 0, u32::MAX)
            .ok()?
            .reply()
            .ok()?;
        let text = decode_text(reply.value, reply.format);
        if text.trim().is_empty() {
            if let Some(fallback) = fallback {
                let reply = self
                    .conn
                    .get_property(false, window, fallback, AtomEnum::ANY, 0, u32::MAX)
                    .ok()?
                    .reply()
                    .ok()?;
                return Some(decode_text(reply.value, reply.format));
            }
        }
        Some(text)
    }

    /// Absolute position plus size of `window`.
    pub fn geometry(&self, window: Window) -> Option<crate::window::Rect> {
        let geometry = self.conn.get_geometry(window).ok()?.reply().ok()?;
        let translated = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some(crate::window::Rect {
            x: i32::from(translated.dst_x),
            y: i32::from(translated.dst_y),
            width: i32::from(geometry.width),
            height: i32::from(geometry.height),
        })
    }

    /// Pointer position relative to the root window and the button mask.
    pub fn pointer(&self) -> Option<(i32, i32, u16)> {
        let reply = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((
            i32::from(reply.root_x),
            i32::from(reply.root_y),
            reply.mask.into(),
        ))
    }

    /// 256-bit keycode bitmap from `XQueryKeymap`.
    pub fn keymap(&self) -> Option<[u8; 32]> {
        let reply = self.conn.query_keymap().ok()?.reply().ok()?;
        Some(reply.keys)
    }

    /// Every keysym currently mapped, paired with its keycode, so a logical
    /// [`crate::input::Key`] can be resolved against the user's real layout.
    pub fn keysym_table(&self) -> Vec<(u32, u8)> {
        let setup = self.conn.setup();
        let min_keycode = setup.min_keycode;
        // The request takes a u8 count; a setup never spans more than 256 codes.
        let count = setup
            .max_keycode
            .saturating_sub(min_keycode)
            .saturating_add(1);
        let Ok(cookie) = self.conn.get_keyboard_mapping(min_keycode, count) else {
            return Vec::new();
        };
        let Ok(reply) = cookie.reply() else {
            return Vec::new();
        };
        let per_keycode = usize::from(reply.keysyms_per_keycode);
        if per_keycode == 0 {
            return Vec::new();
        }

        let mut table = Vec::with_capacity(reply.keysyms.len());
        for (index, keysym) in reply.keysyms.iter().enumerate() {
            if *keysym == 0 {
                continue;
            }
            let keycode = u16::from(min_keycode) + (index / per_keycode) as u16;
            let Ok(keycode) = u8::try_from(keycode) else {
                continue;
            };
            table.push((*keysym, keycode));
        }
        table
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom, PlatformError> {
    conn.intern_atom(true, name)
        .map_err(|err| PlatformError::os("XInternAtom", err.to_string()))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|err| PlatformError::os("XInternAtom", err.to_string()))
}

/// X11 property text is UTF-8 for `_NET_WM_NAME` and Latin-1 for legacy
/// `WM_NAME` written as `STRING`. Decoding lossily keeps the picker usable
/// instead of dropping a window with an odd name.
fn decode_text(value: Vec<u8>, format: u8) -> String {
    if format == 8 {
        String::from_utf8_lossy(&value).into_owned()
    } else {
        // 16/32-bit properties are not names; report them as the raw scalars
        // rather than guessing at an encoding.
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_text_accepts_utf8_names() {
        let decoded = decode_text("Minecraft 1.8.9".as_bytes().to_vec(), 8);
        assert_eq!(decoded, "Minecraft 1.8.9");
    }

    #[test]
    fn decode_text_is_lossy_rather_than_panicking() {
        let decoded = decode_text(vec![0x4d, 0xff, 0xff], 8);
        assert!(decoded.starts_with('M'));
    }

    #[test]
    fn decode_text_ignores_non_byte_formats() {
        assert!(decode_text(vec![1, 0, 0, 0], 32).is_empty());
    }

    #[test]
    fn handle_conversion_rejects_64_bit_only_ids() {
        assert_eq!(
            X11::window_from_handle(crate::window::WindowHandle(0x1234)),
            Some(0x1234)
        );
        assert_eq!(
            X11::window_from_handle(crate::window::WindowHandle(u64::from(u32::MAX) + 1)),
            None
        );
    }
}
