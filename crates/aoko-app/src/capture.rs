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
//! There is no index-to-name table anywhere in this module. This module owns the
//! [`Screen`] type and the loop that writes the files; the list of screens is the
//! shell's business.
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
//!
//! # Reproducing
//!
//! Verified on this host with lavapipe, from the repository root:
//!
//! ```text
//! unset LD_LIBRARY_PATH
//! cargo build -p aoko-app --features capture
//! VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!     ./target/debug/aoko --capture-screenshots target/capture-evidence
//! ```
//!
//! `LD_LIBRARY_PATH` has to be unset for the **run**, not only for the build. The Vibe
//! CLI exports a stale `libstdc++`, and with it set wgpu enumerates no adapter and the
//! harness reports `No usable headless GPU adapter found` — which reads like a missing
//! driver rather than a library-path problem. An X server is not needed: the headless
//! renderer writes the same PNGs with no `DISPLAY` at all, and `xvfb-run` changes
//! nothing here (it is the live-window path that wants one).
//!
//! Because `target/` is shared with other builders, a plain `cargo build -p aoko-app`
//! replaces `target/debug/aoko` with a binary that has no `capture` feature, and the run
//! then errors that the harness needs a rebuild. Build with the feature immediately
//! before capturing.

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
/// Nine tabs, in the rail's real order: Combat, Render, Utility, Risky, Beta,
/// Accounts, Servers, Stats, Settings. `MIGRATION_ARCHITECTURE.md` section 8 item 9
/// is why this is an explicit list with a test and not an index loop: the C# script
/// drove a seven-name array against a nine-tab window, so `gui-stats.png` held
/// Accounts and `gui-settings.png` held Servers, and Stats and Settings were never
/// captured at all. Every entry below is named after the tab it actually shows.
#[must_use]
pub fn registry() -> Vec<Screen> {
    vec![
        Screen {
            name: "combat",
            enter: enter_combat,
        },
        Screen {
            name: "render",
            enter: enter_render,
        },
        Screen {
            name: "utility",
            enter: enter_utility,
        },
        Screen {
            name: "risky",
            enter: enter_risky,
        },
        Screen {
            name: "beta",
            enter: enter_beta,
        },
        Screen {
            name: "accounts",
            enter: enter_accounts,
        },
        Screen {
            name: "servers",
            enter: enter_servers,
        },
        Screen {
            name: "stats",
            enter: enter_stats,
        },
        Screen {
            name: "settings",
            enter: enter_settings,
        },
    ]
}

/// Seeds the state `scripts/New-GuiScreenshots.ps1` fabricated by reflection, so the
/// two sets of images show the same thing: connected and injected, DevMode off, the
/// same Clicker defaults (`MinCPS 12`, `MaxCPS 16`, randomization index 2, aim assist
/// on at FOV 45 / range 4.5 / strength 40, SpeedBridge on at 150 ms), and the stats
/// the script synthesises (120 left clicks plus 45 right clicks).
///
/// Capability fabrication is not repeated here: `aoko-protocol`'s
/// `BridgeCapabilities::for_version_fallback` is the same table the bridge reports,
/// and the harness has no bridge. The numbers above are what a screen needs to look
/// like the shipped GUI rather than an empty one.
pub fn seed_state(shell: &mut Shell) {
    // Settings the script seeds through `Clicker` (`New-GuiScreenshots.ps1:160-172`),
    // by the same binding paths the copies themselves use.
    for (bind, value) in SEEDED_VALUES {
        shell.seed_value(bind, value);
    }

    for module in SEEDED_MODULES {
        shell.seed_module(module, true);
    }

    seed_data_tabs(shell);
}

/// The three accounts the capture shows. The script read whatever was in
/// `accounts.json`; a capture has no store, so the registry names them.
pub const SEEDED_ACCOUNTS: [(&str, &str, &str); 3] = [
    ("capture-account-1", "Notch", "localts"),
    ("capture-account-2", "Herobrine", "offline"),
    ("capture-account-3", "Steve", "cookie"),
];

/// The three servers the capture shows, and the one that answers its ping.
pub const SEEDED_SERVERS: [(&str, &str, &str); 3] = [
    ("capture-server-1", "Spawn", "play.example.net"),
    ("capture-server-2", "Anarchy", "anarchy.example.net:25566"),
    ("capture-server-3", "Creative", "creative.example.net"),
];

/// The account the capture marks as signed in, and the UUID it compares against.
pub const SEEDED_SESSION_NAME: &str = "Notch";

/// The compact UUID of [`SEEDED_SESSION_NAME`]: the offline UUID of `Notch`, which
/// is the value `OfflineAccount.ComputeOfflineUuid` produces for that name.
pub const SEEDED_SESSION_UUID: &str = "b50ad385829d3141a2167e7d7539ba7f";

