//! `linux/input-event-codes.h` to [`Key`] and back.
//!
//! The letter codes follow the physical QWERTY rows, not the alphabet: `KEY_A` is
//! 30 but `KEY_B` is 48. The three row tables below are the whole mapping, shared
//! by the observer (code to key) and the uinput synthesizer (key to code), so the
//! two directions cannot drift.
//!
//! The OEM punctuation is observed as the character a US layout types there
//! (`KEY_SEMICOLON` is `;`), because that is the name [`Key::virtual_key`] answers
//! `VK_OEM_1` from: a bound `;` then reads the same on both platforms instead of
//! arriving as a raw evdev number that names no key.

use crate::input::Key;

/// `KEY_Q`..`KEY_P` = 16..=25.
pub(crate) const ROW_TOP: [(u16, char); 10] = [
    (16, 'q'),
    (17, 'w'),
    (18, 'e'),
    (19, 'r'),
    (20, 't'),
    (21, 'y'),
    (22, 'u'),
    (23, 'i'),
    (24, 'o'),
    (25, 'p'),
];

/// `KEY_A`..`KEY_L` = 30..=38.
pub(crate) const ROW_HOME: [(u16, char); 9] = [
    (30, 'a'),
    (31, 's'),
    (32, 'd'),
    (33, 'f'),
    (34, 'g'),
    (35, 'h'),
    (36, 'j'),
    (37, 'k'),
    (38, 'l'),
];

/// `KEY_Z`..`KEY_M` = 44..=50.
pub(crate) const ROW_BOTTOM: [(u16, char); 7] = [
    (44, 'z'),
    (45, 'x'),
    (46, 'c'),
    (47, 'v'),
    (48, 'b'),
    (49, 'n'),
    (50, 'm'),
];

/// `KEY_1`..`KEY_9` = 2..=10, `KEY_0` = 11.
pub(crate) const DIGITS: [(u16, char); 10] = [
    (2, '1'),
    (3, '2'),
    (4, '3'),
    (5, '4'),
    (6, '5'),
    (7, '6'),
    (8, '7'),
    (9, '8'),
    (10, '9'),
    (11, '0'),
];

fn letter_from_code(code: u16) -> Option<char> {
    ROW_TOP
        .iter()
        .chain(ROW_HOME.iter())
        .chain(ROW_BOTTOM.iter())
        .chain(DIGITS.iter())
        .find(|(mapped, _)| *mapped == code)
        .map(|(_, character)| *character)
}

/// Character to `KEY_*` code, the inverse of [`letter_from_code`].
pub(crate) fn code_for_char(character: char) -> Option<u16> {
    let character = character.to_ascii_lowercase();
    ROW_TOP
        .iter()
        .chain(ROW_HOME.iter())
        .chain(ROW_BOTTOM.iter())
        .chain(DIGITS.iter())
        .find(|(_, mapped)| *mapped == character)
        .map(|(code, _)| *code)
}

/// Every code this crate names, for the uinput device's key-bit declaration.
pub(crate) fn all_letter_codes() -> impl Iterator<Item = u16> {
    ROW_TOP
        .iter()
        .chain(ROW_HOME.iter())
        .chain(ROW_BOTTOM.iter())
        .chain(DIGITS.iter())
        .map(|(code, _)| *code)
}

