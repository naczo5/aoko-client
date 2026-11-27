//! Account data types, offline-mode UUID derivation, and the credential-file
//! parsers, ported from `Aoko/Core/Accounts/`.
//!
//! Everything here is pure: no HTTP, no file system, no dialogs. The store, the
//! SISU auth flow, and the Lunar mirror live in `aoko-core`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Values stored in [`StoredAccount::source`].
pub mod sources {
    pub const UNKNOWN: &str = "unknown";
    pub const LOCALTS: &str = "localts";
    pub const COOKIE: &str = "cookie";
    pub const TOKEN: &str = "token";
    pub const OFFLINE: &str = "offline";
}

/// Player-facing wording for a source, so import results and chips agree.
#[must_use]
pub fn source_display(source: Option<&str>) -> String {
    match source.map(str::to_lowercase).as_deref() {
        Some(sources::LOCALTS) => "Localts token".to_owned(),
        Some(sources::COOKIE) => "Microsoft cookies".to_owned(),
        Some(sources::TOKEN) => "access token".to_owned(),
        Some(sources::OFFLINE) => "cracked name".to_owned(),
        _ => match source {
            Some(value) if !value.trim().is_empty() => value.to_owned(),
            _ => "unknown source".to_owned(),
        },
    }
}

/// An authenticated Minecraft profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftProfile {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub refresh_token: String,
    pub source: String,
}

impl MinecraftProfile {
    /// Builds a profile, deriving `source` from the presence of a refresh token.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        uuid: impl Into<String>,
        access_token: impl Into<String>,
        refresh_token: Option<String>,
        source: Option<String>,
    ) -> Self {
        let refresh_token = refresh_token.unwrap_or_default();
        let has_refresh = !refresh_token.trim().is_empty();
        let source = match source {
            Some(value) if !value.trim().is_empty() => value,
            _ => {
                if has_refresh {
                    sources::LOCALTS.to_owned()
                } else {
                    sources::TOKEN.to_owned()
                }
            }
        };

        Self {
            name: name.into(),
            uuid: uuid.into(),
            access_token: access_token.into(),
            refresh_token,
            source,
        }
    }

    #[must_use]
    pub fn has_refresh(&self) -> bool {
        !self.refresh_token.trim().is_empty()
    }
}

/// A saved login. `id` is a local key that never leaves the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccount {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(rename = "accessToken", default)]
    pub access_token: String,
    #[serde(rename = "refreshToken", default)]
    pub refresh_token: String,
    #[serde(default = "default_source")]
    pub source: String,
    /// Unix epoch milliseconds. The managed type is a `DateTimeOffset`; storing
    /// milliseconds keeps the JSON shape (`"addedAt": "2024-05-01T00:00:00+00:00"`)
    /// reconstructible without pulling in a date library.
    #[serde(rename = "addedAt", default)]
    pub added_at: Option<String>,
    #[serde(rename = "lastUsedAt", default)]
    pub last_used_at: Option<String>,
}

fn default_source() -> String {
    sources::UNKNOWN.to_owned()
}

impl StoredAccount {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, uuid: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            uuid: uuid.into(),
            access_token: String::new(),
            refresh_token: String::new(),
            source: sources::UNKNOWN.to_owned(),
            added_at: None,
            last_used_at: None,
        }
    }

    #[must_use]
    pub fn has_refresh(&self) -> bool {
        !self.refresh_token.trim().is_empty()
    }

    #[must_use]
    pub fn is_offline(&self) -> bool {
        self.source.eq_ignore_ascii_case(sources::OFFLINE)
    }

    /// Chip label: `Name  (cracked)`, `(Localts)`, `(token)`, `(cookie)`, or the
    /// raw source word for a hand-edited entry.
    #[must_use]
    pub fn display_label(&self) -> String {
        if self.is_offline() {
            format!("{}  (cracked)", self.name)
        } else if self.has_refresh() {
            format!("{}  (Localts)", self.name)
        } else if self.source.eq_ignore_ascii_case(sources::TOKEN) {
            format!("{}  (token)", self.name)
        } else if self.source.eq_ignore_ascii_case(sources::COOKIE) {
            format!("{}  (cookie)", self.name)
        } else {
            format!("{}  ({})", self.name, self.source)
        }
    }
}

/// `accounts-settings.json`, camelCase, unknown keys ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSettingsPayload {
    #[serde(rename = "populateLunarAccountManager", default)]
    pub populate_lunar_account_manager: bool,
}

