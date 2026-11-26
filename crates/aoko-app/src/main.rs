//! The binary: one `main`, one window, one mount point.
//!
//! Builder 4 owns this file (the boundary in `UI_INVENTORY.md` §7): both builders want
//! window options, and a second `main` would mean two window configurations that can
//! drift. Builder 5 owns [`aoko_app::shell::Shell`]'s body and, through it, everything
//! drawn inside the window.
//!
//! `--capture-screenshots <out_dir>` is part of the verification contract
//! (`MIGRATION_ARCHITECTURE.md` §10), so it is parsed here and dispatched to
//! [`aoko_app::capture`]. Without the `capture` feature the flag explains itself rather
//! than pretending the build is complete: `Window::render_to_image` and the headless
//! renderer are `test-support`-gated at the pin.
//!
//! # Logging
//!
//! Phase 1 stopped swallowing errors and started returning them with a classification.
//! Returning them is not the same as showing them: until a subscriber exists, every
//! `warn!`/`error!`/`debug!` in `aoko-core`, `aoko-platform` and the shell is written
//! into a void. [`init_logging`] is the one place a subscriber is installed.

use std::process::ExitCode;

use aoko_app::{args, assets, shell::Shell, theme::Theme, window};
use gpui::AppContext as _;

/// The filter used when `RUST_LOG` is unset, and why.
///
/// `warn` for everything, `info` for our own four crates. The errors Phase 1 stopped
/// swallowing are `warn`/`error`, so they show without configuration; our own `info`
/// records are few and describe state a user would otherwise have to guess (today:
/// whether legacy profiles were migrated); dependency `info`/`debug` output from wgpu,
/// naga, calloop and x11 is exactly what a desktop run must not print, because it
/// drowns the lines that matter. `RUST_LOG` replaces this filter wholesale, so
/// `RUST_LOG=debug` still gives everything.
#[cfg(not(any(test, feature = "capture")))]
const DEFAULT_FILTER: &str =
    "warn,aoko_app=info,aoko_platform=info,aoko_core=info,aoko_protocol=info";

fn main() -> ExitCode {
    init_logging();

    let parsed = match args::parse(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("aoko: {error}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };

    if parsed.help {
        println!("{}", args::USAGE);
        return ExitCode::SUCCESS;
    }
    if parsed.version {
        println!("aoko {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    // An unknown name falls back to Slate, the same rule `ApplyGuiTheme` uses.
    let theme = Theme::by_name(parsed.theme.as_deref().unwrap_or("Slate"));

    if let Some(out_dir) = parsed.capture_screenshots {
        return capture(out_dir, theme);
    }

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            if let Err(error) = assets::install_fonts(cx) {
                // A missing family falls back silently and reflows every measurement, so
                // this is fatal rather than a warning.
                panic!("cannot install the bundled fonts: {error}");
            }
            cx.open_window(window::options(), move |_window, cx| {
                cx.new(|_| Shell::new(theme))
            })
            .expect("open the application window");
        });

    ExitCode::SUCCESS
}

/// Installs the process-wide subscriber, once, before anything can log.
///
/// Tests install nothing at all (`cfg(test)`): a global subscriber would be installed by
/// whichever test got there first, and a test that asserts on output would become
/// order-dependent on the test runner.
///
/// A capture build installs nothing *unless* `RUST_LOG` is set. A capture is a render,
/// not a session: its table goes on stdout and its evidence is nine PNGs, so a stray
/// `warn!` interleaved with them makes the run harder to read. The gate is runtime rather
/// than `cfg` because a capture that swallows a `warn!` is exactly when someone wants to
/// see it, and `RUST_LOG` is the switch they already know; a capture with no `RUST_LOG`
/// installs no subscriber and prints nothing but its table.
///
/// `try_init`, not `init`: a second install (a harness, a child process, a re-entrant
/// `main`) must not panic. Whichever install lands first wins.
#[cfg(not(test))]
fn init_logging() {
    use tracing_subscriber::EnvFilter;

    let configured = EnvFilter::try_from_default_env();

    #[cfg(feature = "capture")]
    let filter = match configured {
        Ok(filter) => filter,
        // Quiet unless asked: a capture with no `RUST_LOG` installs no subscriber.
        Err(_) => return,
    };

    #[cfg(not(feature = "capture"))]
    let filter = configured.unwrap_or_else(|_| {
        // Unset, or set to something `EnvFilter` cannot parse: fall back rather than
        // refuse to start, because losing logs must never cost the application.
        EnvFilter::new(DEFAULT_FILTER)
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        // stderr, never stdout: `--help`, `--version` and the capture table own stdout,
        // and a caller parsing them should not have to strip log lines first.
        .with_writer(std::io::stderr)
        .try_init();
}

/// The test harness stays quiet. See the documented variant above.
#[cfg(test)]
fn init_logging() {}

#[cfg(feature = "capture")]
fn capture(out_dir: std::path::PathBuf, theme: Theme) -> ExitCode {
    match aoko_app::capture::run(&out_dir, theme) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("aoko: capture failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "capture"))]
fn capture(_out_dir: std::path::PathBuf, _theme: Theme) -> ExitCode {
    eprintln!(
        "aoko: this build has no capture support.\n\
         \n\
         `--capture-screenshots` needs `Window::render_to_image` and\n\
         `gpui_platform::current_headless_renderer`, both of which are gated behind\n\
         gpui's `test-support` feature at the pinned revision. Rebuild with:\n\
         \n\
         \x20   cargo run -p aoko-app --features capture -- --capture-screenshots <out_dir>"
    );
    ExitCode::from(1)
}
