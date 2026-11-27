//! Update check: release metadata parsing and version ordering.
//!
//! Port of `Aoko/Core/UpdateService.cs`. The HTTP call is behind
//! [`ReleaseFetcher`] so the whole check is exercisable headlessly, which is what
//! `UpdateServiceTests` needs and what keeps `aoko-core` free of a socket.
//!
//! **No production [`ReleaseFetcher`] exists, and that is a decision rather than a gap left
//! open.** Managed sent one request: `https://api.github.com/repos/naczo5/aoko-client/releases/latest`
//! with `Accept: application/vnd.github+json`, a `User-Agent` and an 8 s timeout
//! (`UpdateService.cs:72-78`). This tree has no client to send it with — `Cargo.lock` carries no
//! `reqwest`, `ureq`, `hyper`, `rustls`, `native-tls` or `openssl`, and gpui is built with
//! `default-features = false`, so its own `http_client` is not compiled either. The two ways to
//! acquire one are a TLS stack (`ring`/`aws-lc`, which has to cross-compile to the Windows leg) or
//! WinHTTP bindings written against the `windows` crate blind, and both are new, unverifiable
//! defence surface added for a version banner. So the port stays whole: the tag rules, the
//! ordering and the trait are exercised below, and the shipped surface paints only what a real
//! event produces — the running version, and no status line, because no check runs. The four
//! `SetUpdateStatus` sentences that would have said otherwise were deleted rather than kept
//! readerless (`16d1030`), and the outcome the check existed for survives on its own: the rail's
//! version line opens the releases page through `aoko_platform::open_url` (`705e936`), which landed
//! as the platform-layer commit this note used to say was missing.
//!
//! Behaviour being preserved:
//!
//! - A GitHub tag is normalised by trimming, dropping any number of leading `v`/`V`
//!   characters, and cutting everything from the first `-` or `+`
//!   (`UpdateService.cs:47-50`).
//! - One to four dot-separated components are accepted, with empty components
//!   removed; anything else is an error (`:52-54`). Missing components are `0`, so
//!   `v0.10` is `0.10.0.0` (`:56`, `:63`).
//! - A negative component is rejected, but `-0` parses to `0` like
//!   `int.TryParse` (`:59`).
//! - `tag_name` is required (`:34-35`); `html_url` falls back to the releases page
//!   when absent or null (`:36-38`).
//! - Ordering is `System.Version` ordering over four components (`:66-67`).
//! - Only `major.minor.build` is ever rendered (`:69-70`), which is also what
//!   `CurrentVersion` uses (`:22-23`).
//!
//! `REQUEST_TIMEOUT_MS`, [`GITHUB_ACCEPT`] and [`user_agent`] are exported so the
//! eventual HTTP implementation in `aoko-platform`/`aoko-app` sends exactly the
//! request the C# client did (`:72-78`).

use std::fmt;
use std::sync::Arc;

/// GitHub endpoint for the newest release (`UpdateService.cs:17`).
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/naczo5/aoko-client/releases/latest";
/// Fallback link when a release carries no `html_url` (`UpdateService.cs:18`).
pub const RELEASES_URL: &str = "https://github.com/naczo5/aoko-client/releases";
/// `HttpClient.Timeout` (`UpdateService.cs:74`).
pub const REQUEST_TIMEOUT_MS: u64 = 8_000;
/// `DefaultRequestHeaders.Accept` (`UpdateService.cs:76`).
pub const GITHUB_ACCEPT: &str = "application/vnd.github+json";

/// A release version with the four components `System.Version` always has here.
///
/// `Ord` is field order, which is exactly `System.Version`'s comparison for
/// non-negative components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReleaseVersion {
    major: i32,
    minor: i32,
    build: i32,
    revision: i32,
}

impl ReleaseVersion {
    /// Builds a version from its four components.
    #[must_use]
    pub const fn new(major: i32, minor: i32, build: i32, revision: i32) -> Self {
        Self {
            major,
            minor,
            build,
            revision,
        }
    }

    /// Major component.
    #[must_use]
    pub const fn major(self) -> i32 {
        self.major
    }

    /// Minor component.
    #[must_use]
    pub const fn minor(self) -> i32 {
        self.minor
    }

    /// Build component.
    #[must_use]
    pub const fn build(self) -> i32 {
        self.build
    }

    /// Revision component. Parsed for ordering, never rendered, as in C#.
    #[must_use]
    pub const fn revision(self) -> i32 {
        self.revision
    }
}