/// Non-letter `Key` codes, `linux/input-event-codes.h`.
pub(crate) const KEY_ESC: u16 = 1;
pub(crate) const KEY_BACKSPACE: u16 = 14;
pub(crate) const KEY_TAB: u16 = 15;
pub(crate) const KEY_ENTER: u16 = 28;
pub(crate) const KEY_LEFTCTRL: u16 = 29;
pub(crate) const KEY_LEFTSHIFT: u16 = 42;
pub(crate) const KEY_LEFTALT: u16 = 56;
pub(crate) const KEY_SPACE: u16 = 57;
pub(crate) const KEY_F1: u16 = 59;
pub(crate) const KEY_F10: u16 = 68;
pub(crate) const KEY_F11: u16 = 87;
pub(crate) const KEY_F12: u16 = 88;
pub(crate) const KEY_UP: u16 = 103;
pub(crate) const KEY_LEFT: u16 = 105;
pub(crate) const KEY_RIGHT: u16 = 106;
pub(crate) const KEY_DOWN: u16 = 108;
pub(crate) const KEY_RIGHTCTRL: u16 = 97;
pub(crate) const KEY_RIGHTALT: u16 = 100;
pub(crate) const KEY_RIGHTSHIFT: u16 = 54;

/// The navigation cluster, `KEY_HOME`..`KEY_DELETE`.
pub(crate) const KEY_HOME: u16 = 102;
pub(crate) const KEY_PAGEUP: u16 = 104;
pub(crate) const KEY_END: u16 = 107;
pub(crate) const KEY_PAGEDOWN: u16 = 109;
pub(crate) const KEY_INSERT: u16 = 110;
pub(crate) const KEY_DELETE: u16 = 111;

/// The OEM punctuation, the keys the `VK_OEM_*` codes name. Each is observed as the
/// character a US layout types there, which is the name `Key::virtual_key` answers from.
pub(crate) const KEY_MINUS: u16 = 12;
pub(crate) const KEY_EQUAL: u16 = 13;
pub(crate) const KEY_LEFTBRACE: u16 = 26;
pub(crate) const KEY_RIGHTBRACE: u16 = 27;
pub(crate) const KEY_SEMICOLON: u16 = 39;
pub(crate) const KEY_APOSTROPHE: u16 = 40;
pub(crate) const KEY_GRAVE: u16 = 41;
pub(crate) const KEY_BACKSLASH: u16 = 43;
pub(crate) const KEY_COMMA: u16 = 51;
pub(crate) const KEY_DOT: u16 = 52;
pub(crate) const KEY_SLASH: u16 = 53;
/// The extra key left of `Z` on a 102-key ISO keyboard, which types `\` where it exists
/// and has no key of its own on a US one.
pub(crate) const KEY_102ND: u16 = 86;

/// `linux/input-event-codes.h` to [`Key`], for events read from evdev.
#[must_use]
pub fn key_from_code(code: u16) -> Key {
    if let Some(character) = letter_from_code(code) {
        return Key::Char(character);
    }
    match code {
        KEY_ESC => Key::Escape,
        KEY_BACKSPACE => Key::Backspace,
        KEY_TAB => Key::Tab,
        KEY_ENTER => Key::Enter,
        KEY_LEFTCTRL | KEY_RIGHTCTRL => Key::Control,
        KEY_LEFTSHIFT | KEY_RIGHTSHIFT => Key::Shift,
        KEY_LEFTALT | KEY_RIGHTALT => Key::Alt,
        KEY_SPACE => Key::Space,
        KEY_F1..=KEY_F10 => Key::F((code - KEY_F1 + 1) as u8),
        KEY_F11 => Key::F(11),
        KEY_F12 => Key::F(12),
        KEY_UP => Key::Up,
        KEY_LEFT => Key::Left,
        KEY_RIGHT => Key::Right,
        KEY_DOWN => Key::Down,
        KEY_HOME => Key::Home,
        KEY_END => Key::End,
        KEY_PAGEUP => Key::PageUp,
        KEY_PAGEDOWN => Key::PageDown,
        KEY_INSERT => Key::Insert,
        KEY_DELETE => Key::Delete,
        // The OEM keys, named by the character a US layout types there.
        KEY_MINUS => Key::Char('-'),
        KEY_EQUAL => Key::Char('='),
        KEY_LEFTBRACE => Key::Char('['),
        KEY_RIGHTBRACE => Key::Char(']'),
        KEY_SEMICOLON => Key::Char(';'),
        KEY_APOSTROPHE => Key::Char('\''),
        KEY_GRAVE => Key::Char('`'),
        KEY_BACKSLASH | KEY_102ND => Key::Char('\\'),
        KEY_COMMA => Key::Char(','),
        KEY_DOT => Key::Char('.'),
        KEY_SLASH => Key::Char('/'),
        other => Key::Other(u32::from(other)),
    }
}