/// Offline-mode ("cracked") profile derivation.
pub mod offline {
    /// Source marker for an offline account.
    pub const SOURCE_NAME: &str = super::sources::OFFLINE;
    /// Placeholder token: offline servers never validate it.
    pub const OFFLINE_TOKEN: &str = "0";
    /// Message shown when a name fails validation.
    pub const INVALID_NAME_MESSAGE: &str =
        "Usernames must be 3-16 characters using only letters, numbers, and underscores.";

    /// `[A-Za-z0-9_]{3,16}` applied to the trimmed name.
    #[must_use]
    pub fn is_valid_name(name: Option<&str>) -> bool {
        let Some(name) = name else {
            return false;
        };
        let trimmed = name.trim();
        let length = trimmed.len();
        (3..=16).contains(&length)
            && trimmed
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    }

    /// Validates the name and derives the offline UUID (no dashes).
    pub fn try_create(raw_name: Option<&str>) -> Result<(String, String), &'static str> {
        let trimmed = raw_name.unwrap_or("").trim();
        if !is_valid_name(Some(trimmed)) {
            return Err(INVALID_NAME_MESSAGE);
        }

        let uuid = compute_offline_uuid(trimmed).replace('-', "");
        Ok((trimmed.to_owned(), uuid))
    }

    /// The standard offline v3 (MD5) UUID over `"OfflinePlayer:" + name`, which
    /// is what Java's `UUID.nameUUIDFromBytes` produces.
    #[must_use]
    pub fn compute_offline_uuid(name: &str) -> String {
        let mut hash = md5(format!("OfflinePlayer:{name}").as_bytes());
        hash[6] = (hash[6] & 0x0F) | 0x30; // version 3
        hash[8] = (hash[8] & 0x3F) | 0x80; // RFC 4122 variant
        dashed(&hash)
    }

    fn dashed(bytes: &[u8; 16]) -> String {
        let mut out = String::with_capacity(36);
        for (index, byte) in bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                out.push('-');
            }
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    /// MD5 (RFC 1321). Only used for the offline UUID derivation, where the
    /// algorithm is the specification.
    fn md5(input: &[u8]) -> [u8; 16] {
        const SHIFTS: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];
        const TABLE: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
            0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
            0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
            0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
            0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
            0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
            0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
            0xeb86d391,
        ];

        let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];

        let mut message = input.to_vec();
        let bit_length = (input.len() as u64) * 8;
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0);
        }
        message.extend_from_slice(&bit_length.to_le_bytes());

        for chunk in message.as_chunks::<64>().0 {
            let mut words = [0u32; 16];
            for (index, slot) in words.iter_mut().enumerate() {
                let start = index * 4;
                *slot = u32::from_le_bytes([
                    chunk[start],
                    chunk[start + 1],
                    chunk[start + 2],
                    chunk[start + 3],
                ]);
            }

            let [mut a, mut b, mut c, mut d] = state;

            for step in 0..64 {
                let (mixed, word_index) = match step / 16 {
                    0 => ((b & c) | (!b & d), step),
                    1 => ((d & b) | (!d & c), (5 * step + 1) % 16),
                    2 => (b ^ c ^ d, (3 * step + 5) % 16),
                    _ => (c ^ (b | !d), (7 * step) % 16),
                };

                let temp = d;
                d = c;
                c = b;
                let sum = a
                    .wrapping_add(mixed)
                    .wrapping_add(TABLE[step])
                    .wrapping_add(words[word_index]);
                b = b.wrapping_add(sum.rotate_left(SHIFTS[step]));
                a = temp;
            }

            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
        }

        let mut out = [0u8; 16];
        for (index, word) in state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }
}

/// One cookie from a Netscape export or a `Cookie:` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieEntry {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub name: String,
    pub value: String,
}

impl CookieEntry {
    #[must_use]
    pub fn new(
        domain: impl Into<String>,
        path: impl Into<String>,
        secure: bool,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            domain: domain.into(),
            path: path.into(),
            secure,
            name: name.into(),
            value: value.into(),
        }
    }

    #[must_use]
    pub fn pair(&self) -> String {
        format!("{}={}", self.name, self.value)
    }
}

/// A parsed credential file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCookies {
    /// Keyed by `domain\0name`, matching the managed dictionary key.
    pub cookies: BTreeMap<String, CookieEntry>,
    pub refresh_token: String,
    pub access_token: String,
}