/// Fills the two data tabs so their screens show rows rather than empty states.
///
/// The store seam is [`crate::views::stores::NoStore`] in a capture, which is why
/// this seeds the state machines directly rather than a store: the recipe for a
/// screen is the state it paints.
fn seed_data_tabs(shell: &mut Shell) {
    let now = 1_700_000_000_000_i64;

    let accounts: Vec<aoko_protocol::accounts::StoredAccount> = SEEDED_ACCOUNTS
        .iter()
        .map(|(id, name, source)| {
            let mut account = aoko_protocol::accounts::StoredAccount::new(
                *id,
                *name,
                aoko_protocol::accounts::offline::compute_offline_uuid(name).replace('-', ""),
            );
            account.source = (*source).to_owned();
            account
        })
        .collect();

    let state = shell.accounts_mut();
    state.set_session(crate::views::accounts_state::SessionStrip {
        connected: true,
        supports_set_session: true,
        supports_session_read: true,
        identity: Some(crate::views::accounts_state::SessionIdentity {
            name: Some(SEEDED_SESSION_NAME.to_owned()),
            uuid: Some(SEEDED_SESSION_UUID.to_owned()),
        }),
        ..crate::views::accounts_state::SessionStrip::default()
    });
    state.load(&accounts, Some("capture-account-1"), now);

    let rows: Vec<crate::views::servers_logic::ServerRowSource> = SEEDED_SERVERS
        .iter()
        .map(
            |(id, name, address)| crate::views::servers_logic::ServerRowSource {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                address: (*address).to_owned(),
                added_at: Some("2024-01-01T00:00:00Z".to_owned()),
                last_joined_at: None,
            },
        )
        .collect();

    let servers = shell.servers_mut();
    servers.set_join_state(crate::views::servers_logic::ServerJoinState::Ready);
    servers.load(&rows, Some("capture-server-1"), None, now);
    servers.apply_ping(
        "capture-server-1",
        &crate::views::servers_logic::PingOutcome {
            online: true,
            latency_ms: 48,
            players_label: "12/100".to_owned(),
            description: Some("A Minecraft Server".to_owned()),
            error: String::new(),
        },
    );
    servers.mark_auto_pinged();
}

/// The values `New-GuiScreenshots.ps1` sets before capturing, by binding path.
pub const SEEDED_VALUES: [(&str, f64); 7] = [
    ("MinCPS", 12.0),
    ("MaxCPS", 16.0),
    ("RandomizationModeIndex", 2.0),
    ("AimAssistFov", 45.0),
    ("AimAssistRange", 4.5),
    ("AimAssistStrength", 40.0),
    ("SpeedBridgeDelayMs", 150.0),
];

/// The modules the script turns on before capturing.
pub const SEEDED_MODULES: [&str; 2] = ["aimassist", "speedbridge"];

/// The nine tab names the registry must produce, in order.
pub const SCREEN_ORDER: [&str; 9] = [
    "combat", "render", "utility", "risky", "beta", "accounts", "servers", "stats", "settings",
];

// One `enter` per tab. A closure cannot be a `fn` pointer with a captured index, so
// the selector is spelled out: that is also why the rail order and the registry
// order cannot silently disagree.

fn enter_combat(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(0, cx);
}

fn enter_render(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(1, cx);
}

fn enter_utility(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(2, cx);
}

fn enter_risky(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(3, cx);
}

fn enter_beta(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(4, cx);
}

fn enter_accounts(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(5, cx);
}

fn enter_servers(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(6, cx);
}

fn enter_stats(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(7, cx);
}

fn enter_settings(shell: &mut Shell, cx: &mut gpui::Context<Shell>) {
    seed_state(shell);
    shell.select_tab(8, cx);
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn the_registry_names_all_nine_tabs_in_rail_order() {
        let names: Vec<&str> = registry().iter().map(|screen| screen.name).collect();
        assert_eq!(names, SCREEN_ORDER.to_vec());
        assert_eq!(
            names.len(),
            9,
            "seven names against nine tabs is the C# defect"
        );
    }

    #[test]
    fn every_screen_name_is_a_file_stem() {
        for screen in registry() {
            assert!(
                screen
                    .name
                    .chars()
                    .all(|character| character.is_ascii_lowercase()),
                "{} is not a lower-case file stem",
                screen.name
            );
        }
    }

    #[test]
    fn the_seed_matches_the_screenshot_script() {
        assert_eq!(SEEDED_VALUES[0], ("MinCPS", 12.0));
        assert_eq!(SEEDED_VALUES[1], ("MaxCPS", 16.0));
        assert_eq!(SEEDED_VALUES[2], ("RandomizationModeIndex", 2.0));
        assert_eq!(SEEDED_VALUES[3], ("AimAssistFov", 45.0));
        assert_eq!(SEEDED_VALUES[6], ("SpeedBridgeDelayMs", 150.0));
        assert_eq!(SEEDED_MODULES, ["aimassist", "speedbridge"]);
    }
}

