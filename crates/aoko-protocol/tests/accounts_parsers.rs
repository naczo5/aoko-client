//! Port of `Aoko.Tests/CookieParserClaimTests.cs`, plus the pure `Accounts/*` parsers.
//!
//! A pasted token is untrusted input: every claim read must survive a non-string value,
//! and no failure message may quote a cookie value, because parser messages reach the
//! status bar, the batch summary, and screenshots.

use aoko_protocol::accounts::offline::{
    compute_offline_uuid, is_valid_name, try_create, INVALID_NAME_MESSAGE, OFFLINE_TOKEN,
    SOURCE_NAME,
};
use aoko_protocol::accounts::{
    extract_mca_token, parse_cookies, source_display, sources, StoredAccount,
};

const SECRET: &str = "SUPER-SECRET-AUTH-VALUE";

fn base64_url(value: &str) -> String {
    // The fixtures only need the URL-safe alphabet, so a hand-rolled encoder keeps the
    // test dependency-free.
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let triple = u32::from_be_bytes([0, buffer[0], buffer[1], buffer[2]]);
        let indexes = [
            ((triple >> 18) & 0x3F) as usize,
            ((triple >> 12) & 0x3F) as usize,
            ((triple >> 6) & 0x3F) as usize,
            (triple & 0x3F) as usize,
        ];
        for index in indexes.iter().take(chunk.len() + 1) {
            out.push(ALPHABET[*index] as char);
        }
    }
    out
}

fn jwt(payload_json: &str) -> String {
    format!(
        "{}.{}.sig",
        base64_url("{\"alg\":\"RS256\",\"typ\":\"JWT\"}"),
        base64_url(payload_json)
    )
}

#[test]
fn a_pasted_token_with_non_string_claims_fails_as_a_domain_error() {
    let payloads = [
        r#"{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":2533274909086470}"#,
        r#"{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":["1","2"]}"#,
        r#"{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xid":{"id":"1"}}"#,
        r#"{"pfd":[{"type":1,"id":"069a79f4","name":"Notch"}],"xuid":"1"}"#,
        r#"{"pfd":[{"type":"mc","id":69,"name":"Notch"}],"xuid":"1"}"#,
        r#"{"profiles":"069a79f444e94726a5befca90e38aaf5","xuid":"1"}"#,
        r#"{"pfd":"mc","xuid":"1"}"#,
    ];

    for payload in payloads {
        let error = parse_cookies(&jwt(payload)).expect_err("must be a domain error");
        assert!(
            error.message.contains("Unrecognized cookie format"),
            "{} -> {}",
            payload,
            error.message
        );
    }
}

#[test]
fn a_malformed_netscape_line_does_not_quote_the_cookie_value() {
    let text = format!("# Netscape HTTP Cookie File\n.login.live.com\tTRUE\t/\t{SECRET}\n");

    let error = parse_cookies(&text).expect_err("must reject");

    assert!(!error.message.contains(SECRET), "{}", error.message);
    assert!(
        error
            .message
            .contains("expected at least 6 tab-separated fields"),
        "{}",
        error.message
    );
}

#[test]
fn no_parse_failure_echoes_what_the_user_pasted() {
    let inputs = [
        "nonsense".to_owned(),
        "a=b".to_owned(),
        ".example.com\tTRUE\t/".to_owned(),
        format!(".login.live.com\tTRUE\t/\tFALSE\t0\tother\t{SECRET}"),
        format!(".login.live.com\tTRUE\t/\tFALSE\t0\tother\t{SECRET}\t9"),
    ];

    for input in inputs {
        if let Err(error) = parse_cookies(&input) {
            assert!(!error.message.contains(SECRET), "{}", error.message);
        }
    }
}

