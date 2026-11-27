//! The keybind button's label, ported from `MainWindow.xaml.cs`.
//!
//! `SetKeybindButtonContent` (`Aoko/MainWindow.xaml.cs:2143-2156`) has three branches:
//! an unsupported module reads its `UnavailableReason`, the module being captured
//! reads `Bind: [Press key or mouse...]`, and otherwise the button reads
//! ``Bind: {FormatVirtualKey(InputHooks.GetModuleKey(module))}``. The XAML button itself
//! has no `Content` (`MainWindow.xaml:359`), so this label is the only text it ever
//! shows. An unbound module formats as `Unbound`, which is why an untouched profile
//! reads `Bind: Unbound` rather than `Unbound`.
//!
//! [`format_virtual_key`] is the explicit half of the managed `FormatVirtualKey`
//! (`:2157-2173`) — every branch the C# spells out in code. The managed method ends with
//! ``((Key)KeyInterop.KeyFromVirtualKey(vk)).ToString()``, i.e. the WPF `Key` enum
//! member's name (`LeftShift`, `NumPad5`, `OemSemicolon`). That name table is **not**
//! ported: it is ~150 members of a Windows-only enum, this crate has no copy of it to
//! check against, and a remembered name would be a wrong label. Two things stand in for it,
//! and between them a stored code always reads back as itself:
//!
//! * `NAMED_KEYS`, `MOUSE_KEYS` and `OEM_KEYS` name every code a capture can store — the
//!   arrows, the navigation cluster, `Space`, `Enter`, `Tab`, `Backspace`, `Escape`, `F1`..`F24`,
//!   the punctuation keys, and the five mouse pseudo-keys (`Mouse Left` .. `Mouse X2`) that a
//!   press stores. The managed fallback printed a `Key` name for the keyboard ones
//!   (`Space`, `Enter`, `PageUp`), so the label matches without the enum.
//! * Anything else prints as `VK 0x..` and [`virtual_key`] parses that back, which is what
//!   makes a code outside the tables — a modifier a managed profile stored, a numpad key, a
//!   media key — both readable and stable instead of silently becoming "no bind".
//!
//! A key is written by `Shell::apply_captured_key`, from what
//! [`crate::components::keybind_capture`] reports: a captured virtual key lands in
//! `Profile::module_keys` for the 38 module buttons, and in `Profile::auto_rod_action_key` /
//! `throwpot_action_key` for the two action binds. Escape arrives as `0` — the managed
//! unbind, `MainWindow.xaml.cs:2042` — and is stored, which is why a cleared binding reads
//! `Bind: Unbound` rather than reverting to its old key. A capture that loses focus reports
//! [`Captured::Abandoned`](crate::components::keybind_capture::Captured::Abandoned) and
//! writes nothing, so the label and the profile behind it cannot disagree.
//!
//! [`virtual_key`] is the inverse: the key name GPUI reports for a pressed key, mapped back
//! to the Windows vk the profile stores — and the same for `MOUSE_KEYS`' labels, which no
//! backend sends as a key but a capture does report. Its key names are the ones the backends
//! produce (`gpui_windows/src/events.rs:1593-1646`, `parse_immutable`, and
//! `gpui_linux/src/linux/platform.rs:1473-1585` on Linux: lower-case letters, digits and
//! symbols, `enter`, `pageup`, `backspace`, `left` and the rest of the named set, and the
//! `keysym_get_name` tail for function keys), not an invented vocabulary.

