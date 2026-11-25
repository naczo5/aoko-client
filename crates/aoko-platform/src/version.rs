//! Minecraft version detection from launcher command lines.
//!
//! Ported one-for-one from `Aoko/Core/ProcessCommandLine.cs` (`TryParseVersion`)
//! and `Aoko/Core/GameStateClient.cs` (`NormalizeDetectedVersion`,
//! `ResolveBridgeDllName`). The C# used compiled `Regex` instances; the rules
//! are re-expressed as hand-written scanners so this crate needs no regex
//! engine and allocates nothing on the hot path.
//!
//! The five probes run in the same order as the C# and the first hit wins,
//! which is what makes `--version` beat a jar path on a conflicting line.

/// A version this host knows how to name. Returned by reference so detection
/// never allocates.
pub type KnownVersion = &'static str;

/// `GameStateClient.NormalizeDetectedVersion`.
///
/// Maps arbitrary text onto one of five canonical labels, or `None`.
/// Token boundaries matter: `2.26.0` (a log4j version) must not match `26.2`.
#[must_use]
pub fn normalize_detected_version(raw: &str) -> Option<KnownVersion> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // `ToLowerInvariant` only affects the ASCII comparisons below; the needles
    // are all ASCII, so an ASCII-lowercase copy is enough and stays `Cow`-free
    // by comparing case-insensitively in place.
    let value = trimmed;

    if contains_version_token(value, "26.3") {
        return Some("26.3");
    }
    if contains_version_token(value, "26.2") {
        return Some("26.2");
    }
    if contains_version_token(value, "26.1")
        || value.eq_ignore_ascii_case("26")
        || value.eq_ignore_ascii_case("minecraft 26")
        || starts_with_ignore_ascii_case(value, "26.")
    {
        return Some("26.1");
    }
    if contains_version_token(value, "1.21") || value.eq_ignore_ascii_case("1.21") {
        return Some("1.21");
    }
    if contains_version_token(value, "1.8.9")
        || value.eq_ignore_ascii_case("1.8")
        || starts_with_ignore_ascii_case(value, "1.8.")
    {
        return Some("1.8.9");
    }

    None
}

/// `GameStateClient.ResolveBridgeDllName`: 26.x and 1.21.x share the modern
/// bridge, everything else gets the legacy one.
#[must_use]
pub fn resolve_bridge_dll_name(resolved_version: &str) -> &'static str {
    let version = resolved_version.trim();
    if version.is_empty() {
        return "bridge.dll";
    }
    if starts_with_ignore_ascii_case(version, "26.")
        || starts_with_ignore_ascii_case(version, "1.21")
    {
        "bridge_261.dll"
    } else {
        "bridge.dll"
    }
}

/// `ProcessCommandLine.TryParseVersion`.
///
/// Reads a JVM command line for a Minecraft version marker produced by any
/// launcher: an explicit `--version` argument, a client jar path, a Fabric
/// intermediary jar, the official `versions/<v>/` layout, or a Lunar natives
/// marker (`v26_2`).
#[must_use]
pub fn try_parse_version(command_line: &str) -> Option<KnownVersion> {
    if command_line.trim().is_empty() {
        return None;
    }

    if let Some(from_arg) = version_argument(command_line) {
        return normalize_detected_version(from_arg);
    }
    if let Some(from_client_jar) = client_jar_version(command_line) {
        return normalize_detected_version(from_client_jar);
    }
    if let Some(from_intermediary) = intermediary_version(command_line) {
        return normalize_detected_version(from_intermediary);
    }
    if let Some(from_versions_dir) = versions_dir_version(command_line) {
        return normalize_detected_version(from_versions_dir);
    }
    if let Some(marker) = natives_marker(command_line) {
        return normalize_natives_marker(marker);
    }

    None
}

/// `--version(?:=|\s+)([A-Za-z0-9_.-]+)`, case-insensitive.
fn version_argument(command_line: &str) -> Option<&str> {
    let bytes = command_line.as_bytes();
    let needle = b"--version";
    let mut search = 0usize;

    while let Some(offset) = find_ascii_ignore_case(&bytes[search..], needle) {
        let after = search + offset + needle.len();
        let mut cursor = after;

        if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
        } else {
            let whitespace_start = cursor;
            while let Some(byte) = bytes.get(cursor) {
                if byte.is_ascii_whitespace() {
                    cursor += 1;
                } else {
                    break;
                }
            }
            if cursor == whitespace_start {
                search = after;
                continue;
            }
        }

        let value_start = cursor;
        while let Some(byte) = bytes.get(cursor) {
            if is_word_byte(*byte) || *byte == b'.' || *byte == b'-' {
                cursor += 1;
            } else {
                break;
            }
        }

        if cursor > value_start {
            return command_line.get(value_start..cursor);
        }
        search = after;
    }

    None
}

