//! Nick hider alias validation, ported from `Aoko/Core/NickHiderConfig.cs`.

/// Longest alias the bridge accepts.
pub const MAX_ALIAS_LENGTH: usize = 32;

/// Section-sign; a legacy colour-code prefix is never part of a plain-text alias.
const SECTION_SIGN: char = '\u{00A7}';

/// Returns a safe, plain-text alias or an empty string when it is invalid.
///
/// Trimming happens here because this is the value that reaches the wire.
#[must_use]
pub fn normalize_alias(value: Option<&str>) -> String {
    let alias = value.unwrap_or("").trim();
    if alias.is_empty() || utf16_len(alias) > MAX_ALIAS_LENGTH {
        return String::new();
    }
    if alias.chars().any(is_forbidden) {
        return String::new();
    }
    alias.to_owned()
}

/// Validates raw editor input *without* trimming, so typing whitespace survives
/// until the field is sent.
#[must_use]
pub fn normalize_input(value: Option<&str>) -> String {
    let alias = value.unwrap_or("");
    if utf16_len(alias) > MAX_ALIAS_LENGTH {
        return String::new();
    }
    if alias.chars().any(is_forbidden) {
        return String::new();
    }
    alias.to_owned()
}

/// `char.IsControl` or a section sign.
fn is_forbidden(character: char) -> bool {
    character.is_control() || character == SECTION_SIGN
}

/// The managed `string.Length` is a UTF-16 code-unit count, so the cap is applied
/// on the same unit to keep an astral-plane alias from being accepted here and
/// rejected by the bridge.
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}
