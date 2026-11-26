//! The custom-palette file and the create-palette card's editor state.
//!
//! `aoko_protocol::palettes` already owns the JSON half: `CustomPalette`, its camelCase
//! field names and the load/save/upsert/delete primitives (`Aoko/Core/CustomPalette.cs`).
//! What nothing owned was the other half of `PaletteStore` (`:38-114`): which file, what
//! is applied right now, what the five editor boxes hold, and how a name becomes a
//! [`Theme`]. That is this module.
//!
//! Three pieces of state, one per thing the managed app keeps:
//!
//! * the **file**, `<app data>/Aoko/palettes.json` (`:43-51`), read on every operation
//!   and written whole, because `PaletteStore` has no cache either;
//! * the **current** palette name, the C# `Clicker.GuiTheme` the Settings card shows on
//!   `Current palette: ` (`MainWindow.xaml:1904`);
//! * the **draft**, the name box and the four hex boxes (`:1952-2001`). The other five
//!   roles are not edited: the card's own caption says they are derived (`:1946-1947`)
//!   and `PaletteDraft::build` derives them with `AddCustomPaletteButton_Click`'s
//!   formulas (`MainWindow.xaml.cs:1128-1143`).
//!
//! Resolution is `NormalizeThemeName` (`MainWindow.xaml.cs:1292-1306`), which is not the
//! same rule as [`Theme::by_name`]: managed looks the name up in `GuiPalettes`, a table
//! seeded with the 18 built-ins (`:143-373`) and appended to by `RegisterPalette`
//! (`:1005-1032`), and it matches case-insensitively, returning the stored spelling.
//! [`PaletteStore::resolve`] reproduces that order — built-ins first, then the file — so
//! a name that exists as both resolves the way managed resolves it.
//!
//! Nothing paints this yet. The palette editor card is in the Settings table
//! (`copy::SETTINGS`) and its rows are shell-owned `Control` variants that do not exist;
//! this module is what those rows will read, and it deliberately imports no GPUI type.

use std::fs;
use std::path::{Path, PathBuf};

use aoko_platform::{app_data_directory, PlatformError};
use aoko_protocol::palettes::{
    delete, from_palettes_json, to_palettes_json, upsert, CustomPalette,
};

use crate::theme::{Color, CustomRoles, Theme};
use crate::views::copy;

/// The folder inside the app-data root that holds this client's files
/// (`Aoko/Core/CustomPalette.cs:46-47`).
///
/// [`app_data_directory`] resolves the root only, the same way
/// `aoko_protocol::profile::get_profiles_directory` appends this segment itself.
pub const APP_DIR: &str = "Aoko";

/// The file inside that folder (`Aoko/Core/CustomPalette.cs:50`).
pub const FILE_NAME: &str = "palettes.json";

/// The palette applied when none was chosen, and the fallback after a delete
/// (`MainWindow.xaml.cs:1290`, `:1300`, `:1202`).
pub const DEFAULT_PALETTE: &str = "Slate";

/// Longest accepted name, in UTF-16 units, because `name.Length` is
/// (`MainWindow.xaml.cs:1214`).
pub const MAX_NAME_LENGTH: usize = 32;

/// The five boxes of the create-palette card, held as the text the user has typed.
///
/// The hex boxes are text rather than [`Color`] on purpose: the managed card validates on
/// the press, not on every keystroke, and an invalid draft must stay visible so the user
/// can fix it instead of being silently reset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteDraft {
    /// `NewPaletteNameBox` (`MainWindow.xaml:1952`).
    pub name: String,
    /// `PaletteBgBox` (`:1963`).
    pub background: String,
    /// `PalettePanelBox` (`:1974`).
    pub panel: String,
    /// `PaletteAccentBox` (`:1985`).
    pub accent: String,
    /// `PaletteTextBox` (`:1996`).
    pub text: String,
}

impl Default for PaletteDraft {
    /// The markup's own initial values, so a first frame matches the managed window.
    fn default() -> Self {
        Self {
            name: String::new(),
            background: "#0A0B0F".to_owned(),
            panel: "#12141A".to_owned(),
            accent: "#C7625A".to_owned(),
            text: "#E8EAEE".to_owned(),
        }
    }
}