impl fmt::Display for ReleaseVersion {
    /// `major.minor.build`, matching `UpdateService.FormatVersion` (`:69-70`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

/// Why an update check could not produce an answer.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The release body had no string `tag_name` (`:34-35`).
    #[error("the latest release has no tag")]
    MissingTag,
    /// A field that must be a string was another JSON type (`:37`).
    #[error("release field `{field}` is not a string")]
    InvalidField {
        /// Offending JSON key.
        field: &'static str,
    },
    /// The tag is not a version this host understands (`:54`, `:60`).
    #[error("unsupported release version: {value}")]
    UnsupportedVersion {
        /// The rejected value.
        value: String,
    },
    /// The response body was not JSON.
    #[error("release metadata is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The request itself failed (`EnsureSuccessStatusCode`, `:28`).
    #[error("release request failed: {0}")]
    Transport(String),
}

/// What the UI shows after a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckResult {
    /// Running version, `major.minor.build`.
    pub current_version: String,
    /// Newest published version, `major.minor.build`.
    pub latest_version: String,
    /// Link to the release, or the releases page.
    pub release_url: String,
    /// Whether the published version is newer.
    pub is_update_available: bool,
}

/// Transport seam: returns the raw JSON body of [`LATEST_RELEASE_API_URL`].
///
/// Returning the body rather than a decoded struct keeps the parse rules above in
/// one place and lets a test drive them with a literal.
pub trait ReleaseFetcher: Send + Sync {
    /// Fetches the newest release document.
    ///
    /// # Errors
    /// Any transport or status-code failure, as [`UpdateError::Transport`].
    fn latest_release_json(&self) -> Result<String, UpdateError>;
}

/// Reads the version this binary was built as.
///
/// `Assembly.GetName().Version?.ToString(3) ?? "0.0.0"` (`:22-23`) becomes the
/// crate version, rendered the same way.
#[must_use]
pub fn current_version() -> String {
    parse_release_version(env!("CARGO_PKG_VERSION"))
        .map(|version| version.to_string())
        .unwrap_or_else(|_| "0.0.0".to_owned())
}

/// `User-Agent` header value (`:75`).
#[must_use]
pub fn user_agent() -> String {
    format!("aoko-client-update-checker/{}", current_version())
}

/// `UpdateService.ParseReleaseVersion` (`:45-64`).
///
/// # Errors
/// [`UpdateError::UnsupportedVersion`] when the value is not one to four
/// non-negative dot-separated integers.
pub fn parse_release_version(value: &str) -> Result<ReleaseVersion, UpdateError> {
    let normalized = value.trim().trim_start_matches(['v', 'V']);
    let core = match normalized.find(['-', '+']) {
        Some(index) => &normalized[..index],
        None => normalized,
    };

    let unsupported = |value: &str| UpdateError::UnsupportedVersion {
        value: value.to_owned(),
    };

    let mut numbers = [0_i32; 4];
    let mut count = 0_usize;
    for part in core.split('.').filter(|part| !part.is_empty()) {
        if count == numbers.len() {
            return Err(unsupported(value));
        }
        numbers[count] = parse_component(part).ok_or_else(|| unsupported(value))?;
        count += 1;
    }

    if count == 0 {
        return Err(unsupported(value));
    }

    Ok(ReleaseVersion::new(
        numbers[0], numbers[1], numbers[2], numbers[3],
    ))
}

/// `int.TryParse(part, out number) && number >= 0` (`:59`).
fn parse_component(raw: &str) -> Option<i32> {
    // `NumberStyles.Integer` accepts surrounding whitespace and one sign.
    let trimmed = raw.trim();
    let negative = trimmed.starts_with('-');
    let digits = trimmed.strip_prefix(['-', '+']).unwrap_or(trimmed);

    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let mut value: i64 = 0;
    for byte in digits.bytes() {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
        if value > i64::from(i32::MAX) {
            // `int.TryParse` fails above `int.MaxValue`.
            return None;
        }
    }

    // `numbers[i] < 0` is rejected by the C# loop, so `-0` passes and `-1` does not.
    if negative && value != 0 {
        return None;
    }

    i32::try_from(value).ok()
}

/// `UpdateService.IsNewerRelease` (`:66-67`) without the exceptions.
///
/// # Errors
/// [`UpdateError::UnsupportedVersion`] for either argument when unparsable.
pub fn is_newer_release(current_version: &str, latest_tag: &str) -> Result<bool, UpdateError> {
    Ok(parse_release_version(latest_tag)? > parse_release_version(current_version)?)
}