impl ParsedCookies {
    /// Joins the `login.live.com` cookies into a request header.
    pub fn to_sisu_cookie_header(&self) -> Result<String, CookieAuthError> {
        let mut by_name: BTreeMap<&str, &CookieEntry> = BTreeMap::new();
        for entry in self.cookies.values() {
            if !entry.domain.to_lowercase().ends_with("login.live.com") {
                continue;
            }
            by_name.entry(entry.name.as_str()).or_insert(entry);
        }

        if by_name.is_empty() {
            return Err(CookieAuthError::new(
                "Cookie file has no login.live.com cookies for SISU auth.",
            ));
        }

        Ok(by_name
            .values()
            .map(|entry| entry.pair())
            .collect::<Vec<_>>()
            .join("; "))
    }
}

/// A credential-file problem the UI shows verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieAuthError {
    pub message: String,
    pub http_status_code: Option<u16>,
    pub is_transient: bool,
    pub retry_after_seconds: Option<u64>,
}

impl CookieAuthError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            http_status_code: None,
            is_transient: false,
            retry_after_seconds: None,
        }
    }
}

impl std::fmt::Display for CookieAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CookieAuthError {}

const MSA_TOKEN_PREFIX: &str = "M.C";
const HTTP_ONLY_PREFIX: &str = "#HttpOnly_";
const UNRECOGNIZED_FORMAT: &str = "Unrecognized cookie format. Use a Netscape cookie file, \
     semicolon-separated cookie header, Localts token file, or Minecraft access-token (eyJ...) \
     file.";

/// Classifies and parses the credential files the account UI accepts:
/// Netscape cookie exports, semicolon-separated cookie headers, Localts `M.C...`
/// token files, and bare Minecraft access-token JWTs.
pub fn parse_cookies(text: &str) -> Result<ParsedCookies, CookieAuthError> {
    let trimmed = normalize_input(text).trim().to_owned();

    if trimmed.contains('\t') && looks_like_netscape(&trimmed) {
        return from_netscape(&trimmed);
    }
    if looks_like_cookie_header(&trimmed) {
        return from_cookie_header(&trimmed);
    }
    if looks_like_localts(&trimmed) {
        return from_localts(&trimmed);
    }
    // `TryExtractMcaToken` refuses tab-separated input (`CookieParser.cs:401`), which is why
    // the managed order sends a tabbed document to the netscape parser below rather than
    // trying the token: a tabbed document is a cookie export, and that parser's error message
    // is the one the user needs. Without this, a broken export that happened to contain a JWT
    // was imported as an access-token account and the cookie file was never mentioned.
    if trimmed.contains('\t') {
        return from_netscape(&trimmed);
    }
    if let Some(token) = extract_mca_token(&trimmed) {
        if is_minecraft_access_token(&token) {
            return Ok(ParsedCookies {
                cookies: BTreeMap::new(),
                refresh_token: String::new(),
                access_token: token,
            });
        }
    }

    Err(CookieAuthError::new(UNRECOGNIZED_FORMAT))
}