/// `minecraft-(\d+(?:\.\d+)+)-client\.jar`, case-insensitive.
fn client_jar_version(command_line: &str) -> Option<&str> {
    let bytes = command_line.as_bytes();
    let needle = b"minecraft-";
    let mut search = 0usize;

    while let Some(offset) = find_ascii_ignore_case(&bytes[search..], needle) {
        let after = search + offset + needle.len();
        if let Some(version) = dotted_digits(&bytes[after..]) {
            let tail = after + version.len();
            if starts_with_ascii_ignore_case(&bytes[tail..], b"-client.jar") {
                return command_line.get(after..tail);
            }
        }
        search = after;
    }

    None
}

/// `intermediary[/-](\d+(?:\.\d+)+)`, case-insensitive.
fn intermediary_version(command_line: &str) -> Option<&str> {
    let bytes = command_line.as_bytes();
    let needle = b"intermediary";
    let mut search = 0usize;

    while let Some(offset) = find_ascii_ignore_case(&bytes[search..], needle) {
        let after = search + offset + needle.len();
        match bytes.get(after) {
            Some(&separator) if separator == b'/' || separator == b'-' => {
                if let Some(version) = dotted_digits(&bytes[after + 1..]) {
                    return command_line.get(after + 1..after + 1 + version.len());
                }
            }
            _ => {}
        }
        search = after;
    }

    None
}

/// `[\\/]versions[\\/](\d+(?:\.\d+)+)[\\/]`, case-insensitive.
fn versions_dir_version(command_line: &str) -> Option<&str> {
    let bytes = command_line.as_bytes();
    let needle = b"versions";
    let mut search = 0usize;

    while let Some(offset) = find_ascii_ignore_case(&bytes[search..], needle) {
        if offset == 0 {
            search = search + offset + needle.len();
            continue;
        }
        let after = search + offset + needle.len();
        let before = search + offset - 1;
        let separator_before = bytes.get(before).is_some_and(|b| *b == b'/' || *b == b'\\');
        let separator_after = bytes.get(after).is_some_and(|b| *b == b'/' || *b == b'\\');

        if separator_before && separator_after {
            if let Some(version) = dotted_digits(&bytes[after + 1..]) {
                let end = after + 1 + version.len();
                let terminated = bytes.get(end).is_some_and(|b| *b == b'/' || *b == b'\\');
                if terminated {
                    return command_line.get(after + 1..end);
                }
            }
        }
        search = after;
    }

    None
}

/// `[vV](26_\d+(?:_\d+)?|1_21(?:_\d+)?|1_8(?:_\d+)?)\b`.
///
/// Lunar ships natives as `v26_2`, `v1_21_11`, `v1_8`. Alternation order is
/// preserved: `1_21` is tried before `1_8` so `v1_21_11` resolves to `1.21.11`
/// rather than stopping at `1_8`... which it cannot, but the order also decides
/// `v1_21` versus a hypothetical `v1_2`.
fn natives_marker(command_line: &str) -> Option<&str> {
    let bytes = command_line.as_bytes();
    // `26_` requires at least one digit after it (`26_\d+`); `1_21` and `1_8`
    // take an optional `_\d+` suffix instead.
    let alternatives: [(&[u8], bool); 3] = [(b"26_", true), (b"1_21", false), (b"1_8", false)];
    let mut index = 0usize;

    while let Some(byte) = bytes.get(index) {
        if *byte == b'v' || *byte == b'V' {
            for (prefix, needs_digits) in alternatives {
                if let Some(length) =
                    match_natives_alternative(&bytes[index + 1..], prefix, needs_digits)
                {
                    let end = index + 1 + length;
                    let boundary = bytes.get(end).is_none_or(|b| !is_word_byte(*b));
                    if boundary {
                        return command_line.get(index + 1..end);
                    }
                }
            }
        }
        index += 1;
    }

    None
}