/// The update check over an injected [`ReleaseFetcher`].
pub struct UpdateService {
    fetcher: Arc<dyn ReleaseFetcher>,
}

impl UpdateService {
    /// Wires the transport.
    #[must_use]
    pub fn new(fetcher: Arc<dyn ReleaseFetcher>) -> Self {
        Self { fetcher }
    }

    /// Fetches and evaluates the newest release.
    ///
    /// # Errors
    /// Any [`UpdateError`]: transport, JSON, tag or version format.
    pub fn check(&self) -> Result<UpdateCheckResult, UpdateError> {
        let body = self.fetcher.latest_release_json()?;
        Self::parse_check(&body)
    }

    /// Evaluates a release body that was already fetched.
    ///
    /// # Errors
    /// Any [`UpdateError`] other than [`UpdateError::Transport`].
    pub fn parse_check(body: &str) -> Result<UpdateCheckResult, UpdateError> {
        let document: serde_json::Value = serde_json::from_str(body)?;

        let tag = document
            .get("tag_name")
            .and_then(serde_json::Value::as_str)
            .ok_or(UpdateError::MissingTag)?;

        let release_url = match document.get("html_url") {
            None | Some(serde_json::Value::Null) => RELEASES_URL.to_owned(),
            Some(serde_json::Value::String(url)) => url.clone(),
            // `GetString()` throws for a non-string element, so the C# check fails
            // here too rather than silently falling back.
            Some(_) => {
                return Err(UpdateError::InvalidField { field: "html_url" });
            }
        };

        let current_version = current_version();
        let current = parse_release_version(&current_version)?;
        let latest = parse_release_version(tag)?;

        Ok(UpdateCheckResult {
            current_version,
            latest_version: latest.to_string(),
            release_url,
            is_update_available: latest > current,
        })
    }
}

