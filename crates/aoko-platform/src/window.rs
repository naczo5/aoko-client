//! Window enumeration, foreground detection and client-area geometry.

use crate::error::PlatformError;
use crate::process::{compare_ignore_case, contains_any};

/// Opaque native window identifier (HWND on Windows, X11 window id elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowHandle(pub u64);

/// Axis-aligned rectangle in screen coordinates, physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub fn is_non_empty(&self) -> bool {
        self.width > 0 && self.height > 0
    }
}

/// One top-level window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub handle: WindowHandle,
    pub title: String,
    pub class_name: Option<String>,
    /// Owning process, when the backend can resolve it.
    pub pid: Option<u32>,
    /// Basename of the owning executable, e.g. `javaw.exe`.
    ///
    /// `WindowDetection.WindowTarget.ProcessName` in the managed host. Kept
    /// separate from [`WindowInfo::pid`] because the picker labels entries with
    /// it and the JVM test is a name test.
    pub process_name: Option<String>,
    /// Outer bounds, including decorations.
    pub bounds: Rect,
}

impl WindowInfo {
    /// `WindowTarget.IsJvm`: the window belongs to a `java`/`javaw` process.
    #[must_use]
    pub fn is_jvm(&self) -> bool {
        self.process_name
            .as_deref()
            .is_some_and(crate::process::is_java_process_name)
    }

    /// `WindowTarget.DisplayLabel`, including the `[JVM]` prefix.
    #[must_use]
    pub fn display_label(&self) -> String {
        let prefix = if self.is_jvm() { "[JVM] " } else { "" };
        let process = self.process_name.as_deref().unwrap_or("unknown");
        let pid = self
            .pid
            .map_or_else(|| "0".to_string(), |pid| pid.to_string());
        format!(
            "{prefix}{title} \u{2014} {process} (PID {pid})",
            title = self.title
        )
    }
}

/// `WindowDetection.GameTitles`, verbatim.
///
/// Three literals, matched as substrings. This list is narrower than the marker
/// list [`crate::describe_target`] scores with on purpose: `fabric`, `forge`,
/// `quilt` and `feather` are project words as much as loader words, so they
/// belong in a "how game-like is this" score and never in "is this the game".
const GAME_TITLE_MARKERS: &[&str] = &["Minecraft", "Lunar Client", "Badlion"];

/// The managed host's title test: `title.Contains(gameTitle, OrdinalIgnoreCase)`
/// for each of [`GAME_TITLE_MARKERS`].
///
/// Ordinal and ASCII-case-folding, so it is a plain substring test with no
/// culture-sensitive casing, exactly like the managed comparison. Shared by
/// [`crate::process::select_best_window`] and [`is_game_window`] so the rule
/// exists once.
pub(crate) fn is_game_title(title: &str) -> bool {
    contains_any(title, GAME_TITLE_MARKERS)
}

/// **Is this window the game?**
///
/// `WindowDetection.IsMinecraftActive`'s title test, asked about one window.
/// The managed host answers yes when the window title contains `Minecraft`,
/// `Lunar Client` or `Badlion` (ordinal, case-insensitive), and it asks that
/// same question in `IsMinecraftActive` and `EnumWindowCallback`.
///
/// This is a yes/no rule, deliberately *not* the confidence score from
/// [`crate::describe_target`]. The score also matches loader and project words
/// (`fabric`, `forge`, `quilt`, `feather`), which is how a focused editor on a
/// mod workspace reaches `is_likely_minecraft()` and gets treated as the game.
/// Anything that acts on a window - synthetic input above all - should gate on
/// this function instead of the score.
///
/// The rule is title-only, exactly as managed: neither
/// [`WindowInfo::class_name`] nor [`WindowInfo::process_name`] is consulted, so
/// a window whose *title* names Minecraft counts as the game even when an
/// editor owns it. That blind spot is inherited, not introduced. Callers that
/// must not act on a non-game process can compose this with
/// [`WindowInfo::is_jvm`], which is true for every window the managed host
/// itself would ever find.
///
/// ```
/// use aoko_platform::{is_game_window, Rect, WindowHandle, WindowInfo};
///
/// let editor = WindowInfo {
///     handle: WindowHandle(1),
///     title: "fabric-example-mod - Visual Studio Code".to_string(),
///     class_name: Some("code".to_string()),
///     pid: Some(42),
///     process_name: Some("code".to_string()),
///     bounds: Rect::default(),
/// };
/// assert!(!is_game_window(&editor), "a mod workspace is not the game");
///
/// let game = WindowInfo {
///     handle: WindowHandle(2),
///     title: "Minecraft 1.8.9".to_string(),
///     class_name: Some("LWJGL".to_string()),
///     pid: Some(43),
///     process_name: Some("javaw.exe".to_string()),
///     bounds: Rect::default(),
/// };
/// assert!(is_game_window(&game));
/// assert!(is_game_window(&game) && game.is_jvm());
/// ```
#[must_use]
pub fn is_game_window(info: &WindowInfo) -> bool {
    is_game_title(&info.title)
}