fn match_natives_alternative(rest: &[u8], prefix: &[u8], needs_digits: bool) -> Option<usize> {
    if !starts_with_ascii(rest, prefix) {
        return None;
    }
    let mut cursor = prefix.len();
    let first_digits = count_digits(&rest[cursor..]);
    if needs_digits && first_digits == 0 {
        return None;
    }
    cursor += first_digits;
    if rest.get(cursor) == Some(&b'_') {
        let extra = count_digits(&rest[cursor + 1..]);
        if extra > 0 {
            cursor += 1 + extra;
        }
    }
    Some(cursor)
}

/// `raw.Replace('_', '.')` followed by `NormalizeDetectedVersion`, without the
/// intermediate allocation for the common markers.
fn normalize_natives_marker(marker: &str) -> Option<KnownVersion> {
    let mut buffer = [0u8; 16];
    let bytes = marker.as_bytes();
    if bytes.len() > buffer.len() {
        return None;
    }
    for (slot, byte) in buffer.iter_mut().zip(bytes) {
        *slot = if *byte == b'_' { b'.' } else { *byte };
    }
    let normalized = std::str::from_utf8(&buffer[..bytes.len()]).ok()?;
    normalize_detected_version(normalized)
}

/// `(?<![\d.])needle(?![\d])`, case-insensitive over ASCII.
fn contains_version_token(value: &str, needle: &str) -> bool {
    let bytes = value.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut search = 0usize;

    while let Some(offset) = find_ascii_ignore_case(&bytes[search..], needle_bytes) {
        let start = search + offset;
        let end = start + needle_bytes.len();
        let before_ok = start == 0
            || bytes
                .get(start - 1)
                .is_none_or(|b| !b.is_ascii_digit() && *b != b'.');
        let after_ok = bytes.get(end).is_none_or(|b| !b.is_ascii_digit());
        if before_ok && after_ok {
            return true;
        }
        search = start + 1;
        while search < bytes.len() && !value.is_char_boundary(search) {
            search += 1;
        }
        if search >= bytes.len() {
            break;
        }
    }

    false
}

/// `\d+(?:\.\d+)+` at the head of `bytes`.
fn dotted_digits(bytes: &[u8]) -> Option<&str> {
    let mut cursor = count_digits(bytes);
    if cursor == 0 {
        return None;
    }
    let mut groups = 0usize;
    loop {
        if bytes.get(cursor) != Some(&b'.') {
            break;
        }
        let digits = count_digits(&bytes[cursor + 1..]);
        if digits == 0 {
            break;
        }
        cursor += 1 + digits;
        groups += 1;
    }
    if groups == 0 {
        return None;
    }
    std::str::from_utf8(&bytes[..cursor]).ok()
}

fn count_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// ASCII-only approximation of .NET's `\w`, which for these needles is
/// `[A-Za-z0-9_]`.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn starts_with_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && &haystack[..needle.len()] == needle
}