impl fmt::Debug for UpdateService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateService").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedFetcher {
        body: String,
    }

    impl FixedFetcher {
        fn new(body: impl Into<String>) -> Self {
            Self { body: body.into() }
        }
    }

    impl ReleaseFetcher for FixedFetcher {
        fn latest_release_json(&self) -> Result<String, UpdateError> {
            Ok(self.body.clone())
        }
    }

    struct FailingFetcher;

    impl ReleaseFetcher for FailingFetcher {
        fn latest_release_json(&self) -> Result<String, UpdateError> {
            Err(UpdateError::Transport("connection refused".to_owned()))
        }
    }

    /// `UpdateServiceTests.IsNewerReleaseComparesSemanticVersions`.
    #[test]
    fn is_newer_release_compares_semantic_versions() {
        let rows = [
            ("0.9.1", "v0.9.2", true),
            ("0.9.1", "0.10.0", true),
            ("0.9.1", "v0.10", true),
            ("0.9.1", "v0.9.1", false),
            ("0.9.2", "v0.9.1", false),
            ("1.0.0", "v1.0.1-beta", true),
        ];

        for (current, latest, expected) in rows {
            assert_eq!(
                is_newer_release(current, latest).ok(),
                Some(expected),
                "{current} vs {latest}"
            );
        }
    }

    /// `UpdateServiceTests.ParseReleaseVersionAcceptsGitHubTagPrefixAndMetadata`.
    #[test]
    fn parse_release_version_accepts_a_tag_prefix_and_metadata() {
        assert_eq!(
            parse_release_version("v0.9.1+build.4").ok(),
            Some(ReleaseVersion::new(0, 9, 1, 0))
        );
        assert_eq!(
            parse_release_version("  VV1.2.3.4-rc1 ").ok(),
            Some(ReleaseVersion::new(1, 2, 3, 4))
        );
        assert_eq!(
            parse_release_version("7").ok(),
            Some(ReleaseVersion::new(7, 0, 0, 0))
        );
        // Everything from the first `-` or `+` is chopped off *before* the parts
        // are parsed, so a `-0` component is unreachable: this is `1.`, i.e. `1`.
        assert_eq!(
            parse_release_version("1.-0.3").ok(),
            Some(ReleaseVersion::new(1, 0, 0, 0))
        );
        // `Split('.', RemoveEmptyEntries)`: empty parts disappear, and everything
        // from the first `-`/`+` is dropped.
        assert_eq!(
            parse_release_version("1..").ok(),
            Some(ReleaseVersion::new(1, 0, 0, 0))
        );
        assert_eq!(
            parse_release_version("1.2.3-").ok(),
            Some(ReleaseVersion::new(1, 2, 3, 0))
        );
    }

    #[test]
    fn parse_release_version_rejects_unsupported_shapes() {
        for value in [
            "",
            "   ",
            "v",
            "1.2.3.4.5",
            "1.2.x",
            "-1.0.0",
            "1.2.3b",
            "99999999999",
        ] {
            assert!(
                matches!(
                    parse_release_version(value),
                    Err(UpdateError::UnsupportedVersion { .. })
                ),
                "{value:?} should have been rejected"
            );
        }
    }

    #[test]
    fn versions_render_as_three_components_and_order_by_field() {
        let version = ReleaseVersion::new(0, 9, 1, 7);
        assert_eq!(version.to_string(), "0.9.1");
        assert_eq!(version.major(), 0);
        assert_eq!(version.minor(), 9);
        assert_eq!(version.build(), 1);
        assert_eq!(version.revision(), 7);
        assert!(ReleaseVersion::new(0, 10, 0, 0) > ReleaseVersion::new(0, 9, 9, 9));
        assert!(ReleaseVersion::new(1, 0, 0, 0) > ReleaseVersion::new(0, 99, 99, 99));
    }

    #[test]
    fn check_reports_a_newer_release_from_a_github_body() {
        let service = UpdateService::new(Arc::new(FixedFetcher::new(
            r#"{
                "tag_name": "v99.0.1",
                "html_url": "https://github.com/naczo5/aoko-client/releases/tag/v99.0.1",
                "name": "99.0.1"
            }"#,
        )));

        let result = service.check().expect("a well formed body");
        assert_eq!(result.current_version, current_version());
        assert_eq!(result.latest_version, "99.0.1");
        assert_eq!(
            result.release_url,
            "https://github.com/naczo5/aoko-client/releases/tag/v99.0.1"
        );
        assert!(result.is_update_available);
    }

    #[test]
    fn check_treats_the_running_version_as_current() {
        let service = UpdateService::new(Arc::new(FixedFetcher::new(format!(
            r#"{{"tag_name": "v{}"}}"#,
            current_version()
        ))));

        let result = service.check().expect("a well formed body");
        assert_eq!(result.latest_version, current_version());
        assert_eq!(result.release_url, RELEASES_URL);
        assert!(!result.is_update_available);
    }

    #[test]
    fn check_falls_back_to_the_releases_page() {
        let service = UpdateService::new(Arc::new(FixedFetcher::new(
            r#"{"tag_name": "v99.0.0", "html_url": null}"#,
        )));
        assert_eq!(
            service.check().expect("a well formed body").release_url,
            RELEASES_URL
        );
    }

    #[test]
    fn check_rejects_a_release_without_a_usable_tag() {
        for body in [
            r#"{"html_url": "https://example.invalid"}"#,
            r#"{"tag_name": null}"#,
            r#"{"tag_name": 4}"#,
            r#"[]"#,
        ] {
            let service = UpdateService::new(Arc::new(FixedFetcher::new(body)));
            assert!(
                matches!(service.check(), Err(UpdateError::MissingTag)),
                "{body} should report a missing tag"
            );
        }
    }

    #[test]
    fn check_rejects_a_non_string_release_url() {
        let service = UpdateService::new(Arc::new(FixedFetcher::new(
            r#"{"tag_name": "v99.0.0", "html_url": 5}"#,
        )));

        assert!(matches!(
            service.check(),
            Err(UpdateError::InvalidField { field: "html_url" })
        ));
    }

    #[test]
    fn check_surfaces_invalid_json_and_transport_failures() {
        let service = UpdateService::new(Arc::new(FixedFetcher::new("not json")));
        assert!(matches!(service.check(), Err(UpdateError::Json(_))));

        let service = UpdateService::new(Arc::new(FailingFetcher));
        assert!(matches!(service.check(), Err(UpdateError::Transport(_))));
    }

    #[test]
    fn the_request_metadata_matches_the_managed_client() {
        assert_eq!(
            LATEST_RELEASE_API_URL,
            "https://api.github.com/repos/naczo5/aoko-client/releases/latest"
        );
        assert_eq!(
            RELEASES_URL,
            "https://github.com/naczo5/aoko-client/releases"
        );
        assert_eq!(REQUEST_TIMEOUT_MS, 8_000);
        assert_eq!(GITHUB_ACCEPT, "application/vnd.github+json");
        assert_eq!(
            user_agent(),
            format!("aoko-client-update-checker/{}", current_version())
        );
    }
}
