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
//! guessed. It is unreachable in Phase 2 anyway: no surface captures a key yet, so every
//! module key in every profile is absent and every button reads `Bind: Unbound`.

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
}
