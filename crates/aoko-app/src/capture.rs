//! The screenshot harness: mechanism and the screen registry.
//!
//! The C# had no in-app capture code. `scripts/New-GuiScreenshots.ps1` loaded the
//! assembly, drove the real view tree and grabbed the window with `PrintWindow`. Two
//! defects in that script are the reason this module is shaped the way it is
//! (`MIGRATION_ARCHITECTURE.md` §8.9):
//!
//! 1. It drove **tab index** from a seven-name array against nine tabs, so
//!    `gui-stats.png` holds the Accounts tab, `gui-settings.png` holds Servers, and
//!    Stats and Settings were never captured at all.
//! 2. The video tour repeated the offset under different labels.
//!
//! So: the registry is a list of [`Screen`] values, each **named by the screen it
//! shows**, and the harness names the file after the screen rather than by position.
//! There is no index-to-name table anywhere in this module, and [`registry`] starts
//! with exactly one placeholder because filling it with the nine tabs is Builder 5's
//! job.
//!
//! # Capture path
//!
//! The brief preferred a *live* window with `Window::render_to_image()` so client
//! decorations would appear. That path is **not available on Linux at this pin**:
//! `render_to_image` is implemented by `gpui_macos`, `gpui_windows` and the test
//! platform only (`gpui/src/platform.rs:1548` is the default, which bails with
//! "render_to_image not implemented for this platform"), and `gpui_linux` has no
//! implementation. So the harness uses `HeadlessAppContext`, whose `TestPlatform`
//! window does implement it through the wgpu headless renderer.
//!
//! The one thing headless cannot do is set `WindowOptions` through
//! `HeadlessAppContext::open_window`, which hardcodes `focus: false, show: false,
//! ..Default::default()` and so can never ask for client decorations. This module
//! therefore bypasses that helper and calls `App::open_window` with the same
//! [`crate::window::options`] the live app uses. Client-drawn chrome is a *client
//! area*, so it appears in the capture; what is missing is the compositor's own
//! frame, which the C# cropped out anyway (`DWMWA_EXTENDED_FRAME_BOUNDS`).
//!
//! Determinism: the scale factor is pinned to 1.0, the clock is advanced rather than
//! real time waited, and the animation gate is set to reduced motion so every widget
//! renders its end state instead of a frame of a transition.

use crate::shell::Shell;

/// One captured screen.
pub struct Screen {
    /// File stem for the PNG. Lower-case, no spaces: `combat`, `accounts`.
    pub name: &'static str,
    /// Puts the shell into the state this screen shows: selects a tab, seeds state.
    pub enter: fn(&mut Shell, &mut gpui::Context<Shell>),
}

/// The screens to capture, in the order they are written.
///
/// **Exactly one placeholder for now.** Builder 5 replaces this with the nine tabs in
/// order — Combat, Render, Utility, Risky, Beta, Accounts, Servers, Stats, Settings —
/// and the registry test asserts the shape so a shorter list cannot pass as a full run.
#[must_use]
pub fn registry() -> Vec<Screen> {
    vec![Screen {
        name: "placeholder",
        enter: |_shell, _cx| {},
    }]
}

#[cfg(feature = "capture")]
pub use harness::run;

#[cfg(feature = "capture")]
mod harness {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui::{AnyWindowHandle, App, AppContext as _, HeadlessAppContext};

    use crate::assets::{self, Assets};
    use crate::shell::Shell;
    use crate::theme::{motion, Theme};

    /// Anything the harness can fail on.
    pub type HarnessResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    /// Renders every registered screen to `<out_dir>/<name>.png`.
    ///
    /// Prints one line per file with its dimensions, so a caller can report real
    /// numbers rather than "it ran".
    pub fn run(out_dir: &Path, theme: Theme) -> HarnessResult {
        std::fs::create_dir_all(out_dir)?;

        // A still gate makes the frame deterministic: end states, no transitions.
        motion::set_animations_enabled(false);

        let platform_text_system = gpui_platform::current_platform(true).text_system();
        let mut cx =
            HeadlessAppContext::with_platform(platform_text_system, Arc::new(Assets), || {
                gpui_platform::current_headless_renderer()
            });

        cx.update(|app: &mut App| assets::install_fonts(app))?;
        // `AnimationExt` renders end states when this is set, which is the half of the
        // motion gate that covers a call site which ignores the token.
        cx.update(|app: &mut App| app.set_reduce_motion(true));

        for screen in super::registry() {
            // One window per screen: the state goes in at construction, so the harness
            // never reaches into a live entity and a screen cannot inherit the previous
            // screen's state.
            let window = cx.update(|app: &mut App| {
                let root_theme = theme.clone();
                app.open_window(crate::window::options(), move |_window, cx| {
                    cx.new(|cx| {
                        let mut shell = Shell::new(root_theme);
                        (screen.enter)(&mut shell, cx);
                        shell
                    })
                })
            })?;
            let handle: AnyWindowHandle = window.into();

            // Pin the scale factor so text metrics and hairlines are not at the mercy of
            // the environment, then settle the first frame.
            cx.simulate_window_scale_factor_change(handle, 1.0);
            cx.run_until_parked();
            cx.advance_clock(Duration::from_millis(1200));
            cx.run_until_parked();

            let image = cx.capture_screenshot(handle)?;
            let path = out_dir.join(format!("{}.png", screen.name));
            image.save(&path)?;
            println!(
                "{:<12} {:>4}x{:<4} {}",
                screen.name,
                image.width(),
                image.height(),
                path.display()
            );
        }

        Ok(())
    }

    /// The report line's shape, so a caller can assert it without duplicating the format.
    #[cfg(test)]
    #[must_use]
    pub fn report_line(name: &str, width: u32, height: u32, path: &str) -> String {
        format!("{name:<12} {width:>4}x{height:<4} {path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_has_exactly_one_placeholder_and_no_duplicate_names() {
        let screens = registry();
        assert_eq!(screens.len(), 1, "Builder 5 owns the nine-tab registry");
        assert_eq!(screens[0].name, "placeholder");

        // Names are file stems: lower case, no spaces.
        for screen in &screens {
            assert!(!screen.name.is_empty());
            assert!(
                screen
                    .name
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '-'),
                "{} is not a file-stem name",
                screen.name
            );
        }

        let mut names: Vec<&str> = screens.iter().map(|screen| screen.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), screens.len(), "two screens share a file name");
    }

    #[test]
    fn no_tab_order_is_hardcoded_here() {
        // The C# bug was an index-to-name table living beside the capture code. This
        // module must not grow one: the registry is the only order, and it is by name.
        //
        // The needles are assembled at run time so this test's own source cannot satisfy
        // its check, which is exactly what happened the first time it ran.
        let source = include_str!("capture.rs");
        let needles = [
            concat!("Selected", "Index"),
            concat!("\"comb", "at\""),
            concat!("\"rend", "er\""),
            concat!("TAB_", "ORDER"),
        ];
        for needle in needles {
            assert!(
                !source.contains(needle),
                "capture.rs must not hardcode {needle}; the registry owns names"
            );
        }
    }
}