#[test]
fn a_valid_token_with_string_claims_still_parses() {
    let token =
        jwt(r#"{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":"2533274909086470"}"#);

    let parsed = parse_cookies(&token).expect("parses");

    assert_eq!(parsed.access_token, token);
    assert_eq!(parsed.refresh_token, "");
}

#[test]
fn a_netscape_export_yields_the_auth_pair() {
    let text = format!(
        "# Netscape HTTP Cookie File\n\
         .login.live.com\tTRUE\t/\tTRUE\t0\t__Host-MSAAUTHP\t{SECRET}\n\
         .login.live.com\tTRUE\t/\tTRUE\t0\tMSPCID\tanother\n"
    );

    let parsed = parse_cookies(&text).expect("parses");

    // Cookie parses fill `cookies`, not `access_token`: the auth pair is exchanged
    // later by the SISU flow, which lives in `aoko-core`.
    assert!(parsed.access_token.is_empty());
    assert_eq!(parsed.cookies.len(), 2);
    let header = parsed.to_sisu_cookie_header().expect("header");
    assert!(header.contains("__Host-MSAAUTHP="), "{header}");
    assert!(header.contains(SECRET), "{header}");
}

#[test]
fn a_cookie_header_paste_round_trips_into_a_header() {
    let text = format!("__Host-MSAAUTHP={SECRET}; __Host-MSAAUTH=another");

    let parsed = parse_cookies(&text).expect("parses");

    assert!(parsed.access_token.is_empty());
    assert!(parsed
        .to_sisu_cookie_header()
        .expect("header")
        .contains(SECRET));
}

#[test]
fn a_localts_token_file_is_recognised() {
    let text = "M.C522.ABCDEF";

    let parsed = parse_cookies(text).expect("parses");

    // A `M.C...` file is a Localts *refresh* token: cookies are empty, the access token
    // is not set yet, and the raw token becomes `RefreshToken` for the MSA exchange.
    assert!(parsed.cookies.is_empty());
    assert_eq!(parsed.access_token, "");
    assert_eq!(parsed.refresh_token, text);
}

#[test]
fn extract_mca_token_reads_the_shipped_line_shape() {
    let text = "token:eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig:1799999999";

    assert_eq!(
        extract_mca_token(text).as_deref(),
        Some("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig")
    );
    assert!(extract_mca_token("no token here").is_none());
    assert!(extract_mca_token("").is_none());
}

#[test]
fn a_space_separated_netscape_value_is_the_rest_of_the_row() {
    // The row's cookie name also occurs inside its own domain. The value used to be taken by
    // searching the row for the name, so the hit landed in `.example.com` and the stored value
    // became `om TRUE / TRUE 0 example positional value`.
    let text = "# Netscape HTTP Cookie File\n\
                .login.live.com TRUE / TRUE 0 __Host-MSAAUTHP secret value\n\
                .example.com TRUE / TRUE 0 example positional value\n";

    let parsed = parse_cookies(text).expect("parses");

    let cookie = parsed
        .cookies
        .get(".example.com\0example")
        .expect("the space-separated row keeps its cookie");
    assert_eq!(cookie.value, "positional value");
    assert_eq!(cookie.name, "example");
    assert_eq!(
        parsed.cookies[".login.live.com\0__Host-MSAAUTHP"].value,
        "secret value"
    );
}

#[test]
fn an_http_only_row_is_not_a_comment_whatever_the_marker_case() {
    // `IsCommentLine` matched the marker with `OrdinalIgnoreCase`, so `#HTTPONLY_` is a cookie
    // row and not a comment. Treated as a comment, the only row of the file disappeared and the
    // import reported "Cookie file is empty."
    let text = "# Netscape HTTP Cookie File\n\
                #HTTPONLY_.login.live.com\tTRUE\t/\tTRUE\t0\t__Host-MSAAUTHP\tsecret\n";

    let parsed = parse_cookies(text).expect("parses");

    assert_eq!(parsed.cookies.len(), 1);
    assert!(parsed
        .cookies
        .contains_key(".login.live.com\0__Host-MSAAUTHP"));
}

#[test]
fn a_short_localts_token_is_still_a_localts_token() {
    // `LooksLikeLocaltsRefreshToken` was the bare `M.C` prefix; a length rule here rejected a
    // token the managed host accepted.
    let parsed = parse_cookies("M.C42").expect("parses");

    assert!(parsed.cookies.is_empty());
    assert_eq!(parsed.refresh_token, "M.C42");
    assert_eq!(parsed.access_token, "");
}

#[test]
fn a_tabbed_export_with_a_token_is_a_cookie_file_not_a_token_file() {
    // `TryExtractMcaToken` refuses tab-separated input, so a malformed cookie export that
    // contains a JWT reports the cookie-file error instead of importing the token.
    let token =
        jwt(r#"{"profiles":{"mc":"069a79f444e94726a5befca90e38aaf5"},"xuid":"2533274909086470"}"#);
    let text = format!(".login.live.com\tTRUE\t/\t{token}\n");

    let error = parse_cookies(&text).expect_err("a tabbed export is a cookie file");

    assert!(
        error
            .message
            .contains("expected at least 6 tab-separated fields"),
        "{}",
        error.message
    );
    assert!(!error.message.contains(&token), "{}", error.message);
}

#[test]
fn source_display_covers_every_known_source() {
    assert_eq!(source_display(Some(sources::COOKIE)), "Microsoft cookies");
    assert_eq!(source_display(Some(sources::LOCALTS)), "Localts token");
    assert_eq!(source_display(Some(sources::TOKEN)), "access token");
    assert_eq!(source_display(Some(sources::OFFLINE)), "cracked name");
    assert_eq!(source_display(Some(sources::UNKNOWN)), "unknown");
    assert_eq!(source_display(None), "unknown source");
    assert_eq!(source_display(Some("  ")), "unknown source");
    assert_eq!(source_display(Some("future-source")), "future-source");
}

#[test]
fn offline_names_are_validated_like_the_managed_host() {
    for name in ["Notch", "jeb_", "abc", "Player123", "sixteen_chars_16"] {
        assert!(is_valid_name(Some(name)), "{name}");
    }
    for name in [
        None,
        Some(""),
        Some("   "),
        Some("ab"),
        Some("bad name"),
        Some("a-b"),
        Some("seventeen_chars_17"),
    ] {
        assert!(!is_valid_name(name), "{name:?}");
    }
}

#[test]
fn offline_creation_builds_the_offline_account() {
    let (name, uuid) = try_create(Some("  Notch  ")).expect("created");

    assert_eq!(name, "Notch");
    assert_eq!(
        uuid, "b50ad385829d3141a2167e7d7539ba7f",
        "`TryCreate` strips the dashes from the dashed UUID"
    );
    assert_eq!(try_create(Some("bad name")), Err(INVALID_NAME_MESSAGE));
    assert_eq!(OFFLINE_TOKEN, "0");
    assert_eq!(SOURCE_NAME, "offline");
}

#[test]
fn offline_uuids_match_the_reference_vectors() {
    assert_eq!(
        compute_offline_uuid("Notch"),
        "b50ad385-829d-3141-a216-7e7d7539ba7f"
    );
    assert_eq!(
        compute_offline_uuid("Steve"),
        "5627dd98-e6be-3c21-b8a8-e92344183641"
    );
    assert_eq!(
        compute_offline_uuid("jeb_"),
        "a762f560-4fce-3236-812a-b80efff0b62b"
    );

    // Version 3, RFC 4122 variant.
    let uuid = compute_offline_uuid("Notch");
    assert_eq!(&uuid[14..15], "3", "{uuid}");
    assert!(matches!(&uuid[19..20], "8" | "9" | "a" | "b"), "{uuid}");
}

#[test]
fn stored_accounts_report_their_kind() {
    let mut offline = StoredAccount::new("1", "Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f");
    assert!(!offline.is_offline(), "the default source is `unknown`");
    offline.source = sources::OFFLINE.to_owned();
    assert!(offline.is_offline());

    let mut online = StoredAccount::new("2", "Notch", "b50ad385-829d-3141-a216-7e7d7539ba7f");
    online.refresh_token = "refresh".to_owned();
    assert!(!online.is_offline());
    assert!(online.has_refresh());
    assert!(online.display_label().contains("Notch"));
}