/// Strips a BOM, normalises line endings, and rewrites a space-separated
/// Netscape export into the tab-separated form.
fn normalize_input(text: &str) -> String {
    let text = text
        .replace('\u{FEFF}', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    if text.contains('\t') || !looks_like_space_separated_netscape(&text) {
        return text;
    }
    convert_space_separated_netscape(&text)
}

/// Whether the row carries the `#HttpOnly_` cookie prefix.
///
/// `IsCommentLine` and `StripHttpOnlyPrefix` both compared it with
/// `StringComparison.OrdinalIgnoreCase` (`CookieParser.cs:491-496`). Comparing it exactly
/// meant a re-exported or hand-edited file that spells the marker `#HTTPONLY_` lost its
/// `__Host-MSAAUTHP` row to the comment filter, and the import failed with "Cookie file is
/// missing __Host-MSAAUTHP or __Host-MSAAUTH" even though the file was well formed.
fn has_http_only_prefix(line: &str) -> bool {
    let prefix = HTTP_ONLY_PREFIX.as_bytes();
    line.len() >= prefix.len() && line.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn is_comment_line(line: &str) -> bool {
    line.starts_with('#') && !has_http_only_prefix(line)
}

fn strip_http_only_prefix(line: &str) -> &str {
    if has_http_only_prefix(line) {
        // The marker is ASCII, so its length is a char boundary in `line`.
        &line[HTTP_ONLY_PREFIX.len()..]
    } else {
        line
    }
}

/// Splits one space-separated Netscape row into its seven fields.
///
/// Equivalent to the managed `SpaceNetscapeLine` regex
/// `^(\S+)\s+(TRUE|FALSE)\s+(\S+)\s+(TRUE|FALSE)\s+(\d+)\s+(\S+)(?:\s+(.*))?$`: the first six
/// fields are single tokens and the seventh is whatever remains after them, because a cookie
/// value may contain spaces.
///
/// The value is taken positionally. It used to be found by searching the row for the cookie
/// name and taking everything after the hit, which picked the earlier occurrence whenever the
/// name also appeared in the domain or the path: converting `.example.com ... example value`
/// stored `om ... example value` as that cookie's value, so the SISU exchange sent a cookie
/// the browser never wrote.
fn split_space_netscape(line: &str) -> Option<Vec<String>> {
    let mut rest = line;
    let domain = take_netscape_token(&mut rest)?;
    let include_subdomains = take_netscape_token(&mut rest)?;
    if !include_subdomains.eq_ignore_ascii_case("TRUE")
        && !include_subdomains.eq_ignore_ascii_case("FALSE")
    {
        return None;
    }
    let path = take_netscape_token(&mut rest)?;
    let secure = take_netscape_token(&mut rest)?;
    if !secure.eq_ignore_ascii_case("TRUE") && !secure.eq_ignore_ascii_case("FALSE") {
        return None;
    }
    let expiry = take_netscape_token(&mut rest)?;
    if expiry.is_empty() || !expiry.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let name = take_netscape_token(&mut rest)?;
    let value = rest.trim_start().to_owned();

    Some(vec![
        domain,
        include_subdomains,
        path,
        secure,
        expiry,
        name,
        value,
    ])
}

/// Takes the next whitespace-delimited token off the front of `rest`.
///
/// `None` when only whitespace is left, which is how a row runs out of fields.
fn take_netscape_token(rest: &mut &str) -> Option<String> {
    let trimmed = rest.trim_start();
    let end = trimmed
        .find(|character: char| character.is_whitespace())
        .unwrap_or(trimmed.len());
    if end == 0 {
        return None;
    }

    let token = trimmed[..end].to_owned();
    *rest = &trimmed[end..];
    Some(token)
}

fn looks_like_space_separated_netscape(text: &str) -> bool {
    for line in text.split('\n') {
        let candidate = line.trim();
        if candidate.is_empty() || is_comment_line(candidate) {
            continue;
        }
        let candidate = strip_http_only_prefix(candidate);
        return split_space_netscape(candidate).is_some();
    }
    false
}

fn convert_space_separated_netscape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        let stripped = line.trim();
        if stripped.is_empty() || is_comment_line(stripped) {
            out.push_str(line);
            out.push('\n');
            continue;
        }

        let stripped = strip_http_only_prefix(stripped);
        let Some(fields) = split_space_netscape(stripped) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };

        for (index, field) in fields.iter().take(6).enumerate() {
            if index > 0 {
                out.push('\t');
            }
            out.push_str(field);
        }
        if let Some(value) = fields.get(6) {
            if !value.is_empty() {
                out.push('\t');
                out.push_str(value);
            }
        }
        out.push('\n');
    }
    out
}

fn looks_like_netscape(text: &str) -> bool {
    for line in text.split('\n') {
        let candidate = line.trim();
        if candidate.is_empty() || is_comment_line(candidate) {
            continue;
        }
        let candidate = strip_http_only_prefix(candidate);
        return candidate.split('\t').count() >= 6;
    }
    false
}

fn from_netscape(text: &str) -> Result<ParsedCookies, CookieAuthError> {
    let mut cookies = BTreeMap::new();

    for line in text.split('\n') {
        let raw = line.trim();
        if raw.is_empty() || is_comment_line(raw) {
            continue;
        }

        let raw = strip_http_only_prefix(raw);
        let fields: Vec<&str> = raw.split('\t').collect();
        if fields.len() < 6 {
            // Never quote the line: field 7 is a live cookie value that would end up in
            // the status bar and in any screenshot or bug report the user shares.
            return Err(CookieAuthError::new(format!(
                "Invalid cookie line (expected at least 6 tab-separated fields; got {}).",
                fields.len()
            )));
        }

        let domain = fields[0].to_owned();
        let path = fields[2].to_owned();
        let secure = fields[3].eq_ignore_ascii_case("TRUE");
        let name = fields[5];
        let value = fields[6..].join("\t");

        cookies.insert(
            format!("{domain}\0{name}"),
            CookieEntry::new(domain, path, secure, name, value),
        );
    }

    validate_auth_cookies(cookies)
}

