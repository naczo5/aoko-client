//! Process enumeration, Minecraft instance discovery and target ranking.
//!
//! The pure half — marker matching, confidence scoring, ordering — lives here so
//! it can be tested on any host; the OS half (PEB reads on Windows, `/proc` on
//! Linux) sits behind [`ProcessInspector`] implementations in the platform
//! modules.

use core::cmp::Ordering;

use crate::error::PlatformError;
use crate::version::{normalize_detected_version, try_parse_version};
use crate::window::WindowHandle;

/// One running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Executable basename, e.g. `java.exe` / `java`.
    pub name: String,
    /// Full command line where readable. `None` when the OS denies it, and also for a process the
    /// backend did not read one for: reading a command line means reading the target's PEB, which
    /// is paid only for a JVM ([`is_java_process_name`]) because that is the only case anything
    /// asks about. A non-JVM entry is therefore `None` rather than "there is no command line".
    pub command_line: Option<String>,
}

/// A process that looks like a Minecraft client we could attach to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceCandidate {
    pub pid: u32,
    /// Human label for the window picker, e.g. `1.8.9 Lunar Client`.
    pub label: String,
    /// Detected Minecraft version where derivable from the command line.
    pub version: Option<String>,
    /// Launcher family, e.g. `lunar`, `prism`, `vanilla`.
    pub launcher: Option<String>,
    /// Path to the JVM executable, when known.
    pub executable: Option<String>,
}

/// Which client family the process metadata points at.
///
/// Mirrors the `ClientType` strings `InjectionTargetDiscovery.Describe` assigns;
/// [`ClientType::label`] returns the exact C# text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientType {
    LunarClient,
    Badlion,
    MinecraftJava,
    /// A JVM process with no client evidence.
    JavaProcess,
}

impl ClientType {
    /// The literal string the managed picker displays.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LunarClient => "Lunar Client",
            Self::Badlion => "Badlion",
            Self::MinecraftJava => "Minecraft Java",
            Self::JavaProcess => "Java process",
        }
    }
}

impl core::fmt::Display for ClientType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.label())
    }
}

/// Everything the target picker shows about one injectable process.
///
/// This is `InjectionTarget` from `Aoko/Core/InjectionTargetDiscovery.cs`,
/// including the confidence that drives ordering and the warning icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryTarget {
    pub pid: u32,
    /// Main window, when the process owns a selectable one.
    pub window: Option<WindowHandle>,
    pub title: String,
    pub process_name: String,
    pub client_type: ClientType,
    /// Detected version, or `None` when the loader must ask the bridge.
    pub detected_version: Option<String>,
    pub detection_reason: String,
    /// 0-100. `>= 60` means "likely Minecraft" and enables auto-selection.
    pub confidence: u8,
}

impl DiscoveryTarget {
    /// `InjectionTarget.IsLikelyMinecraft`.
    #[must_use]
    pub fn is_likely_minecraft(&self) -> bool {
        self.confidence >= 60
    }

    /// `InjectionTarget.DisplayLabel`, including the two-line layout, the em
    /// dash and the bullet the WPF picker renders.
    #[must_use]
    pub fn display_label(&self) -> String {
        let window = if self.title.trim().is_empty() {
            "No titled window"
        } else {
            self.title.as_str()
        };
        let version = self.detected_version.as_deref().unwrap_or("Auto-detect");
        format!(
            "{client} \u{2014} {window} ({process}, PID {pid})\nVersion: {version} \u{2022} {reason}",
            client = self.client_type.label(),
            process = self.process_name,
            pid = self.pid,
            reason = self.detection_reason,
        )
    }

    /// Convenience view for callers that only consume [`InstanceCandidate`].
    #[must_use]
    pub fn to_candidate(&self) -> InstanceCandidate {
        InstanceCandidate {
            pid: self.pid,
            label: match self.detected_version.as_deref() {
                Some(version) => format!("{version} {}", self.client_type.label()),
                None => self.client_type.label().to_string(),
            },
            version: self.detected_version.clone(),
            // The managed host has no launcher concept; only the two launchers
            // it can name with certainty are reported rather than guessing.
            launcher: match self.client_type {
                ClientType::LunarClient => Some("lunar".to_string()),
                ClientType::Badlion => Some("badlion".to_string()),
                ClientType::MinecraftJava | ClientType::JavaProcess => None,
            },
            executable: None,
        }
    }
}