/// The named keys, `(virtual key, label)`, for the codes that have no printable character.
///
/// One table for both directions — [`format_virtual_key`] prints the label and
/// [`virtual_key`] reads it back — so a key can never be labelled one way and parsed
/// another. The labels are the WPF `Key` member names the managed `FormatVirtualKey`
/// fallback produced (`Key.PageUp`, `Key.Backspace`, ...), and the codes are the `VK_*`
/// values from `windows::Win32::UI::Input::KeyboardAndMouse`.
const NAMED_KEYS: [(i32, &str); 18] = [
    (0x08, "Backspace"),
    (0x09, "Tab"),
    (0x0D, "Enter"),
    (0x1B, "Escape"),
    (0x20, "Space"),
    (0x21, "PageUp"),
    (0x22, "PageDown"),
    (0x23, "End"),
    (0x24, "Home"),
    (0x25, "Left"),
    (0x26, "Up"),
    (0x27, "Right"),
    (0x28, "Down"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    // `VK_APPS`, `VK_BROWSER_BACK` and `VK_BROWSER_FORWARD`: the three named keys GPUI
    // reports (`menu`, `back`, `forward`) that are not in the list above.
    (0x5D, "Apps"),
    (0xA6, "BrowserBack"),
    (0xA7, "BrowserForward"),
];

/// The three names GPUI reports for the keys `NAMED_KEYS` labels `Apps`, `BrowserBack`
/// and `BrowserForward`, `(gpui name, virtual key)`.
///
/// A capture receives GPUI's key name, not the label this file prints. Without these
/// aliases those three keys could be shown as bound and never be bound: [`virtual_key`]
/// answered `None` and the capture reported the press as unbindable.
const KEY_ALIASES: [(&str, i32); 3] = [("menu", 0x5D), ("back", 0xA6), ("forward", 0xA7)];

/// The `VK_OEM_*` keys, `(virtual key, character)`, printed as the character they type.
///
/// The managed fallback printed the WPF name (`Key.OemSemicolon`); the character is the
/// shorter label for the same key and it is the one [`virtual_key`] can read back, which is
/// what keeps `Bind: ;` a binding rather than decoration.
const OEM_KEYS: [(i32, char); 11] = [
    (0xBA, ';'),
    (0xBB, '='),
    (0xBC, ','),
    (0xBD, '-'),
    (0xBE, '.'),
    (0xBF, '/'),
    (0xC0, '`'),
    (0xDB, '['),
    (0xDC, '\\'),
    (0xDD, ']'),
    (0xDE, '\''),
];

/// A keybind button's label for one module's virtual key.
///
/// `vk` is the value stored in `Profile::module_keys`; an absent entry is `0`, which is
/// how the managed `InputHooks.GetModuleKey` reports an unbound module.
#[must_use]
pub fn button_label(vk: i32) -> String {
    format!("Bind: {}", format_virtual_key(vk))
}

/// An action-bind button's label for one action's virtual key.
///
/// `AutoRodActionBindButton` and `ThrowpotActionBindButton` read
/// ``Action: {FormatVirtualKey(...)}`` (`MainWindow.xaml.cs:2135-2140`) from
/// `Profile::auto_rod_action_key` / `Profile::throwpot_action_key`.
#[must_use]
pub fn action_label(vk: i32) -> String {
    format!("Action: {}", format_virtual_key(vk))
}

/// The mouse pseudo-keys, `(virtual key, label)`: the managed `VK_LBUTTON`..`VK_XBUTTON2`
/// (`Aoko/Core/InputHooks.cs:40-44`), which is what a capture reports for a mouse press.
///
/// One table for both directions, like `NAMED_KEYS`: [`format_virtual_key`] prints the label
/// and [`virtual_key`] reads it back, so a stored `0x01` cannot be labelled one way and parsed
/// another. These five are the only entries no keyboard can produce; the press that produces them
/// is [`crate::components::keybind_capture`]'s, and reading them back here is what makes
/// `Mouse Left` a binding rather than decoration.
const MOUSE_KEYS: [(i32, &str); 5] = [
    (0x01, "Mouse Left"),
    (0x02, "Mouse Right"),
    (0x04, "Mouse Middle"),
    (0x05, "Mouse X1"),
    (0x06, "Mouse X2"),
];

/// `FormatVirtualKey` (`MainWindow.xaml.cs:2157-2173`), with the `Key` name table replaced.
///
/// The mouse pseudo-keys (`0x01`, `0x02`, `0x04`-`0x06`) are `MOUSE_KEYS`, the managed naming
/// for the five buttons a global hook reports as keys; the rest are the managed rule for function
/// keys, digits, letters and the backtick key, plus `NAMED_KEYS` and `OEM_KEYS` for the codes
/// a capture can store. A code in none of those prints as `VK 0x..`, which [`virtual_key`]
/// parses, so the label is stable and reversible where the managed enum name is not
/// available.
///
/// Every code a capture can store round-trips, the mouse pseudo-keys included: the button a
/// capture pressed reads back as the code that press stored, which is what lets one table name a
/// binding in both directions.
#[must_use]
pub fn format_virtual_key(vk: i32) -> String {
    if vk <= 0 {
        return String::from("Unbound");
    }
    if let Some((_, label)) = NAMED_KEYS.iter().find(|(code, _)| *code == vk) {
        return String::from(*label);
    }
    if let Some((_, label)) = MOUSE_KEYS.iter().find(|(code, _)| *code == vk) {
        return String::from(*label);
    }
    if let Some((_, character)) = OEM_KEYS.iter().find(|(code, _)| *code == vk) {
        return character.to_string();
    }
    match vk {
        // `VK_F1` is 0x70 and `VK_F24` is 0x87. The managed branch stopped at F12 and let
        // the enum name the rest, which `virtual_key("f13")` can still produce.
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => char::from(u8::try_from(vk).unwrap_or(b'?')).to_string(),
        // No table names this code: print the code itself, in the one form `virtual_key`
        // reads back. A decimal would read as a key name two digits long or as nothing.
        other => format!("VK 0x{other:X}"),
    }
}

/// The virtual key a GPUI key name means, or `None` when the name has no vk.
///
/// This is the inverse of [`format_virtual_key`] and the table
/// [`crate::components::keybind_capture`] reports through. Four properties are deliberate:
///
/// * **The mapping is by physical key, so a shifted symbol shares its unshifted vk.**
///   `:` is `;` on a US layout, so both are `VK_OEM_1` (`0xBA`), which is what the Windows
///   low-level hook reports: it carries a vk code and no shift state the profile could
///   store. The same rule maps `!` to `0x31`, `+` to `0xBB` and `"` to `0xDE`.
/// * **Modifier keys are absent**, because no vk arrives for them: GPUI reports a bare
///   modifier as `ModifiersChanged`, and only synthesises a `shift`/`control`/`alt`/
///   `platform`/`function` keystroke when the modifier is *released*
///   (`gpui/src/window.rs:6657-6676`, `gpui_windows/src/events.rs:1555-1569`). A name with
///   no vk is one a capture refuses and keeps listening past, so a capture that receives
///   nothing but modifiers commits nothing. The managed app could bind `LeftShift`; this
///   cannot, and that is recorded as a gap rather than papered over with a key name the
///   platform never sends.
/// * **The mouse pseudo-keys are labels like any other.** `Mouse Left` .. `Mouse X2` are
///   `MOUSE_KEYS`, reported by a press rather than a keystroke; they parse here for the same
///   reason the keyboard's names do, so a stored code and the label that shows it cannot
///   disagree.
/// * **A code with no name reads back as itself.** `VK 0x..` is parsed here, so a stored
///   code that [`format_virtual_key`] could only print as a number still means one key.
///
/// Keypad keys are absent for the same reason: GPUI on Windows reports a keypad digit as
/// its character (`get_key_from_vkey` over `MapVirtualKeyW`,
/// `gpui_windows/src/keyboard.rs:162-172`), so it would map to the top-row vk (`0x31`)
/// rather than `VK_NUMPAD1` (`0x61`), which would be a wrong vk rather than a missing one.
#[must_use]
pub fn virtual_key(key: &str) -> Option<i32> {
    // The named keys first, from the table that prints them.
    if let Some((code, _)) = NAMED_KEYS
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(key))
    {
        return Some(*code);
    }

    // Then the mouse pseudo-keys, from the table that prints theirs. No platform sends these as
    // a key name — a capture reads the button out of the press — so this branch is only ever
    // reached by a label being read back.
    if let Some((code, _)) = MOUSE_KEYS
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(key))
    {
        return Some(*code);
    }

    // Then the names GPUI reports for the three keys labelled above.
    if let Some((_, code)) = KEY_ALIASES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(key))
    {
        return Some(*code);
    }

    let lower = key.to_ascii_lowercase();

    if lower.len() == 1 {
        let ch = lower.chars().next()?;
        // The OEM characters, from the table that prints them.
        if let Some((code, _)) = OEM_KEYS.iter().find(|(_, character)| *character == ch) {
            return Some(*code);
        }
        return match ch {
            'a'..='z' => Some(0x41 + i32::from(ch as u8 - b'a')),
            '0'..='9' => Some(0x30 + i32::from(ch as u8 - b'0')),
            // The digit row, shifted. `!` is shift+1, `@` shift+2, and so on.
            '!' => Some(0x31),
            '@' => Some(0x32),
            '#' => Some(0x33),
            '$' => Some(0x34),
            '%' => Some(0x35),
            '^' => Some(0x36),
            '&' => Some(0x37),
            '*' => Some(0x38),
            '(' => Some(0x39),
            ')' => Some(0x30),
            // The shifted partner of each `VK_OEM_*` character in `OEM_KEYS`.
            ':' => Some(0xBA),
            '+' => Some(0xBB),
            '<' => Some(0xBC),
            '_' => Some(0xBD),
            '>' => Some(0xBE),
            '?' => Some(0xBF),
            '~' => Some(0xC0),
            '{' => Some(0xDB),
            '|' => Some(0xDC),
            '}' => Some(0xDD),
            '\"' => Some(0xDE),
            _ => None,
        };
    }

    if let Some(number) = lower.strip_prefix('f') {
        if let Ok(number) = number.parse::<i32>() {
            if (1..=24).contains(&number) {
                // `VK_F1` is 0x70; F24 is 0x87.
                return Some(0x6F + number);
            }
        }
    }

    // The label [`format_virtual_key`] prints for a code no table names.
    if let Some(hex) = lower.strip_prefix("vk 0x") {
        if let Ok(code) = i32::from_str_radix(hex, 16) {
            if code > 0 {
                return Some(code);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unbound_module_reads_bind_unbound() {
        assert_eq!(button_label(0), "Bind: Unbound");
        assert_eq!(button_label(-1), "Bind: Unbound");
        assert_eq!(format_virtual_key(0), "Unbound");
    }

    #[test]
    fn the_five_mouse_buttons_keep_their_managed_names() {
        for (vk, expected) in [
            (0x01, "Mouse Left"),
            (0x02, "Mouse Right"),
            (0x04, "Mouse Middle"),
            (0x05, "Mouse X1"),
            (0x06, "Mouse X2"),
        ] {
            assert_eq!(format_virtual_key(vk), expected);
        }
        // 0x03 is `Cancel` in the managed enum and is not a mouse button: it takes the
        // unnamed-code branch, like every other code no table names.
        assert_eq!(format_virtual_key(0x03), "VK 0x3");
    }

    #[test]
    fn function_keys_are_numbered_from_one() {
        assert_eq!(format_virtual_key(0x70), "F1");
        assert_eq!(format_virtual_key(0x7B), "F12");
        // The managed branch stopped at F12 and left the rest to the enum name; F13..F24 are
        // what `virtual_key("f13")` can store, so they are printed the same way.
        assert_eq!(format_virtual_key(0x7C), "F13");
        assert_eq!(format_virtual_key(0x87), "F24");
        assert_eq!(
            button_label(0x76),
            "Bind: F7",
            "the label is the managed `Bind: ` prefix plus the key"
        );
    }

    #[test]
    fn digits_letters_and_backtick_are_themselves() {
        assert_eq!(format_virtual_key(0xC0), "`");
        assert_eq!(format_virtual_key(0x30), "0");
        assert_eq!(format_virtual_key(0x39), "9");
        assert_eq!(format_virtual_key(0x41), "A");
        assert_eq!(format_virtual_key(0x5A), "Z");
    }

    #[test]
    fn an_action_bind_reads_its_own_prefix_and_key() {
        assert_eq!(action_label(0), "Action: Unbound");
        assert_eq!(action_label(0x02), "Action: Mouse Right");
        assert_eq!(action_label(0x70), "Action: F1");
    }

    #[test]
    fn a_code_no_table_names_prints_itself_and_reads_back() {
        // The managed fallback printed the `Key` enum name (`LeftShift`), which this crate
        // has no copy of; the code prints itself instead. Two properties matter and both are
        // asserted: the label is readable, and `virtual_key` turns it back into the code.
        for vk in [0x03, 0x13, 0x5B, 0x60, 0xA0, 0xFF] {
            let label = format_virtual_key(vk);
            assert_eq!(label, format!("VK 0x{vk:X}"), "{vk:#04x}");
            assert_eq!(virtual_key(&label), Some(vk), "{vk:#04x} round trip");
        }
        assert_eq!(format_virtual_key(0xA0), "VK 0xA0", "VK_LSHIFT");
    }

    #[test]
    fn a_captured_key_reaches_the_label_it_formats_to() {
        // The property that matters: a key the widget reports reads back as its label, which
        // is what makes a capture visible on the button that captured it.
        for (name, expected_label) in [
            ("a", "Bind: A"),
            ("z", "Bind: Z"),
            ("5", "Bind: 5"),
            ("f1", "Bind: F1"),
            ("f12", "Bind: F12"),
            ("f13", "Bind: F13"),
            ("`", "Bind: `"),
            ("space", "Bind: Space"),
            ("enter", "Bind: Enter"),
            ("left", "Bind: Left"),
            ("pageup", "Bind: PageUp"),
            (";", "Bind: ;"),
            ("menu", "Bind: Apps"),
        ] {
            let vk = virtual_key(name).unwrap_or_else(|| panic!("{name} has a vk"));
            assert_eq!(button_label(vk), expected_label, "for key {name}");
        }
    }

    #[test]
    fn every_key_a_capture_can_store_reads_back_as_itself() {
        // Every name the platform reports — the printable characters a keyboard produces and
        // the named keys `gpui_windows/src/events.rs:1593-1646` sends — must format and parse
        // as one key, and so must the five mouse pseudo-keys a capture reports from a press. A
        // label the handler cannot read back is a button describing a bind the dispatch never
        // sees.
        let printable = String::from(
            "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./\
             ~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"ZXCVBNM<>?",
        );
        let named = [
            "space",
            "enter",
            "tab",
            "backspace",
            "escape",
            "left",
            "right",
            "up",
            "down",
            "pageup",
            "pagedown",
            "insert",
            "delete",
            "home",
            "end",
            "menu",
            "back",
            "forward",
            "f1",
            "f12",
            "f13",
            "f24",
            "mouse left",
            "mouse right",
            "mouse middle",
            "mouse x1",
            "mouse x2",
        ];
        let singles: Vec<String> = printable
            .chars()
            .map(|character| character.to_string())
            .collect();
        for name in singles.iter().map(String::as_str).chain(named) {
            let vk = virtual_key(name).unwrap_or_else(|| panic!("{name} has a vk"));
            let label = format_virtual_key(vk);
            assert_eq!(
                virtual_key(&label),
                Some(vk),
                "{name} reads back as {label}"
            );
        }
    }

    #[test]
    fn the_cross_platform_keys_are_ones_the_platform_layer_recognises() {
        // The other half of the table lives in `aoko-platform`: `key_for_virtual_key` is how
        // a saved code meets the hook's `Key`, and `Key::virtual_key` is how a hooked key
        // meets a saved code. A code the platform layer does not know is a bind whose label
        // exists and whose key never fires.
        //
        // Both groups beyond the letters and digits are here, because `Key` has a variant for
        // each: the navigation cluster, and the OEM punctuation, which both directions spell
        // as the character a US layout types there. The three labels `NAMED_KEYS` carries that
        // no `Key` variant names (`Apps`, `BrowserBack`, `BrowserForward`) stay out: on Linux
        // nothing names their code, so only the Windows hook's raw `Key::Other(vk)` reaches
        // them.
        for name in [
            "space",
            "enter",
            "tab",
            "backspace",
            "escape",
            "left",
            "right",
            "up",
            "down",
            "home",
            "end",
            "pageup",
            "pagedown",
            "insert",
            "delete",
            ";",
            "=",
            ",",
            "-",
            ".",
            "/",
            "`",
            "[",
            "\\",
            "]",
            "'",
            "a",
            "z",
            "0",
            "9",
            "f1",
            "f12",
            "f24",
        ] {
            let vk = virtual_key(name).unwrap_or_else(|| panic!("{name} has a vk"));
            let key = aoko_platform::key_for_virtual_key(vk)
                .unwrap_or_else(|| panic!("{name} ({vk:#04x}) names no platform Key"));
            assert_eq!(
                key.virtual_key(),
                Some(vk),
                "{name} must come back from the platform layer as the same code"
            );
        }
    }

    #[test]
    fn letters_digits_and_function_keys_are_the_windows_codes() {
        assert_eq!(virtual_key("a"), Some(0x41));
        assert_eq!(virtual_key("z"), Some(0x5A));
        assert_eq!(virtual_key("A"), Some(0x41), "the name is case-insensitive");
        assert_eq!(virtual_key("0"), Some(0x30));
        assert_eq!(virtual_key("9"), Some(0x39));
        assert_eq!(virtual_key("f1"), Some(0x70));
        assert_eq!(virtual_key("f24"), Some(0x87));
    }

    #[test]
    fn a_shifted_symbol_shares_its_physical_keys_vk() {
        // The hook carries a vk and no shift state, so `:` and `;` are one key.
        assert_eq!(virtual_key(";"), Some(0xBA));
        assert_eq!(virtual_key(":"), Some(0xBA));
        assert_eq!(virtual_key("!"), Some(0x31));
        assert_eq!(virtual_key("+"), Some(0xBB));
        assert_eq!(virtual_key("\""), Some(0xDE));
        assert_eq!(virtual_key("~"), Some(0xC0));
    }

    #[test]
    fn the_named_keys_are_the_windows_codes() {
        for (name, vk) in [
            ("space", 0x20),
            ("backspace", 0x08),
            ("tab", 0x09),
            ("enter", 0x0D),
            ("escape", 0x1B),
            ("delete", 0x2E),
            ("insert", 0x2D),
            ("home", 0x24),
            ("end", 0x23),
            ("left", 0x25),
            ("up", 0x26),
            ("right", 0x27),
            ("down", 0x28),
            ("pageup", 0x21),
            ("pagedown", 0x22),
        ] {
            assert_eq!(virtual_key(name), Some(vk), "for key {name}");
        }
    }

    #[test]
    fn a_name_with_no_virtual_key_is_none_rather_than_a_guess() {
        // A bare modifier never arrives as a key press: GPUI turns it into `ModifiersChanged`
        // and only synthesises one of these five names when the modifier is released, so a
        // capture that receives nothing but modifiers has nothing to store. Neither has a
        // name outside the vocabulary.
        for name in [
            "shift", "control", "alt", "platform", "function", "f25", "f0", "", "unbound", "mouse",
            "mouse 4", "vk 0x", "vk 0xZZ",
        ] {
            assert_eq!(virtual_key(name), None, "for key {name}");
        }
    }
}