impl PaletteDraft {
    /// The palette this draft describes, or `None` when one of the four hexes does not
    /// parse.
    ///
    /// Port of the palette literal in `AddCustomPaletteButton_Click`
    /// (`MainWindow.xaml.cs:1128-1143`): the four typed colours are written back as
    /// `#RRGGBB` (`ColorToHex`, `:1257`) and the five remaining roles are derived from
    /// them — the same formulas [`Theme::from_custom`] falls back to, so a stored palette
    /// and a palette built from the same four hexes cannot drift.
    fn build(&self, name: &str) -> Option<CustomPalette> {
        let background = parse_entry(&self.background)?;
        let panel = parse_entry(&self.panel)?;
        let accent = parse_entry(&self.accent)?;
        let text = parse_entry(&self.text)?;

        Some(CustomPalette {
            name: name.to_owned(),
            background: background.to_hex(),
            panel: panel.to_hex(),
            slider_background: panel.lighten(0.06).to_hex(),
            slider_foreground: panel.lighten(0.16).to_hex(),
            accent: accent.to_hex(),
            text: text.to_hex(),
            dim_text: text.blend(background, 0.45).to_hex(),
            tab_selected: panel.lighten(0.10).to_hex(),
            tab_hover: panel.lighten(0.04).to_hex(),
        })
    }
}

/// Why an Add-palette press stored nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// The name failed `IsValidPaletteName` (`MainWindow.xaml.cs:1212-1224`).
    NameInvalid,
    /// The name is one of the 18 built-ins (`:1105`), carrying the name as typed.
    NameBuiltin(String),
    /// One of the four hex boxes did not parse (`:1112-1117`).
    ColorsInvalid,
    /// The file could not be written (`:1152-1156`), carrying the I/O message.
    Write(String),
}

impl DraftError {
    /// The line the card shows for this failure, in the managed wording.
    ///
    /// `AddCustomPaletteButton_Click` sets exactly one of these through
    /// `SetPaletteStatus` (`:1060-1064`) and returns without writing.
    #[must_use]
    pub fn status(&self) -> String {
        match self {
            Self::NameInvalid => copy::PALETTE_STATUS_NAME_INVALID.to_owned(),
            Self::NameBuiltin(name) => copy::PALETTE_STATUS_NAME_BUILTIN.replace("{name}", name),
            Self::ColorsInvalid => copy::PALETTE_STATUS_COLORS_INVALID.to_owned(),
            Self::Write(message) => {
                format!("{}{message}", copy::PALETTE_STATUS_SAVE_FAILED_PREFIX)
            }
        }
    }
}

/// What an Add-palette press did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftOutcome {
    /// The name is already in the file and the managed code asks before replacing it
    /// (`MainWindow.xaml.cs:1120-1127`). Nothing is written; the caller shows `prompt`
    /// under `title` and calls [`PaletteStore::save_draft`] again with `overwrite`.
    ConfirmOverwrite {
        /// The `MessageBox` body, with the name filled in.
        prompt: String,
        /// The `MessageBox` caption.
        title: &'static str,
    },
    /// The palette was written and applied.
    Saved {
        /// What was stored, with the five derived roles filled.
        palette: CustomPalette,
        /// `Saved and applied palette '<name>'.` (`:1170`).
        status: String,
    },
}

/// The palette file, the palette that is applied, and the editor's four hex drafts.
#[derive(Debug, Clone)]
pub struct PaletteStore {
    path: PathBuf,
    current: String,
    draft: PaletteDraft,
}

impl PaletteStore {
    /// The store in the platform's app-data directory.
    ///
    /// # Errors
    /// [`PlatformError`] when the platform names no app-data location, which is the
    /// honest answer: there is nowhere to put the file.
    pub fn from_system() -> Result<Self, PlatformError> {
        app_data_directory().map(|root| Self::at(root.join(APP_DIR).join(FILE_NAME)))
    }