fn looks_like_cookie_header(text: &str) -> bool {
    if text.contains('\t') || !text.contains('=') {
        return false;
    }
    if text.contains(';') {
        return true;
    }

    for line in text.split('\n') {
        let candidate = line.trim();
        if !candidate.is_empty() && !candidate.starts_with('#') && candidate.contains('=') {
            return true;
        }
    }

    false
}

fn from_cookie_header(text: &str) -> Result<ParsedCookies, CookieAuthError> {
    const DOMAIN: &str = "login.live.com";
    let mut cookies = BTreeMap::new();

    for segment in text.split(';') {
        let entry = segment.trim();
        if entry.is_empty() {
            continue;
        }

        let Some(equals) = entry.find('=') else {
            continue;
        };
        if equals == 0 {
            continue;
        }

        let name = entry[..equals].trim();
        let value = entry[equals + 1..].trim();
        if name.is_empty() {
            continue;
        }

        cookies.insert(
            format!("{DOMAIN}\0{name}"),
            CookieEntry::new(DOMAIN, "/", true, name, value),
        );
    }

    validate_auth_cookies(cookies)
}

fn validate_auth_cookies(
    cookies: BTreeMap<String, CookieEntry>,
) -> Result<ParsedCookies, CookieAuthError> {
    if cookies.is_empty() {
        return Err(CookieAuthError::new("Cookie file is empty."));
    }

    let has_auth = cookies
        .values()
        .any(|entry| matches!(entry.name.as_str(), "__Host-MSAAUTHP" | "__Host-MSAAUTH"));
    if !has_auth {
        return Err(CookieAuthError::new(
            "Cookie file is missing __Host-MSAAUTHP or __Host-MSAAUTH.",
        ));
    }

    Ok(ParsedCookies {
        cookies,
        refresh_token: String::new(),
        access_token: String::new(),
    })
}

/// `CookieParser.LooksLikeLocaltsRefreshToken`: the `M.C` prefix, and nothing else.
///
/// A minimum length used to be required here (prefix plus eight bytes). The managed parser
/// had no such rule, so a short token file was recognised by the C# host and rejected by
/// this one, and the `text.find(MSA_TOKEN_PREFIX)` fallback below accepted the same token
/// anyway once every line had been skipped.
fn looks_like_localts_refresh_token(candidate: &str) -> bool {
    candidate.starts_with(MSA_TOKEN_PREFIX)
}

fn looks_like_localts(text: &str) -> bool {
    for line in text.split('\n') {
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            continue;
        }
        if candidate.to_lowercase().contains("localts") {
            return true;
        }
        if looks_like_localts_refresh_token(candidate) {
            return true;
        }
        if candidate.contains('\t') || candidate.contains('=') {
            continue;
        }

        if let Some(separator) = candidate.find(':') {
            let value = candidate[separator + 1..].trim();
            if looks_like_localts_refresh_token(value) {
                return true;
            }
        }
    }

    false
}

fn from_localts(text: &str) -> Result<ParsedCookies, CookieAuthError> {
    let token = extract_localts_token(text).ok_or_else(|| {
        CookieAuthError::new(
            "Localts input is missing a valid MSA session token (M.C... or username:M.C...).",
        )
    })?;

    Ok(ParsedCookies {
        cookies: BTreeMap::new(),
        refresh_token: token,
        access_token: String::new(),
    })
}

fn extract_localts_token(text: &str) -> Option<String> {
    for line in text.split('\n') {
        let candidate = line.trim();
        if candidate.is_empty()
            || candidate.starts_with('#')
            || candidate.to_lowercase().contains("localts")
        {
            continue;
        }

        if let Some(separator) = candidate.find(':') {
            let value = candidate[separator + 1..].trim();
            if looks_like_localts_refresh_token(value) {
                return Some(value.to_owned());
            }
        }

        if let Some(equals) = candidate.find('=') {
            let value = candidate[equals + 1..].trim();
            if looks_like_localts_refresh_token(value) {
                return Some(value.to_owned());
            }
        }
    }

    let marker = text.find(MSA_TOKEN_PREFIX)?;
    let mut end = marker;
    let bytes = text.as_bytes();
    while end < bytes.len() {
        match bytes[end] {
            b'\n' | b'\r' | b' ' | b'\t' => break,
            _ => end += 1,
        }
    }

    Some(text[marker..end].to_owned())
}