/// `WindowDetection.SortWindowTargets`: JVM entries first, then alphabetical by
/// title. Stable, so equal titles keep enumeration order.
pub fn sort_windows(windows: &mut [WindowInfo]) {
    windows.sort_by(|left, right| {
        right
            .is_jvm()
            .cmp(&left.is_jvm())
            .then_with(|| compare_ignore_case(&left.title, &right.title))
    });
}

pub trait WindowInspector: Send + Sync {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError>;
    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError>;
    fn client_bounds(&self, handle: WindowHandle) -> Result<Rect, PlatformError>;

    /// Whether the OS is currently drawing the pointer.
    ///
    /// The managed host asks `GetCursorInfo` for this (`WindowDetection.IsCursorVisible`)
    /// and requires both `CURSOR_SHOWING` and a non-null `hCursor`, because the
    /// game hides the pointer with `SetCursor(NULL)` while the mouse is
    /// captured. The loops that act on it (triggerbot, clicker, chest stealer)
    /// all gate on "is the cursor hidden", so a wrong `true` blocks them and a
    /// wrong `false` lets them act.
    ///
    /// `Some(value)` is an answer. `None` is not an answer: it means this
    /// backend cannot tell, and it must never be read as "the cursor is
    /// hidden". A gate of the form `if !cursor_visible() { act }` has to require
    /// `Some(false)` and treat `None` as "not hidden"; treating `None` as
    /// `false` there makes the loop act on a guess. Diagnostic surfaces may show
    /// "unknown" instead. A backend that cannot query the OS may return `None`
    /// only while saying so through [`crate::CapabilityReport`]'s notes, so the
    /// gap is a reported platform limit rather than an undocumented hole.
    fn cursor_visible(&self) -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(title: &str, process: &str, pid: u32) -> WindowInfo {
        WindowInfo {
            handle: WindowHandle(0),
            title: title.to_string(),
            class_name: None,
            pid: Some(pid),
            process_name: Some(process.to_string()),
            bounds: Rect::default(),
        }
    }