/// `InjectionTargetDiscovery.Describe`: score one JVM process from the evidence
/// available about it.
#[must_use]
pub fn describe_target(
    pid: u32,
    window: Option<WindowHandle>,
    title: &str,
    process_name: &str,
    command_line: Option<&str>,
    executable: Option<&str>,
) -> DiscoveryTarget {
    let safe_title = title.trim().to_string();
    // `string.Join(" ", safeTitle, commandLine ?? "", executablePath ?? "")`
    // then lowercased. Kept as one owned string because every marker check
    // reads it case-insensitively.
    let metadata = format!(
        "{} {} {}",
        safe_title,
        command_line.unwrap_or_default(),
        executable.unwrap_or_default()
    )
    .to_lowercase();

    let (client_type, mut reason, mut confidence) = if contains_any(&metadata, LUNAR_MARKERS) {
        (
            ClientType::LunarClient,
            "Lunar markers in process metadata",
            100u8,
        )
    } else if contains_any(&metadata, BADLION_MARKERS) {
        (
            ClientType::Badlion,
            "Badlion marker in process metadata",
            95u8,
        )
    } else if contains_any(&metadata, MINECRAFT_MARKERS) {
        (
            ClientType::MinecraftJava,
            "Minecraft/client marker in process metadata",
            85u8,
        )
    } else {
        (
            ClientType::JavaProcess,
            if safe_title.trim().is_empty() {
                "No client marker found"
            } else {
                "Java process with a visible window"
            },
            if safe_title.trim().is_empty() { 10 } else { 35 },
        )
    };

    let mut client_type = client_type;
    let detected_version = try_parse_version(command_line.unwrap_or_default())
        .or_else(|| normalize_detected_version(&safe_title))
        .map(str::to_string);

    if detected_version.is_some() && confidence < 60 {
        confidence = 70;
        reason = "Minecraft version found in process metadata";
        client_type = ClientType::MinecraftJava;
    }

    DiscoveryTarget {
        pid,
        window,
        title: safe_title,
        process_name: process_name.to_string(),
        client_type,
        detected_version,
        detection_reason: reason.to_string(),
        confidence,
    }
}

/// `InjectionTargetDiscovery.SortTargets`.
///
/// Confidence first, then "has a window", then client type and title, then pid
/// so the order is total and stable across enumerations.
pub fn sort_targets(targets: &mut [DiscoveryTarget]) {
    targets.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| right.window.is_some().cmp(&left.window.is_some()))
            .then_with(|| compare_ignore_case(left.client_type.label(), right.client_type.label()))
            .then_with(|| compare_ignore_case(&left.title, &right.title))
            .then_with(|| left.pid.cmp(&right.pid))
    });
}

/// `WindowDetection.SelectBestWindow`: prefer a title that names the game, then
/// any window over none. Callers pass an already group-filtered slice.
#[must_use]
pub fn select_best_window(
    windows: &[crate::window::WindowInfo],
) -> Option<&crate::window::WindowInfo> {
    windows.iter().min_by(|left, right| {
        crate::window::is_game_title(&right.title)
            .cmp(&crate::window::is_game_title(&left.title))
            .then_with(|| (right.handle.0 != 0).cmp(&(left.handle.0 != 0)))
    })
}