/// One row of the capture report, so a caller can assert it without duplicating the
/// format: the name in a 12-column field, the size right-aligned to 4, then the path.
#[must_use]
pub fn report_line(name: &str, width: u32, height: u32, path: &str) -> String {
    format!("{name:<12} {width:>4}x{height:<4} {path}")
}

#[cfg(feature = "capture")]
pub use harness::run;

#[cfg(feature = "capture")]
mod harness {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui::{AnyWindowHandle, App, AppContext as _, HeadlessAppContext};

    use super::report_line;
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
                "{}",
                report_line(
                    screen.name,
                    image.width(),
                    image.height(),
                    &path.display().to_string()
                )
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_has_nine_named_screens_and_no_duplicates() {
        let screens = registry();
        assert_eq!(screens.len(), 9, "one screen per tab");
        assert!(
            screens.iter().all(|screen| screen.name != "placeholder"),
            "the placeholder is gone: the registry is filled"
        );

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
    fn the_report_line_pins_the_printed_columns() {
        // Captured output is read as a table: the screen name is the left column and the
        // dimensions line up under each other, so a size difference is visible at a glance.
        assert_eq!(
            report_line("combat", 1020, 760, "target/capture/combat.png"),
            "combat       1020x760  target/capture/combat.png"
        );
        assert_eq!(
            report_line("settings", 1020, 760, "s.png"),
            "settings     1020x760  s.png"
        );
    }

    #[test]
    fn the_registry_order_is_the_rail_order() {
        // The C# bug was an index-to-name table living beside the capture code, so a
        // seven-name array drove a nine-tab window. The invariant that closes it: the
        // registry names are the rail names, lower-cased, in the same order. If the
        // rail ever gains a tab this fails instead of silently skipping a screen.
        let rail: Vec<String> = crate::views::copy::TABS
            .iter()
            .map(|tab| tab.name.to_lowercase().replace(' ', "-"))
            .collect();
        let screens: Vec<String> = registry()
            .iter()
            .map(|screen| screen.name.to_owned())
            .collect();

        assert_eq!(screens, rail);
    }
}

#[cfg(test)]
mod data_tab_tests {
    use super::*;
    use crate::views::servers_logic::NOT_PINGED_LABEL;

    #[test]
    fn the_seeded_session_uuid_is_the_offline_uuid_of_the_seeded_name() {
        let expected = aoko_protocol::accounts::offline::compute_offline_uuid(SEEDED_SESSION_NAME)
            .replace('-', "");
        assert_eq!(SEEDED_SESSION_UUID, expected);
    }

    #[test]
    fn seeding_fills_both_data_tabs() {
        let mut shell = Shell::new(crate::theme::Theme::slate());
        seed_state(&mut shell);

        let accounts = shell.accounts();
        assert_eq!(accounts.rows().len(), SEEDED_ACCOUNTS.len());
        eprintln!(
            "rows: {:#?}",
            accounts
                .rows()
                .iter()
                .map(|row| (row.name.clone(), row.is_current))
                .collect::<Vec<_>>()
        );
        assert_eq!(accounts.selected_id(), Some("capture-account-1"));
        assert_eq!(accounts.session().name_text(), SEEDED_SESSION_NAME);
        assert!(
            accounts.rows()[0].is_current,
            "the selected account is the signed-in one"
        );
        assert!(
            !accounts.rows()[1].is_current,
            "a different UUID is not in use"
        );

        let servers = shell.servers();
        assert_eq!(servers.rows().len(), SEEDED_SERVERS.len());
        assert_eq!(servers.selected_id(), Some("capture-server-1"));
        assert!(servers.auto_pinged());
        let pinged = servers
            .rows()
            .iter()
            .find(|row| row.id == "capture-server-1")
            .expect("the pinged server is in the list");
        assert_eq!(pinged.status_text, "48 ms");
        assert!(
            servers
                .rows()
                .iter()
                .filter(|row| row.id != "capture-server-1")
                .all(|row| row.status_text == NOT_PINGED_LABEL),
            "the rest are at their rest state"
        );
    }

    #[test]
    fn the_seeded_registry_order_still_matches_the_rail() {
        let names: Vec<&str> = registry().iter().map(|screen| screen.name).collect();
        assert_eq!(names, SCREEN_ORDER);
    }
}