fn starts_with_ascii_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value.len() >= prefix.len() && value[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// ASCII case-insensitive `find`, returning a byte offset that respects `
/// haystack` being UTF-8 because the needle is ASCII.
fn find_ascii_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let limit = haystack.len() - needle.len();
    for start in 0..=limit {
        if haystack[start..start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(start);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_detected_version_matches_the_managed_table() {
        // Cases lifted verbatim from `GameStateClientSeamTests` plus the
        // whitespace and mixed-case handling the C# gets from `Trim`/`ToLower`.
        let cases = [
            ("26.1", Some("26.1")),
            ("26.2", Some("26.2")),
            ("26.3", Some("26.3")),
            ("26.3.1", Some("26.3")),
            ("26.1.2", Some("26.1")),
            ("26", Some("26.1")),
            ("minecraft 26", Some("26.1")),
            ("1.21.4", Some("1.21")),
            ("1.8.9", Some("1.8.9")),
            ("1.8.8", Some("1.8.9")),
            ("2.26.0", None),
            ("unknown", None),
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_detected_version(input),
                expected,
                "normalize_detected_version({input:?})"
            );
        }
    }

    #[test]
    fn normalize_detected_version_ignores_surrounding_whitespace_and_case() {
        assert_eq!(
            normalize_detected_version("  LUNAR CLIENT 26.2 "),
            Some("26.2")
        );
        assert_eq!(normalize_detected_version("   "), None);
    }

    #[test]
    fn normalize_detected_version_does_not_match_inside_a_longer_number() {
        // `126.30` contains `26.3` as a substring but not as a token.
        assert_eq!(normalize_detected_version("126.30"), None);
        // Parity quirk preserved from the C#: `26.30` fails the `26.3` token test
        // (trailing digit) and then falls through `StartsWith("26.")` to `26.1`.
        assert_eq!(normalize_detected_version("26.30"), Some("26.1"));
    }

    #[test]
    fn resolve_bridge_dll_name_maps_modern_versions_to_261() {
        let cases = [
            ("26.3", "bridge_261.dll"),
            ("26.2", "bridge_261.dll"),
            ("26.1", "bridge_261.dll"),
            ("1.21", "bridge_261.dll"),
            ("1.8.9", "bridge.dll"),
            ("auto", "bridge.dll"),
            ("", "bridge.dll"),
            ("   ", "bridge.dll"),
        ];
        for (version, expected) in cases {
            assert_eq!(
                resolve_bridge_dll_name(version),
                expected,
                "resolve_bridge_dll_name({version:?})"
            );
        }
    }

    #[test]
    fn try_parse_version_reads_launcher_command_lines() {
        // Exactly `GameStateClientSeamTests.TryParseVersion_ReadsLauncherCommandLines`.
        let cases: [(&str, Option<&str>); 6] = [
            (
                r"C:\Prism\javaw.exe -cp C:/libs/net/fabricmc/intermediary/26.2/intermediary-26.2.jar;C:/libs/com/mojang/minecraft/26.2/minecraft-26.2-client.jar org.prismlauncher.EntryPoint",
                Some("26.2"),
            ),
            (
                r"C:\libs\log4j-core-2.26.0.jar;C:\libs\minecraft-1.8.9-client.jar net.minecraft.client.main.Main",
                Some("1.8.9"),
            ),
            (
                r"javaw.exe com.moonsworth.lunar.genesis.Genesis --version 26.2 --launcherVersion 3.0",
                Some("26.2"),
            ),
            (
                r"javaw.exe com.moonsworth.lunar.genesis.Genesis --version 26.3 --launcherVersion 3.0",
                Some("26.3"),
            ),
            (
                r"javaw.exe -cp C:/versions/1.21.4/client.jar net.minecraft.client.main.Main --version 1.21.4",
                Some("1.21"),
            ),
            (
                r"javaw.exe -cp C:/libs/log4j-api-2.26.0.jar;C:/libs/log4j-core-2.26.0.jar org.example.Main",
                None,
            ),
        ];
        for (command_line, expected) in cases {
            assert_eq!(
                try_parse_version(command_line),
                expected,
                "try_parse_version({command_line:?})"
            );
        }
    }

    #[test]
    fn try_parse_version_reads_the_equals_form_of_the_version_argument() {
        assert_eq!(
            try_parse_version("java --version=1.8.9 net.minecraft.Main"),
            Some("1.8.9")
        );
        assert_eq!(
            try_parse_version("java --version  26.2 net.minecraft.Main"),
            Some("26.2")
        );
    }

    #[test]
    fn try_parse_version_reads_official_versions_directory() {
        assert_eq!(
            try_parse_version(
                r"javaw.exe -cp C:\Users\me\AppData\Roaming\.minecraft\versions\1.8.9\1.8.9.jar net.minecraft.Main"
            ),
            Some("1.8.9")
        );
    }

    #[test]
    fn try_parse_version_reads_lunar_natives_markers() {
        assert_eq!(
            try_parse_version(
                r"javaw.exe -cp C:\Users\me\.lunarclient\offline\multiver\natives\v26_2\..."
            ),
            Some("26.2")
        );
        assert_eq!(
            try_parse_version(
                r"javaw.exe -Djava.library.path=C:\x\.lunarclient\natives\v1_8\natives"
            ),
            Some("1.8.9")
        );
        assert_eq!(
            try_parse_version(r"javaw.exe -Djava.library.path=C:\x\natives\v1_21_11\natives"),
            Some("1.21")
        );
    }

    #[test]
    fn try_parse_version_returns_none_for_empty_input() {
        assert_eq!(try_parse_version(""), None);
        assert_eq!(try_parse_version("   "), None);
    }

    #[test]
    fn version_argument_wins_over_a_jar_path() {
        // `--version` is probed first, so a conflicting jar path loses.
        assert_eq!(
            try_parse_version("java -cp minecraft-1.8.9-client.jar --version 26.2"),
            Some("26.2")
        );
    }
}