/// [`Key`] to `linux/input-event-codes.h`, for the uinput synthesizer.
///
/// The OEM punctuation answers `None`: the letters-and-digits map is `code_for_char`, and
/// the only codes this names are the ones the virtual device declares.
pub fn code_for_key(key: Key) -> Option<u16> {
    let code = match key {
        Key::Char(character) => code_for_char(character)?,
        Key::Escape => KEY_ESC,
        Key::Backspace => KEY_BACKSPACE,
        Key::Tab => KEY_TAB,
        Key::Enter => KEY_ENTER,
        Key::Control => KEY_LEFTCTRL,
        Key::Shift => KEY_LEFTSHIFT,
        Key::Alt => KEY_LEFTALT,
        Key::Space => KEY_SPACE,
        Key::F(number) if (1..=10).contains(&number) => KEY_F1 + u16::from(number) - 1,
        Key::F(11) => KEY_F11,
        Key::F(12) => KEY_F12,
        Key::F(_) => return None,
        Key::Up => KEY_UP,
        Key::Left => KEY_LEFT,
        Key::Right => KEY_RIGHT,
        Key::Down => KEY_DOWN,
        Key::Home => KEY_HOME,
        Key::End => KEY_END,
        Key::PageUp => KEY_PAGEUP,
        Key::PageDown => KEY_PAGEDOWN,
        Key::Insert => KEY_INSERT,
        Key::Delete => KEY_DELETE,
        Key::Other(code) => u16::try_from(code).ok()?,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_follow_the_qwerty_rows_not_the_alphabet() {
        // The trap: `KEY_A` is 30 but `KEY_B` is 48.
        assert_eq!(key_from_code(30), Key::Char('a'));
        assert_eq!(key_from_code(31), Key::Char('s'));
        assert_eq!(key_from_code(38), Key::Char('l'));
        assert_eq!(key_from_code(48), Key::Char('b'));
        assert_eq!(key_from_code(50), Key::Char('m'));
        assert_eq!(key_from_code(16), Key::Char('q'));
        assert_eq!(key_from_code(25), Key::Char('p'));
    }

    #[test]
    fn digits_map_to_their_character() {
        assert_eq!(key_from_code(2), Key::Char('1'));
        assert_eq!(key_from_code(10), Key::Char('9'));
        assert_eq!(key_from_code(11), Key::Char('0'));
        assert_eq!(key_from_code(3), Key::Char('2'));
    }

    #[test]
    fn modifiers_navigation_and_function_keys_map_by_name() {
        assert_eq!(key_from_code(42), Key::Shift);
        assert_eq!(key_from_code(54), Key::Shift);
        assert_eq!(key_from_code(29), Key::Control);
        assert_eq!(key_from_code(56), Key::Alt);
        assert_eq!(key_from_code(28), Key::Enter);
        assert_eq!(key_from_code(57), Key::Space);
        assert_eq!(key_from_code(1), Key::Escape);
        assert_eq!(key_from_code(15), Key::Tab);
        assert_eq!(key_from_code(14), Key::Backspace);
        assert_eq!(key_from_code(103), Key::Up);
        assert_eq!(key_from_code(108), Key::Down);
        assert_eq!(key_from_code(105), Key::Left);
        assert_eq!(key_from_code(106), Key::Right);
        assert_eq!(key_from_code(102), Key::Home);
        assert_eq!(key_from_code(107), Key::End);
        assert_eq!(key_from_code(104), Key::PageUp);
        assert_eq!(key_from_code(109), Key::PageDown);
        assert_eq!(key_from_code(110), Key::Insert);
        assert_eq!(key_from_code(111), Key::Delete);
        assert_eq!(key_from_code(59), Key::F(1));
        assert_eq!(key_from_code(68), Key::F(10));
        assert_eq!(key_from_code(87), Key::F(11));
        assert_eq!(key_from_code(88), Key::F(12));
    }

    #[test]
    fn the_oem_keys_are_named_by_the_character_a_us_layout_types() {
        for (code, character) in [
            (KEY_MINUS, '-'),
            (KEY_EQUAL, '='),
            (KEY_LEFTBRACE, '['),
            (KEY_RIGHTBRACE, ']'),
            (KEY_SEMICOLON, ';'),
            (KEY_APOSTROPHE, '\''),
            (KEY_GRAVE, '`'),
            (KEY_BACKSLASH, '\\'),
            (KEY_COMMA, ','),
            (KEY_DOT, '.'),
            (KEY_SLASH, '/'),
            // The ISO extra key types the character the US layout's `KEY_BACKSLASH` does.
            (KEY_102ND, '\\'),
        ] {
            assert_eq!(key_from_code(code), Key::Char(character), "code {code}");
        }
        // The character is what `Key::virtual_key` answers a virtual-key code from, which is
        // what makes a bound `;` fire here instead of arriving as a raw evdev number.
        assert_eq!(Key::Char(';').virtual_key(), Some(0xBA));
        // Punctuation is still not synthesizable: the uinput device declares no key for it.
        assert_eq!(code_for_key(Key::Char(';')), None);
    }

    #[test]
    fn unknown_codes_survive_as_raw_codes() {
        assert_eq!(key_from_code(0x2ff), Key::Other(0x2ff));
        assert_eq!(key_from_code(183), Key::Other(183));
    }

    #[test]
    fn the_two_directions_agree_for_every_primary_key() {
        // The right-hand modifier variants collapse onto the logical key
        // (`KEY_RIGHTSHIFT` -> `Key::Shift` -> `KEY_LEFTSHIFT`), so they are the
        // documented exception to a clean round trip. The OEM punctuation is the other one,
        // and only in this direction: the observer names it as the character a US layout
        // types, and [`code_for_key`] answers no code for punctuation.
        const RIGHT_HAND: [u16; 3] = [KEY_RIGHTSHIFT, KEY_RIGHTCTRL, KEY_RIGHTALT];
        const OEM: [u16; 12] = [
            KEY_MINUS,
            KEY_EQUAL,
            KEY_LEFTBRACE,
            KEY_RIGHTBRACE,
            KEY_SEMICOLON,
            KEY_APOSTROPHE,
            KEY_GRAVE,
            KEY_BACKSLASH,
            KEY_COMMA,
            KEY_DOT,
            KEY_SLASH,
            KEY_102ND,
        ];
        for code in 1..=255u16 {
            let key = key_from_code(code);
            if matches!(key, Key::Other(_)) || RIGHT_HAND.contains(&code) || OEM.contains(&code) {
                continue;
            }
            assert_eq!(code_for_key(key), Some(code), "round trip for code {code}");
        }
        for code in RIGHT_HAND {
            assert!(code_for_key(key_from_code(code)).is_some());
        }
    }

    #[test]
    fn characters_map_back_to_their_codes() {
        assert_eq!(code_for_key(Key::Char('a')), Some(30));
        assert_eq!(code_for_key(Key::Char('A')), Some(30));
        assert_eq!(code_for_key(Key::Char('b')), Some(48));
        assert_eq!(code_for_key(Key::Char('0')), Some(11));
        assert_eq!(code_for_key(Key::Char('!')), None);
        assert_eq!(code_for_key(Key::F(13)), None);
        assert_eq!(code_for_key(Key::Other(0x1_0000)), None);
    }
}