/// `string.Contains(marker, OrdinalIgnoreCase)`, ASCII-folded.
pub(crate) fn contains_any(haystack: &str, markers: &[&str]) -> bool {
    markers
        .iter()
        .any(|marker| find_ignore_case(haystack, marker).is_some())
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|start| {
        haystack[*start..*start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// Allocation-free ordinal comparison that folds ASCII case, which is what
/// `StringComparer.OrdinalIgnoreCase` does for the titles in the picker.
#[must_use]
pub fn compare_ignore_case(left: &str, right: &str) -> Ordering {
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();
    loop {
        match (left_chars.next(), right_chars.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_char), Some(right_char)) => {
                let (left_char, right_char) = if left_char.is_ascii() && right_char.is_ascii() {
                    (
                        left_char.to_ascii_lowercase(),
                        right_char.to_ascii_lowercase(),
                    )
                } else {
                    (left_char, right_char)
                };
                match left_char.cmp(&right_char) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
        }
    }
}

/// The JVM process names the host inspects, matching
/// `InjectionTargetDiscovery.JavaProcessNames` plus the `.exe` spellings Wine
/// launchers produce on Linux.
#[must_use]
pub fn is_java_process_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("java")
        || name.eq_ignore_ascii_case("javaw")
        || name.eq_ignore_ascii_case("java.exe")
        || name.eq_ignore_ascii_case("javaw.exe")
}

const LUNAR_MARKERS: &[&str] = &["lunarclient", ".lunarclient", "lunar client"];
const BADLION_MARKERS: &[&str] = &["badlion"];
const MINECRAFT_MARKERS: &[&str] = &[
    "minecraft",
    "net.minecraft",
    "mojang",
    "forge",
    "fabric",
    "quilt",
    "feather",
    "labymod",
];

pub trait ProcessInspector: Send + Sync {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError>;
    fn command_line(&self, pid: u32) -> Result<String, PlatformError>;
    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError>;

    /// Full discovery records, including confidence and the detection reason.
    ///
    /// Defaulted so an implementation that only knows [`InstanceCandidate`]
    /// still satisfies the trait; both shipped OS implementations override it
    /// with the real metadata.
    fn discover_targets(&self) -> Result<Vec<DiscoveryTarget>, PlatformError> {
        Ok(self
            .discover_instances()?
            .into_iter()
            .map(|candidate| DiscoveryTarget {
                pid: candidate.pid,
                window: None,
                title: String::new(),
                process_name: candidate.label,
                client_type: ClientType::JavaProcess,
                detected_version: candidate.version,
                detection_reason: "derived from InstanceCandidate".to_string(),
                confidence: 0,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(
        pid: u32,
        hwnd: Option<u64>,
        title: &str,
        client: ClientType,
        confidence: u8,
    ) -> DiscoveryTarget {
        DiscoveryTarget {
            pid,
            window: hwnd.map(WindowHandle),
            title: title.to_string(),
            process_name: "java".to_string(),
            client_type: client,
            detected_version: None,
            detection_reason: String::new(),
            confidence,
        }
    }

    // `InjectionTargetDiscoveryTests.Describe_RecognizesLunarAndCommandLineVersion`
    #[test]
    fn describe_recognizes_lunar_and_command_line_version() {
        let described = describe_target(
            123,
            None,
            "Lunar Client",
            "javaw",
            Some("javaw.exe --version 1.8.9 -Dfoo=lunarclient"),
            Some(r"C:\Users\Test\.lunarclient\jre\bin\javaw.exe"),
        );

        assert_eq!(described.client_type.label(), "Lunar Client");
        assert_eq!(described.detected_version.as_deref(), Some("1.8.9"));
        assert!(described.is_likely_minecraft());
        assert!(described.display_label().contains("PID 123"));
    }

    // `InjectionTargetDiscoveryTests.Describe_UsesVersionAsEvidenceForUnidentifiedJavaProcess`
    #[test]
    fn describe_uses_version_as_evidence_for_unidentified_java_process() {
        let described = describe_target(
            456,
            None,
            "Game Window",
            "java",
            Some("java.exe --version 26.2"),
            None,
        );

        assert_eq!(described.client_type.label(), "Minecraft Java");
        assert_eq!(described.detected_version.as_deref(), Some("26.2"));
        assert!(described.is_likely_minecraft());
        assert_eq!(
            described.detection_reason,
            "Minecraft version found in process metadata"
        );
    }

    // `InjectionTargetDiscoveryTests.SortTargets_PrefersLikelyClientsAndVisibleWindows`
    #[test]
    fn sort_targets_prefers_likely_clients_and_visible_windows() {
        let mut targets = vec![
            DiscoveryTarget {
                pid: 100,
                window: None,
                title: String::new(),
                process_name: "java".to_string(),
                client_type: ClientType::JavaProcess,
                detected_version: None,
                detection_reason: "No client marker found".to_string(),
                confidence: 10,
            },
            DiscoveryTarget {
                pid: 200,
                window: Some(WindowHandle(42)),
                title: "Minecraft".to_string(),
                process_name: "javaw".to_string(),
                client_type: ClientType::MinecraftJava,
                detected_version: Some("1.8.9".to_string()),
                detection_reason: "Minecraft/client marker in process metadata".to_string(),
                confidence: 85,
            },
            target(300, None, "Lunar Client", ClientType::LunarClient, 100),
        ];

        sort_targets(&mut targets);

        let order: Vec<u32> = targets.iter().map(|t| t.pid).collect();
        assert_eq!(order, vec![300, 200, 100]);
    }

    #[test]
    fn sort_targets_breaks_confidence_ties_on_window_then_client_then_title() {
        let mut targets = vec![
            target(3, None, "Beta", ClientType::JavaProcess, 35),
            target(1, Some(7), "Zulu", ClientType::JavaProcess, 35),
            target(2, Some(9), "Alpha", ClientType::JavaProcess, 35),
            target(4, None, "Game", ClientType::MinecraftJava, 35),
        ];

        sort_targets(&mut targets);

        let order: Vec<u32> = targets.iter().map(|t| t.pid).collect();
        // Windowed entries first (by title), then client type, then title: the
        // chain is `ClientType` before `Title`, so "Java process" beats "Game".
        assert_eq!(order, vec![2, 1, 3, 4]);
    }

    #[test]
    fn describe_without_any_evidence_stays_a_low_confidence_java_process() {
        let described = describe_target(7, None, "  ", "java", None, None);
        assert_eq!(described.client_type, ClientType::JavaProcess);
        assert_eq!(described.confidence, 10);
        assert_eq!(described.detection_reason, "No client marker found");
        assert!(!described.is_likely_minecraft());
        assert!(described.display_label().contains("No titled window"));
        assert!(described.display_label().contains("Auto-detect"));
    }

    #[test]
    fn describe_prefers_badlion_over_generic_minecraft_markers() {
        let described = describe_target(
            9,
            None,
            "Badlion Client",
            "javaw",
            Some("javaw.exe -Dbrand=badlion net.minecraft.Main"),
            None,
        );
        assert_eq!(described.client_type, ClientType::Badlion);
        assert_eq!(described.confidence, 95);
    }

    #[test]
    fn describe_keeps_a_high_confidence_client_type_when_a_version_is_found() {
        let described = describe_target(
            11,
            None,
            "Lunar Client",
            "javaw",
            Some("javaw.exe --version 26.2"),
            None,
        );
        assert_eq!(described.client_type, ClientType::LunarClient);
        assert_eq!(described.confidence, 100);
        assert_eq!(
            described.detection_reason,
            "Lunar markers in process metadata"
        );
    }

    #[test]
    fn instance_candidate_allows_unknown_metadata() {
        // A JVM with no title, no version and no client marker: the conversion
        // must carry the unknown metadata through rather than inventing a version
        // or a launcher, which is what a picker would then display as fact.
        let described = describe_target(1, None, "", "java", None, None);
        let candidate = described.to_candidate();

        // The description itself is the empty case, not a fixture the test built.
        assert_eq!(described.client_type, ClientType::JavaProcess);
        assert_eq!(described.confidence, 10);
        assert!(described.detected_version.is_none());

        assert_eq!(candidate.pid, 1);
        assert!(candidate.version.is_none());
        assert!(candidate.launcher.is_none());
        assert!(candidate.executable.is_none());
        assert_eq!(candidate.label, "Java process");
    }

    #[test]
    fn instance_candidate_label_follows_the_documented_shape() {
        let mut described = describe_target(
            5,
            None,
            "Lunar Client",
            "javaw",
            Some("javaw.exe --version 1.8.9 -Dfoo=lunarclient"),
            None,
        );
        let candidate = described.to_candidate();
        assert_eq!(candidate.label, "1.8.9 Lunar Client");
        assert_eq!(candidate.launcher.as_deref(), Some("lunar"));

        described.detected_version = None;
        let candidate = described.to_candidate();
        assert_eq!(candidate.label, "Lunar Client");
    }

    #[test]
    fn java_process_names_cover_windows_and_wine_spellings() {
        for name in ["java", "Java", "javaw", "java.exe", "JAVAW.EXE"] {
            assert!(is_java_process_name(name), "{name} should be a JVM process");
        }
        for name in ["javac", "java8", "node", ""] {
            assert!(
                !is_java_process_name(name),
                "{name} should not be a JVM process"
            );
        }
    }

    #[test]
    fn default_discover_targets_derives_from_candidates() {
        struct OnlyCandidates;
        impl ProcessInspector for OnlyCandidates {
            fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError> {
                Ok(Vec::new())
            }
            fn command_line(&self, _pid: u32) -> Result<String, PlatformError> {
                Err(PlatformError::not_found("none"))
            }
            fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError> {
                Ok(vec![InstanceCandidate {
                    pid: 42,
                    label: "1.8.9 Lunar Client".to_string(),
                    version: Some("1.8.9".to_string()),
                    launcher: Some("lunar".to_string()),
                    executable: None,
                }])
            }
        }

        let targets = OnlyCandidates.discover_targets().expect("default impl");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].pid, 42);
        assert_eq!(targets[0].detected_version.as_deref(), Some("1.8.9"));
        assert_eq!(targets[0].confidence, 0);
        assert!(!targets[0].is_likely_minecraft());
    }
}