    #[test]
    fn zero_size_rect_is_empty() {
        assert!(!Rect::default().is_non_empty());
        assert!(Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1
        }
        .is_non_empty());
    }

    #[test]
    fn negative_size_rect_is_empty() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: -5,
            height: 10,
        };
        assert!(!rect.is_non_empty());
    }

    // `WindowDetectionTests.SortWindowTargets_PlacesJvmEntriesFirst_ThenAlphabeticalByTitle`
    #[test]
    fn sort_windows_places_jvm_entries_first_then_alphabetical_by_title() {
        let mut windows = vec![
            window("Zulu App", "chrome", 100),
            window("Alpha Game", "javaw", 200),
            window("Beta Tool", "notepad", 300),
            window("Minecraft", "java", 400),
        ];

        sort_windows(&mut windows);

        assert!(windows[0].is_jvm());
        assert!(windows[1].is_jvm());
        assert!(!windows[2].is_jvm());
        assert!(!windows[3].is_jvm());
        assert_eq!(windows[0].title, "Alpha Game");
        assert_eq!(windows[1].title, "Minecraft");
        assert_eq!(windows[2].title, "Beta Tool");
        assert_eq!(windows[3].title, "Zulu App");
    }

    // `WindowDetection.IsMinecraftActive` / `EnumWindowCallback`: the three
    // `GameTitles` literals as ordinal, case-insensitive substrings.
    #[test]
    fn is_game_window_matches_the_managed_game_titles() {
        assert!(is_game_window(&window("Minecraft 1.8.9", "javaw", 10)));
        assert!(is_game_window(&window("Lunar Client v1.8.9", "javaw", 11)));
        assert!(is_game_window(&window("Badlion Client 1.8.9", "javaw", 12)));
        // `OrdinalIgnoreCase`, so any casing of the literal matches.
        assert!(is_game_window(&window("MINECRAFT", "java", 13)));
        assert!(is_game_window(&window("[JVM] lunar client", "java", 14)));
        // Substring, not prefix and not a whole word: a real client title with
        // the version in front still matches, and narrowing this rule to make
        // the false-positive test below pass would break it.
        assert!(is_game_window(&window(
            "Minecraft 1.21.4 - Lunar Client",
            "javaw",
            15
        )));
    }

    #[test]
    fn is_game_window_rejects_titles_that_only_look_close() {
        // The managed list requires "Lunar Client", not the bare "lunar" this
        // crate used to match, so unrelated Lunar-titled applications are out.
        assert!(!is_game_window(&window(
            "Lunar Eclipses - Wikipedia",
            "firefox",
            20
        )));
        // Empty and titleless windows are not the game.
        assert!(!is_game_window(&window("", "java", 22)));
    }

    // Inherited limit, pinned rather than wished away: the managed rule is a
    // substring test on the title alone, so any application whose title happens
    // to carry a game name answers `true` here. Narrowing the rule to avoid this
    // would stop detecting real titles like "Minecraft 1.21.4 - Lunar Client",
    // so the false positive is handled at the gate instead of in the rule.
    #[test]
    fn a_non_game_window_whose_title_names_the_game_is_the_inherited_limit() {
        let gallery = window("Badlion skin gallery", "chrome", 21);

        // Managed behaviour, and what the title rule must answer.
        assert!(
            is_game_window(&gallery),
            "Contains(gameTitle, OrdinalIgnoreCase) matches, exactly as managed"
        );

        // The composition an automation gate must use, because `chrome` is not a
        // JVM: the window cannot reach a click.
        let acts = is_game_window(&gallery) && gallery.is_jvm();
        assert!(!acts, "a browser must not pass the automation gate");
    }

    // The case that made the score unusable as a game test: `describe_target`
    // matches loader/project words, so a mod workspace outranks the real game.
    #[test]
    fn an_ide_on_a_mod_workspace_is_not_the_game() {
        let workspace_titles = [
            "fabric-example-mod - Visual Studio Code",
            "forge-1.12.2 (Workspace) - Code",
            "quilt-template - IntelliJ IDEA",
            "feather-library - VSCodium",
        ];

        for title in workspace_titles {
            let editor = window(title, "code", 30);
            assert!(
                !is_game_window(&editor),
                "{title:?} is a mod workspace, not the game"
            );
            assert!(!editor.is_jvm());

            // The scorer still rates it high, which is exactly why the app must
            // stop using the score as a game test.
            let scored = crate::process::describe_target(30, None, title, "code", None, None);
            assert!(
                scored.is_likely_minecraft(),
                "{title:?} is expected to defeat the score, or this test proves nothing"
            );
        }
    }

    // The composition the docs recommend for automation gates.
    #[test]
    fn the_jvm_composition_rejects_an_editor_whose_title_names_minecraft() {
        let editor = window("net.minecraft.client.Main - IntelliJ IDEA", "idea", 40);
        // Title-only, exactly as managed: inherited blind spot.
        assert!(is_game_window(&editor));
        let acts = is_game_window(&editor) && editor.is_jvm();
        assert!(!acts, "an editor must not pass the automation gate");

        let game = window("Minecraft 1.8.9", "javaw.exe", 41);
        assert!(is_game_window(&game) && game.is_jvm());
    }

    #[test]
    fn select_best_window_prefers_a_game_title_over_a_merely_live_handle() {
        let windows = [
            WindowInfo {
                handle: WindowHandle(9),
                ..window("Gradle daemon", "java", 50)
            },
            WindowInfo {
                handle: WindowHandle(7),
                ..window("Minecraft 1.8.9", "javaw", 51)
            },
        ];
        let best = crate::process::select_best_window(&windows).expect("two windows");
        assert_eq!(best.handle, WindowHandle(7));
        assert_eq!(best.title, "Minecraft 1.8.9");
    }

    // `WindowDetectionTests.WindowTarget_DisplayLabel_MarksJvmTargets`
    #[test]
    fn window_info_display_label_marks_jvm_targets() {
        let jvm = window("Lunar Client", "javaw", 42);
        let other = window("Notepad", "notepad", 99);

        assert!(jvm.display_label().starts_with("[JVM]"));
        assert!(!other.display_label().contains("[JVM]"));
        assert!(jvm.display_label().contains("PID 42"));
    }

    #[test]
    fn display_label_uses_the_managed_separator_and_falls_back_to_unknown() {
        let mut info = window("Minecraft 1.8.9", "javaw", 7);
        assert_eq!(
            info.display_label(),
            "[JVM] Minecraft 1.8.9 \u{2014} javaw (PID 7)"
        );
        info.process_name = None;
        info.pid = None;
        assert_eq!(
            info.display_label(),
            "Minecraft 1.8.9 \u{2014} unknown (PID 0)"
        );
    }
}