/// Extracts a Minecraft access token (`eyJ...`) from arbitrary text.
#[must_use]
pub fn extract_mca_token(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;

    while index + 3 <= bytes.len() {
        if &bytes[index..index + 3] != b"eyJ" {
            index += 1;
            continue;
        }

        let start = index;
        let mut cursor = index + 3;

        // `eyJ` is the start of the *header* segment, not the first dot: the rest of
        // that segment has to be consumed before any `.segment` continuation counts.
        while cursor < bytes.len() && is_base64url_byte(bytes[cursor]) {
            cursor += 1;
        }

        // `eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`: exactly two dots.
        if count_jwt_segments(bytes, &mut cursor) == 2 {
            return Some(text[start..cursor].to_owned());
        }

        index += 1;
    }

    None
}

/// Counts `.<segment>` continuations of a JWT-shaped token, advancing `cursor`.
fn count_jwt_segments(bytes: &[u8], cursor: &mut usize) -> usize {
    let mut segments = 0;
    loop {
        let mut probe = *cursor;
        if probe >= bytes.len() || bytes[probe] != b'.' {
            return segments;
        }
        probe += 1;
        let start = probe;
        while probe < bytes.len() && is_base64url_byte(bytes[probe]) {
            probe += 1;
        }
        if probe == start {
            return segments;
        }
        segments += 1;
        *cursor = probe;

        // A JWT has exactly two dots; stop after the signature segment.
        if segments == 2 {
            return segments;
        }
    }
}

fn is_base64url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// Whether a JWT really is a Minecraft access token.
///
/// `login_with_xbox` tokens carry the Minecraft profile (`profiles.mc`, or a
/// `pfd[type=mc]` entry with an id and a name) plus an Xbox id (`xuid`/`xid`). The
/// header must carry `alg`. Claims are read strictly as strings: a pasted token is
/// untrusted input, and a numeric `xuid` or an object claim must reject the token
/// instead of reaching the account store.
fn is_minecraft_access_token(jwt: &str) -> bool {
    let mut parts = jwt.split('.');
    let (Some(header), Some(payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };

    let Some(header) =
        decode_base64url(header).and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    let Some(payload) =
        decode_base64url(payload).and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };

    if header.get("alg").is_none() {
        return false;
    }

    let has_minecraft_profile = payload
        .get("profiles")
        .and_then(Value::as_object)
        .and_then(|profiles| non_empty_string(profiles, "mc"))
        .is_some()
        || payload
            .get("pfd")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    read_string_claim(entry, "type")
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("mc"))
                        && non_empty_string(entry, "id").is_some()
                        && non_empty_string(entry, "name").is_some()
                })
            });

    if !has_minecraft_profile {
        return false;
    }

    non_empty_string(&payload, "xuid").is_some() || non_empty_string(&payload, "xid").is_some()
}

/// A string claim, or `None` when the property is absent or of another type.
fn read_string_claim<'a, P>(parent: &'a P, name: &str) -> Option<&'a str>
where
    P: ClaimLookup,
{
    parent.claim(name).and_then(Value::as_str)
}

/// A non-blank string claim.
fn non_empty_string<'a, P>(parent: &'a P, name: &str) -> Option<&'a str>
where
    P: ClaimLookup,
{
    read_string_claim(parent, name).filter(|value| !value.trim().is_empty())
}

/// Anything a claim can be read from: a JSON value or a JSON object.
trait ClaimLookup {
    fn claim(&self, name: &str) -> Option<&Value>;
}

impl ClaimLookup for Value {
    fn claim(&self, name: &str) -> Option<&Value> {
        self.get(name)
    }
}

impl ClaimLookup for serde_json::Map<String, Value> {
    fn claim(&self, name: &str) -> Option<&Value> {
        self.get(name)
    }
}

/// Decodes URL-safe base64 without padding.
fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'-' | b'+' => Some(62),
            b'_' | b'/' => Some(63),
            b'=' => None,
            _ => None,
        }
    }

    let mut out = Vec::with_capacity(value.len() * 3 / 4 + 3);
    let mut accumulator: u32 = 0;
    let mut bits = 0;

    for byte in value.bytes() {
        if byte == b'=' {
            continue;
        }
        let digit = sextet(byte)?;
        accumulator = (accumulator << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
        }
    }

    Some(out)
}
