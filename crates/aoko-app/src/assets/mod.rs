//! Bundled fonts and the `AssetSource` implementation.
//!
//! # Font choice (decision 6, §12 of `MIGRATION_ARCHITECTURE.md`)
//!
//! The C# does not set a family for most of the shell, so it inherits the system
//! UI font; `AccountsView.xaml:5` and `ServersView.xaml:5` name `Segoe UI`
//! explicitly, and three monospace sites name `Consolas`
//! (`CommandSearchOverlay.xaml:240`, `:369`, `MainWindow.xaml:2072`). Both are
//! Windows-only, and GPUI needs an explicitly installed family
//! (`TextSystem::add_fonts`, `GPUI_API_NOTES.md` §8). A missing family therefore
//! falls back silently, which is invisible in code and obvious in a screenshot.
//!
//! Chosen and recorded here once, so no other module re-picks it:
//!
//! | Role | Family | Files |
//! | --- | --- | --- |
//! | UI | **Liberation Sans** | `LiberationSans-Regular.ttf`, `LiberationSans-Bold.ttf` |
//! | Monospace | **Liberation Mono** | `LiberationMono-Regular.ttf`, `LiberationMono-Bold.ttf` |
//!
//! Why these:
//!
//! 1. **Open and redistributable.** Liberation 2.x is SIL OFL 1.1
//!    (`/usr/share/doc/fonts-liberation/copyright`), which permits bundling the
//!    unmodified files in a GPL-3 application. Segoe UI and Consolas cannot be
//!    shipped at all, and several popular "UI" families are ambiguous about
//!    bundling a binary in a repository.
//! 2. **A matched pair.** Both families come from the same digitisation, so the
//!    monospace chips in the overlay line up with the sans text beside them
//!    instead of inheriting a second, unrelated set of vertical metrics.
//! 3. **Closer to the Segoe UI baseline than the obvious alternative.** The
//!    DejaVu families are the ones usually already installed, but DejaVu Sans is
//!    markedly wider than Segoe UI, which pushes every label in the 150 px tab
//!    items and the 340-620 px search card towards wrapping. Liberation Sans is a
//!    humanist-grotesque with tighter advance widths, so the drift against the
//!    C# screenshots is smaller. It is *not* metric-compatible with Segoe UI —
//!    drift is accepted once (decision 6) rather than papered over.
//! 4. **Two weights.** Regular and Bold cover the `SemiBold` (600) weight the
//!    module switches and section titles ask for, which resolves to Bold. Adding
//!    a real 600 face would be an asset-set change, not a code change.
//!
//! Anything that needs a family name reads [`UI_FAMILY`] or [`MONO_FAMILY`]; a
//! literal family string elsewhere is the same class of bug as a hex literal.

use std::borrow::Cow;

use gpui::{App, AssetSource, Result, SharedString};

/// UI text family. Recorded in this module's documentation; do not re-pick it.
pub const UI_FAMILY: &str = "Liberation Sans";
/// Monospace family for hex chips, keybind caps and the overlay's `esc`.
pub const MONO_FAMILY: &str = "Liberation Mono";

const SANS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-Regular.ttf");
const SANS_BOLD: &[u8] = include_bytes!("../../assets/fonts/LiberationSans-Bold.ttf");
const MONO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/LiberationMono-Regular.ttf");
const MONO_BOLD: &[u8] = include_bytes!("../../assets/fonts/LiberationMono-Bold.ttf");

/// Asset table: one entry per file that ships inside the binary.
///
/// Paths are the strings `img(..)`/`svg(..)` resolve through the registered
/// `AssetSource`, so they are relative and stable.
const FILES: &[(&str, &[u8])] = &[
    ("fonts/LiberationSans-Regular.ttf", SANS_REGULAR),
    ("fonts/LiberationSans-Bold.ttf", SANS_BOLD),
    ("fonts/LiberationMono-Regular.ttf", MONO_REGULAR),
    ("fonts/LiberationMono-Bold.ttf", MONO_BOLD),
];

/// The application's asset source.
#[derive(Clone, Copy, Default)]
pub struct Assets;

impl Assets {
    /// Every asset path, for tests and for `list("")`.
    #[must_use]
    pub fn paths() -> Vec<&'static str> {
        FILES.iter().map(|(path, _)| *path).collect()
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let normalised = path.trim_start_matches("./");
        Ok(FILES
            .iter()
            .find(|(candidate, _)| *candidate == normalised)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = path.trim_matches('/');
        Ok(FILES
            .iter()
            .map(|(candidate, _)| *candidate)
            .filter(|candidate| prefix.is_empty() || candidate.starts_with(prefix))
            .map(SharedString::from)
            .collect())
    }
}

/// Installs the bundled families. Call once, before opening any window:
/// `add_fonts` invalidates cached font resolution and clears layout caches.
///
/// Returns an error rather than swallowing one, because a silently missing family
/// is exactly the failure mode decision 6 exists to prevent.
pub fn install_fonts(cx: &mut App) -> Result<()> {
    let text_system = cx.text_system().clone();
    text_system.add_fonts(vec![
        Cow::Borrowed(SANS_REGULAR),
        Cow::Borrowed(SANS_BOLD),
        Cow::Borrowed(MONO_REGULAR),
        Cow::Borrowed(MONO_BOLD),
    ])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_loads_and_lists() {
        let assets = Assets;
        for path in Assets::paths() {
            let bytes = assets.load(path).unwrap().expect("asset present");
            assert!(!bytes.is_empty(), "{path} is empty");
            let listed = assets.list("").unwrap();
            assert!(listed.iter().any(|entry| entry.as_ref() == path));
        }
        assert!(assets.load("fonts/nope.ttf").unwrap().is_none());
    }

    #[test]
    fn bundled_fonts_are_real_true_type_files() {
        // A byte-level sanity check that the assets are the fonts and not a text
        // placeholder; a font that will not parse makes `add_fonts` fail at run time.
        let assets = Assets;
        for (path, family) in [
            ("fonts/LiberationSans-Regular.ttf", UI_FAMILY),
            ("fonts/LiberationSans-Bold.ttf", UI_FAMILY),
            ("fonts/LiberationMono-Regular.ttf", MONO_FAMILY),
            ("fonts/LiberationMono-Bold.ttf", MONO_FAMILY),
        ] {
            let bytes = assets.load(path).unwrap().expect("font present");
            assert!(bytes.len() > 100_000, "{path} is too small to be a font");
            // SFNT version tag: TrueType outlines.
            assert_eq!(
                &bytes[0..4],
                &[0x00, 0x01, 0x00, 0x00],
                "{path} is not a TTF"
            );
            assert!(!family.is_empty());
        }
        assert_eq!(UI_FAMILY, "Liberation Sans");
        assert_eq!(MONO_FAMILY, "Liberation Mono");
    }
}
