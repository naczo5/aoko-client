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
//! check against, and a remembered name would be a wrong label. A vk outside the explicit
//! branches formats as its decimal value here, which is what .NET prints for an unnamed
//! enum member, and the difference for *named* members is recorded as a gap rather than
//! guessed. A key is written by `Shell::apply_captured_key`, from what
//! [`crate::components::keybind_capture`] reports: a captured virtual key lands in
//! `Profile::module_keys` for the 38 module buttons, and in `Profile::auto_rod_action_key` /
//! `throwpot_action_key` for the two action binds. Escape arrives as `0` — the managed
//! unbind, `MainWindow.xaml.cs:2042` — and is stored, which is why a cleared binding reads
//! `Bind: Unbound` rather than reverting to its old key. A capture that loses focus reports
//! [`Captured::Abandoned`](crate::components::keybind_capture::Captured::Abandoned) and
//! writes nothing, so the label and the profile behind it cannot disagree.
//!
//! [`virtual_key`] is the inverse: the key name GPUI reports for a pressed key, mapped back
//! to the Windows vk the profile stores. Its names are the ones the Linux backends produce
//! (`gpui_linux/src/linux/platform.rs:1473-1585`: lower-case letters, digits and symbols,
//! `enter`, `pageup`, `backspace`, `left` and the rest of the named set, and the
//! `keysym_get_name` tail for function keys), not an invented vocabulary.

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

/// `FormatVirtualKey` (`MainWindow.xaml.cs:2157-2173`), explicit branches only.
///
/// The mouse pseudo-keys (`0x01`-`0x06`) are the managed naming for the five buttons a
/// global hook reports as keys; the rest are the managed rule for function keys, digits,
/// letters and the backtick key.
#[must_use]
pub fn format_virtual_key(vk: i32) -> String {
    match vk {
        i32::MIN..=0 => String::from("Unbound"),
        0x01 => String::from("Mouse Left"),
        0x02 => String::from("Mouse Right"),
        0x04 => String::from("Mouse Middle"),
        0x05 => String::from("Mouse X1"),
        0x06 => String::from("Mouse X2"),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        0xC0 => String::from("`"),
        0x30..=0x39 | 0x41..=0x5A => char::from(u8::try_from(vk).unwrap_or(b'?')).to_string(),
        // The managed fallback is the `Key` enum name; see the module docs for why that
        // table is not here and what this prints instead.
        other => other.to_string(),
    }
}

/// The virtual key a GPUI key name means, or `None` when the name has no vk.
///
/// This is the inverse of [`format_virtual_key`] and the table
/// [`crate::components::keybind_capture`] reports through. Two properties are deliberate
/// and both are managed behaviour rather than invention:
///
/// * **The mapping is by physical key, so a shifted symbol shares its unshifted vk.**
///   `:` is `;` on a US layout, so both are `VK_OEM_1` (`0xBA`), which is what the Windows
///   low-level hook reports: it carries a vk code and no shift state the profile could
///   store. The same rule maps `!` to `0x31`, `+` to `0xBB` and `"` to `0xDE`.
/// * **Modifier keys are absent**, because no vk arrives for them: the Linux backends drop
///   a modifier press before it becomes a `Keystroke`
///   (`gpui_linux/src/linux/x11/client.rs:1163-1165`). The managed app could bind
///   `LeftShift`; this cannot, and that is recorded as a gap rather than papered over with
///   a key name the platform never sends.
///
/// Keypad keys are absent for the same reason: Linux reports a keypad digit as its digit
/// key, so it would map to the top-row vk (`0x31`) rather than `VK_NUMPAD1` (`0x61`), which
/// would be a wrong vk rather than a missing one.
#[must_use]
pub fn virtual_key(key: &str) -> Option<i32> {
    let lower = key.to_ascii_lowercase();

    if lower.len() == 1 {
        let ch = lower.chars().next()?;
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
            // `VK_OEM_1` through `VK_OEM_7` plus `VK_OEM_102`, each pair on one key.
            ';' | ':' => Some(0xBA),
            '=' | '+' => Some(0xBB),
            ',' | '<' => Some(0xBC),
            '-' | '_' => Some(0xBD),
            '.' | '>' => Some(0xBE),
            '/' | '?' => Some(0xBF),
            '`' | '~' => Some(0xC0),
            '[' | '{' => Some(0xDB),
            '\\' | '|' => Some(0xDC),
            ']' | '}' => Some(0xDD),
            '\'' | '"' => Some(0xDE),
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

    match lower.as_str() {
        "space" => Some(0x20),
        "backspace" => Some(0x08),
        "tab" => Some(0x09),
        "enter" => Some(0x0D),
        "escape" => Some(0x1B),
        "delete" => Some(0x2E),
        "insert" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "pageup" => Some(0x21),
        "pagedown" => Some(0x22),
        _ => None,
    }
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
        // 0x03 is `Cancel` in the managed enum and is not a mouse button.
        assert_eq!(format_virtual_key(0x03), "3");
    }

    #[test]
    fn function_keys_are_numbered_from_one() {
        assert_eq!(format_virtual_key(0x70), "F1");
        assert_eq!(format_virtual_key(0x7B), "F12");
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
    fn a_virtual_key_outside_the_explicit_branches_is_its_decimal_value() {
        // The managed fallback would print the `Key` enum name here (e.g. `LeftShift`);
        // the gap is documented in the module header rather than guessed at.
        assert_eq!(format_virtual_key(0xA0), "160");
    }

    #[test]
    fn a_captured_key_reaches_the_label_it_formats_to() {
        // The property that matters: a key the widget reports reads back as its label,
        // which is what makes a capture visible on the button that captured it. `space`
        // prints as `32` because the WPF `Key` name table is the documented gap, not
        // because the vk is wrong — 0x20 is `VK_SPACE`.
        for (name, expected_label) in [
            ("a", "Bind: A"),
            ("z", "Bind: Z"),
            ("5", "Bind: 5"),
            ("f1", "Bind: F1"),
            ("f12", "Bind: F12"),
            ("`", "Bind: `"),
            ("space", "Bind: 32"),
        ] {
            let vk = virtual_key(name).unwrap_or_else(|| panic!("{name} has a vk"));
            assert_eq!(button_label(vk), expected_label, "for key {name}");
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
        // Modifiers never arrive as a keystroke on Linux, so they get no vk; neither does
        // a name outside the vocabulary.
        for name in [
            "shift", "control", "alt", "platform", "function", "f25", "f0", "",
        ] {
            assert_eq!(virtual_key(name), None, "for key {name}");
        }
    }
}
