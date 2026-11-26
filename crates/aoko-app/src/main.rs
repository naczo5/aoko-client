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

use std::process::ExitCode;

use aoko_app::{args, assets, shell::Shell, theme::Theme, window};
use gpui::AppContext as _;

fn main() -> ExitCode {
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