    /// A store at an explicit path, for tests.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            current: DEFAULT_PALETTE.to_owned(),
            draft: PaletteDraft::default(),
        }
    }

    /// Where the palettes file lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The palette that is applied, as stored — the C# `Clicker.GuiTheme`.
    ///
    /// Kept verbatim rather than normalised, like the C# field: `ApplyGuiTheme`
    /// normalises on the way to paint, not on the way to the profile.
    #[must_use]
    pub fn current_name(&self) -> &str {
        &self.current
    }

    /// The card's editor state.
    #[must_use]
    pub fn draft(&self) -> &PaletteDraft {
        &self.draft
    }

    /// The editor state, for the five boxes' `on_input` handlers.
    pub fn draft_mut(&mut self) -> &mut PaletteDraft {
        &mut self.draft
    }

    /// Every saved palette, in file order.
    ///
    /// Best-effort, exactly as `PaletteStore.LoadAll` is (`CustomPalette.cs:60-80`): a
    /// missing file, a blank one and a corrupt one all read as no palettes, because a
    /// palette the user cannot load must not stop the window from opening.
    #[must_use]
    pub fn palettes(&self) -> Vec<CustomPalette> {
        read_palettes(&self.path)
    }

    /// The theme a palette name paints, or `Slate`.
    ///
    /// The order is `NormalizeThemeName`'s (`MainWindow.xaml.cs:1292-1306`): the name is
    /// trimmed, a built-in wins, then the file is searched case-insensitively, and
    /// anything else — including an unknown custom name — is `Slate`. The returned
    /// theme's `name` is the stored spelling, which is what the cards and the
    /// `Current palette:` line show.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Theme {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Theme::slate();
        }
        if is_builtin_ignoring_case(trimmed) {
            return Theme::by_name(trimmed);
        }
        match self
            .palettes()
            .iter()
            .find(|stored| same_name(&stored.name, trimmed))
        {
            Some(palette) => Theme::from_custom(&palette.name, roles_of(palette)),
            None => Theme::slate(),
        }
    }

    /// The theme the current palette paints, which is what the shell renders with.
    #[must_use]
    pub fn theme(&self) -> Theme {
        self.resolve(&self.current)
    }

    /// Makes a palette current, as a preview row's click does
    /// (`GuiPaletteButton_Click`, `MainWindow.xaml.cs:930-960`).
    ///
    /// The name is stored as given; [`PaletteStore::theme`] normalises when it paints.
    pub fn apply(&mut self, name: &str) {
        self.current = name.to_owned();
    }

    /// The Add-palette press.
    ///
    /// The managed order (`MainWindow.xaml.cs:1095-1171`), step for step: validate the
    /// name, refuse a built-in, parse the four hexes, ask before replacing an existing
    /// name, write, register, apply, clear the name box, report. `overwrite` is the
    /// answer to the question; a first press passes `false`.
    ///
    /// # Errors
    /// [`DraftError`] with the copy line the card shows; nothing is written.
    pub fn save_draft(&mut self, overwrite: bool) -> Result<DraftOutcome, DraftError> {
        let name = self.draft.name.trim().to_owned();
        if !is_valid_name(&name) {
            return Err(DraftError::NameInvalid);
        }
        if is_builtin_exact(&name) {
            return Err(DraftError::NameBuiltin(name));
        }
        let palette = self.draft.build(&name).ok_or(DraftError::ColorsInvalid)?;

        let mut palettes = self.palettes();
        if !overwrite && palettes.iter().any(|stored| same_name(&stored.name, &name)) {
            return Ok(DraftOutcome::ConfirmOverwrite {
                prompt: copy::PALETTE_OVERWRITE_PROMPT.replace("{name}", &name),
                title: copy::PALETTE_OVERWRITE_TITLE,
            });
        }

        upsert(&mut palettes, palette.clone());
        write_palettes(&self.path, &palettes).map_err(DraftError::Write)?;

        self.current = name.clone();
        self.draft.name.clear();
        Ok(DraftOutcome::Saved {
            palette,
            status: copy::PALETTE_STATUS_SAVED.replace("{name}", &name),
        })
    }

    /// The confirmation a delete press shows, before [`PaletteStore::delete`] runs.
    ///
    /// `Delete custom palette '<name>'?` under `Delete Palette`
    /// (`MainWindow.xaml.cs:1178-1181`).
    #[must_use]
    pub fn delete_confirmation(name: &str) -> (String, &'static str) {
        (
            copy::PALETTE_DELETE_PROMPT.replace("{name}", name),
            copy::PALETTE_DELETE_TITLE,
        )
    }

    /// Deletes a palette by name after the confirmation, returning the status line.
    ///
    /// Two managed behaviours are kept rather than tidied: a name that is not in the file
    /// is not an error (`Delete` is a `RemoveAll` followed by an unconditional status,
    /// `:1183-1209`), and deleting the applied palette falls back to `Slate`
    /// (`:1199-1206`).
    ///
    /// # Errors
    /// The `Could not delete palette: <reason>` line when the file cannot be written.
    pub fn delete(&mut self, name: &str) -> Result<String, String> {
        let mut palettes = self.palettes();
        delete(&mut palettes, name);
        write_palettes(&self.path, &palettes)
            .map_err(|message| format!("{}{message}", copy::PALETTE_STATUS_DELETE_FAILED_PREFIX))?;

        if same_name(&self.current, name) {
            self.current = DEFAULT_PALETTE.to_owned();
        }
        Ok(copy::PALETTE_STATUS_DELETED.replace("{name}", name))
    }
}

/// Reads the palette file, mapping every failure to an empty set.
fn read_palettes(path: &Path) -> Vec<CustomPalette> {
    match fs::read_to_string(path) {
        Ok(text) => from_palettes_json(&text),
        Err(_) => Vec::new(),
    }
}

/// Replaces the palette file, creating `<app data>/Aoko` on the first write.
fn write_palettes(path: &Path, palettes: &[CustomPalette]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, to_palettes_json(palettes)).map_err(|error| error.to_string())
}

/// `IsValidPaletteName` (`MainWindow.xaml.cs:1212-1224`).
///
/// `char.IsLetterOrDigit` is Unicode-aware on both sides, so `is_alphanumeric` is the
/// match. The length counts UTF-16 units because `string.Length` does: a name of astral
/// letters is shorter in code points than the managed validator sees it.
fn is_valid_name(name: &str) -> bool {
    if name.trim().is_empty() || name.encode_utf16().count() > MAX_NAME_LENGTH {
        return false;
    }
    name.chars()
        .all(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
}

/// Whether the name is a built-in, compared **case-sensitively**.
///
/// `BuiltInPaletteNames` is a `HashSet<string>` with the default comparer
/// (`MainWindow.xaml.cs:54-68`), so `Slate` is refused and `slate` is not. That asymmetry
/// is managed's and it has a visible consequence: a palette stored as `slate` can never
/// be applied, because resolution matches the built-ins first and case-insensitively.
/// The port keeps the guard exactly as it is; making it stricter would be a product
/// change, not a fix.
fn is_builtin_exact(name: &str) -> bool {
    Theme::builtin_names()
        .iter()
        .any(|builtin| *builtin == name)
}

/// Whether the name is a built-in under `OrdinalIgnoreCase`, the comparison
/// `NormalizeThemeName` and the resolution order use.
fn is_builtin_ignoring_case(name: &str) -> bool {
    Theme::builtin_names()
        .iter()
        .any(|builtin| same_name(*builtin, name))
}

/// `StringComparison.OrdinalIgnoreCase` (`MainWindow.xaml.cs:1105`, `:1298`).
///
/// Lowercasing both sides is the trick `aoko_protocol::profile` already uses for its
/// ordinal-ignore-case name sort, and unlike `eq_ignore_ascii_case` it also folds the
/// non-ASCII letters `char.IsLetterOrDigit` lets into a name.
fn same_name(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

/// `TryParseHex` (`MainWindow.xaml.cs:1227-1253`): `#RGB`, `#RRGGBB`, and either form
/// without the `#`.
///
/// Unlike [`Color::parse_hex`] this rejects the 8-digit ARGB form, because a palette file
/// stores opaque colours only and the managed parser rejects anything that is not six
/// digits after the shorthand expansion.
fn parse_entry(text: &str) -> Option<Color> {
    let digits = text.trim().trim_start_matches('#');
    let expanded: String = if digits.len() == 3 {
        digits.chars().flat_map(|digit| [digit, digit]).collect()
    } else {
        digits.to_owned()
    };
    if expanded.len() != 6 {
        return None;
    }
    Color::parse_hex(&expanded)
}

/// A stored palette's nine roles, borrowed for [`Theme::from_custom`].
fn roles_of(palette: &CustomPalette) -> CustomRoles<'_> {
    CustomRoles {
        background: &palette.background,
        panel: &palette.panel,
        slider_background: &palette.slider_background,
        slider_foreground: &palette.slider_foreground,
        accent: &palette.accent,
        text: &palette.text,
        dim_text: &palette.dim_text,
        tab_selected: &palette.tab_selected,
        tab_hover: &palette.tab_hover,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let index = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("aoko-palettes-{}-{index}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    fn store() -> PaletteStore {
        PaletteStore::at(scratch().join(APP_DIR).join(FILE_NAME))
    }

    #[test]
    fn a_first_run_has_no_palettes_and_resolves_slate() {
        let store = store();

        assert!(store.palettes().is_empty());
        assert_eq!(store.current_name(), DEFAULT_PALETTE);
        assert_eq!(store.theme().name, "Slate");
        assert_eq!(
            store.resolve("Mine").name,
            "Slate",
            "an unknown name is Slate"
        );
        assert!(!store.path().exists(), "reading must not create the file");
        assert_eq!(store.draft(), &PaletteDraft::default());
    }

    #[test]
    fn saving_a_draft_stores_the_four_roles_and_derives_the_other_five() {
        let mut store = store();
        store.draft_mut().name = "  Mine  ".to_owned();
        store.draft_mut().background = "#1a2b3c".to_owned();
        store.draft_mut().panel = "#202020".to_owned();
        store.draft_mut().accent = "#abc".to_owned();
        store.draft_mut().text = "E8EAEE".to_owned();

        let DraftOutcome::Saved { palette, status } =
            store.save_draft(false).expect("a new name saves")
        else {
            panic!("a new name does not need a confirmation");
        };

        assert_eq!(palette.name, "Mine", "the name is trimmed");
        assert_eq!(palette.background, "#1A2B3C", "written back as #RRGGBB");
        assert_eq!(palette.accent, "#AABBCC", "three digits expand");
        assert_eq!(palette.text, "#E8EAEE", "a missing # is accepted");
        let panel = Color::from_hex(0x20_20_20);
        assert_eq!(palette.slider_background, panel.lighten(0.06).to_hex());
        assert_eq!(palette.slider_foreground, panel.lighten(0.16).to_hex());
        assert_eq!(palette.tab_selected, panel.lighten(0.10).to_hex());
        assert_eq!(palette.tab_hover, panel.lighten(0.04).to_hex());
        assert_eq!(
            palette.dim_text,
            Color::from_hex(0xE8_EA_EE)
                .blend(Color::from_hex(0x1A_2B_3C), 0.45)
                .to_hex()
        );

        assert_eq!(store.current_name(), "Mine", "saving applies the palette");
        assert_eq!(status, "Saved and applied palette 'Mine'.");
        assert_eq!(store.draft().name, "", "only the name box is cleared");
        assert_eq!(
            store.draft().background,
            "#1a2b3c",
            "the hex drafts keep what was typed"
        );

        let text = fs::read_to_string(store.path()).expect("the file was written");
        assert!(
            text.contains("\"sliderBackground\""),
            "the managed key shape (`CustomPalette.cs:41-44`): {text}"
        );
        assert_eq!(store.palettes(), vec![palette]);
    }

    #[test]
    fn a_bad_name_a_builtin_name_and_a_bad_hex_are_refused_with_the_managed_copy() {
        let mut store = store();

        store.draft_mut().name = "   ".to_owned();
        assert_eq!(store.save_draft(false), Err(DraftError::NameInvalid));
        assert_eq!(
            DraftError::NameInvalid.status(),
            "Enter a palette name (letters, numbers, spaces, - or _; max 32)."
        );

        store.draft_mut().name = "Mi/ne".to_owned();
        assert_eq!(store.save_draft(false), Err(DraftError::NameInvalid));

        store.draft_mut().name = "Slate".to_owned();
        let refused = store.save_draft(false).expect_err("a built-in is refused");
        assert_eq!(refused, DraftError::NameBuiltin("Slate".to_owned()));
        assert_eq!(
            refused.status(),
            "'Slate' is a built-in palette name. Choose another."
        );

        store.draft_mut().name = "Mine".to_owned();
        store.draft_mut().accent = "#12345".to_owned();
        assert_eq!(store.save_draft(false), Err(DraftError::ColorsInvalid));
        assert_eq!(
            DraftError::ColorsInvalid.status(),
            "One or more colors are invalid. Use hex like #1A2B3C."
        );

        store.draft_mut().accent = "#FF1A2B3C".to_owned();
        assert_eq!(
            store.save_draft(false),
            Err(DraftError::ColorsInvalid),
            "the 8-digit ARGB form a hex entry accepts is not a palette colour"
        );

        assert!(!store.path().exists(), "a refused press writes nothing");
        assert_eq!(store.current_name(), DEFAULT_PALETTE);
    }

    #[test]
    fn an_existing_name_asks_first_and_only_writes_when_told_to() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.draft_mut().accent = "#111111".to_owned();
        store.save_draft(false).expect("first save");

        store.draft_mut().name = "Mine".to_owned();
        store.draft_mut().accent = "#222222".to_owned();
        match store.save_draft(false).expect("the name exists") {
            DraftOutcome::ConfirmOverwrite { prompt, title } => {
                assert_eq!(
                    prompt,
                    "A custom palette named 'Mine' already exists. Overwrite it?"
                );
                assert_eq!(title, "Overwrite Palette");
            }
            other => panic!("expected a confirmation, got {other:?}"),
        }
        assert_eq!(
            store.palettes()[0].accent,
            "#111111",
            "nothing is written before the answer"
        );
        assert_eq!(
            store.current_name(),
            "Mine",
            "the first save is still applied"
        );

        let DraftOutcome::Saved { status, .. } = store.save_draft(true).expect("overwrite") else {
            panic!("the confirmed press replaces without asking again");
        };
        assert_eq!(status, "Saved and applied palette 'Mine'.");
        assert_eq!(
            store.palettes().len(),
            1,
            "upsert replaces, it does not append"
        );
        assert_eq!(store.palettes()[0].accent, "#222222");
    }

    #[test]
    fn a_custom_palette_resolves_by_name_and_a_builtin_wins_a_case_tie() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.draft_mut().background = "#1A2B3C".to_owned();
        store.save_draft(false).expect("save");

        let theme = store.resolve("mine");
        assert_eq!(theme.name, "Mine", "the stored spelling comes back");
        assert_eq!(theme.background, Color::from_hex(0x1A_2B_3C));
        assert_eq!(store.theme().name, "Mine");
        assert_eq!(
            store.resolve("  MINE  ").name,
            "Mine",
            "trimmed, like NormalizeThemeName"
        );

        // The managed wart, kept and pinned: the built-in guard is case-sensitive, so
        // `slate` is storable, and resolution matches the built-in first.
        store.draft_mut().name = "slate".to_owned();
        store.draft_mut().background = "#1A2B3C".to_owned();
        store
            .save_draft(false)
            .expect("the guard lets a lowercase built-in through, as managed does");
        assert_eq!(store.palettes().len(), 2);
        assert_eq!(store.resolve("slate").name, "Slate");
        assert_ne!(
            store.resolve("slate").background,
            Color::from_hex(0x1A_2B_3C),
            "the stored palette is unreachable, which is what managed does too"
        );
    }

    #[test]
    fn deleting_the_applied_palette_falls_back_to_slate() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.save_draft(false).expect("save");
        assert_eq!(store.current_name(), "Mine");

        let (prompt, title) = PaletteStore::delete_confirmation("Mine");
        assert_eq!(prompt, "Delete custom palette 'Mine'?");
        assert_eq!(title, "Delete Palette");

        assert_eq!(
            store.delete("Mine").expect("deleted"),
            "Deleted palette 'Mine'."
        );
        assert!(store.palettes().is_empty());
        assert_eq!(
            store.current_name(),
            DEFAULT_PALETTE,
            "the deleted palette was applied"
        );
        assert_eq!(store.theme().name, "Slate");

        assert_eq!(
            store
                .delete("Gone")
                .expect("a missing name is not an error"),
            "Deleted palette 'Gone'.",
            "managed reports the deletion unconditionally"
        );
    }

    #[test]
    fn a_corrupt_file_reads_as_no_palettes() {
        let store = store();
        fs::create_dir_all(store.path().parent().expect("parent")).expect("dir");
        fs::write(store.path(), "not json").expect("write");

        assert!(store.palettes().is_empty(), "the loader is best-effort");
        assert_eq!(store.resolve("Mine").name, "Slate");
        assert_eq!(store.theme().name, "Slate");
    }

    #[test]
    fn the_name_rule_is_the_managed_one() {
        assert!(is_valid_name("Lime Water"));
        assert!(is_valid_name("Mine_2-b"));
        assert!(same_name("Slate", "slate"));
        assert_eq!("x".repeat(MAX_NAME_LENGTH).encode_utf16().count(), 32);
        assert!(is_valid_name(&"x".repeat(MAX_NAME_LENGTH)));
        assert!(!is_valid_name(&"x".repeat(MAX_NAME_LENGTH + 1)));
        assert!(!is_valid_name("Mi/ne"));
        assert!(!is_valid_name("   "));
        assert!(!is_valid_name("\t"));
    }
}
