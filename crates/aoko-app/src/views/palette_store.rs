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
//! The four colour boxes are painted: the shell builds a `Control::ColorField` per bind and its
//! row reads and writes the draft through [`PaletteStore::get`] and [`PaletteStore::set`], so a
//! typed hex and the swatch beside it move the one string the Add press judges, and a colour the
//! picker commits later will land there too. The rest of the editor card — the name box, the Add
//! and Delete presses, the saved-palette list and the current-name row — is still shell-owned
//! control work that does not exist yet. This module deliberately imports no GPUI type.
//!
//! # Deviations from managed
//!
//! Three behaviours here are **not** ports, all because the managed behaviour is a trap
//! rather than a quirk. Each is pinned by a test that fails if the managed behaviour is
//! restored, and all are recorded in the migration's trade-offs section.
//!
//! * **The built-in name guard is case-insensitive.** `BuiltInPaletteNames.Contains`
//!   (`MainWindow.xaml.cs:1105`) is ordinal, so managed accepted a palette named `slate`;
//!   resolution is case-insensitive and matches the built-ins first, so the saved palette
//!   could never be applied. A palette that is saved and unreachable is worse than a
//!   refused name, so [`PaletteStore::save_draft`] refuses every casing and reuses the
//!   managed sentence ([`copy::PALETTE_STATUS_NAME_BUILTIN`]). Resolution keeps the
//!   managed order — a hand-edited file can still contain such a name, and it still
//!   resolves the managed way.
//! * **A delete of a name that is not in the file reports that.** Managed's `Delete` is a
//!   `RemoveAll` followed by an unconditional `Deleted palette '<name>'.`
//!   (`MainWindow.xaml.cs:1183-1209`), so it removed nothing and said it had. Reporting
//!   success for a failure is the defect class this migration exists to remove, so
//!   [`PaletteStore::delete`] refuses with [`DeleteError::NotFound`] and writes nothing.
//! * **A saved card's swatches are what applying the palette paints.** Managed built them with
//!   `ParseHexOr(palette.Panel, Colors.Black)` and three siblings
//!   (`MainWindow.xaml.cs:1052-1055`), so a hand-edited role painted black on the card while
//!   the apply fell back to a derived shade. [`PaletteStore::rows`] resolves through
//!   [`Theme::from_custom`] — the chain `RegisterPalette` uses — so the preview cannot
//!   disagree with the click.

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

/// One of the four roles a palette colour box edits.
///
/// The enum exists so the box names are paired with the draft's fields in exactly one place,
/// [`role`]. [`PaletteStore::get`] and [`PaletteStore::set`] then match the variants
/// exhaustively, so adding a role breaks both at compile time instead of letting the read and
/// the write drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Background,
    Panel,
    Accent,
    Text,
}

/// The role one palette box edits, by managed box name.
///
/// The four names are the boxes' own `x:Name`s — `PaletteBgBox` (`MainWindow.xaml:1963`),
/// `PalettePanelBox` (`:1974`), `PaletteAccentBox` (`:1985`) and `PaletteTextBox` (`:1996`) —
/// which are also the bind strings a `Control::ColorField` is built with, so a caller passes the
/// bind it already holds and keeps no pairing of its own. `PaletteDraft::build`'s four `parse_entry`
/// calls read the same fields, so the box, the swatch and the Add press all agree on which string
/// is which role.
fn role(bind: &str) -> Option<Role> {
    match bind {
        "PaletteBgBox" => Some(Role::Background),
        "PalettePanelBox" => Some(Role::Panel),
        "PaletteAccentBox" => Some(Role::Accent),
        "PaletteTextBox" => Some(Role::Text),
        _ => None,
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
///
/// `Saved` holds its palette behind a `Box` on purpose: [`CustomPalette`] is ten `String`s, so
/// inline it made the whole enum 264 bytes wide and every `save_draft` return carried that width
/// — including the refusal, which stores nothing. Boxing is what clippy's `large_enum_variant`
/// asks for; an `allow` would have hidden the size rather than moved it.
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
        palette: Box<CustomPalette>,
        /// `Saved and applied palette '<name>'.` (`:1170`).
        status: String,
    },
}

/// One saved palette as its card paints it: the name and the four swatches
/// (`MainWindow.xaml:1917-1935`, the `DataTemplate` of `CustomPalettesItems`).
///
/// The four colours are resolved through [`Theme::from_custom`], which is the same fallback
/// chain `RegisterPalette` (`MainWindow.xaml.cs:1004-1042`) uses when the palette is
/// applied, so the card previews what a click on it would paint. Managed built the same four
/// swatches from `ParseHexOr(..., Colors.Black)` / `ParseHexOr(..., Colors.Gray)`
/// (`:1052-1055`): a stored role that does not parse painted black, so a palette with a
/// corrupt `panel` previewed a colour it would never paint. See the module header, and
/// `a_corrupt_role_previews_what_applying_would_paint`, which fails if the black fallback
/// returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteRow {
    /// The stored spelling, which is what the `Current palette:` line shows.
    pub name: String,
    /// `BackgroundSwatch` (`:1049`).
    pub background: Color,
    /// `PanelSwatch` (`:1050`).
    pub panel: Color,
    /// `SliderSwatch` (`:1051`), from the palette's `slider_background`.
    pub slider: Color,
    /// `AccentSwatch` (`:1052`).
    pub accent: Color,
}

/// What a delete press did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOutcome {
    /// `Deleted palette '<name>'.` (`MainWindow.xaml.cs:1209`).
    pub status: String,
    /// The palette that replaced the applied one when the removed palette was applied —
    /// `Slate` (`:1202-1207`) — and `None` when what is applied did not move.
    ///
    /// The caller persists it to the profile's `GuiTheme`, because the profile is the file the
    /// applied name lives in; [`PaletteStore::current_name`] already reflects the same name.
    pub fallback: Option<&'static str>,
}

/// Why a delete press removed nothing.
///
/// A distinct type from [`DraftError`] because the two failures a delete can hit are not
/// the ones a save can: a missing name, which managed did not treat as a failure at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteError {
    /// No palette in the file has that name, so nothing was removed. Carries the name as
    /// given.
    ///
    /// **New copy.** Managed's `RemoveAll` deleted nothing and then reported
    /// `Deleted palette '<name>'.` (`MainWindow.xaml.cs:1183-1209`); see the module
    /// header for why the port does not.
    NotFound(String),
    /// The file could not be written (`:1188-1194`), carrying the I/O message.
    Write(String),
}

impl DeleteError {
    /// The line the card shows for this failure.
    #[must_use]
    pub fn status(&self) -> String {
        match self {
            Self::NotFound(name) => copy::PALETTE_STATUS_DELETE_MISSING.replace("{name}", name),
            Self::Write(message) => {
                format!("{}{message}", copy::PALETTE_STATUS_DELETE_FAILED_PREFIX)
            }
        }
    }
}

/// The palette file, the palette that is applied, and the editor's four hex drafts.
#[derive(Debug, Clone)]
pub struct PaletteStore {
    path: PathBuf,
    current: String,
    draft: PaletteDraft,
    /// The row a remove press is waiting to confirm, `Profiles::delete_armed`'s shape.
    ///
    /// The managed app asked in a `MessageBox` (`MainWindow.xaml.cs:1178-1181`); the card asks in
    /// place, so the armed name is state — which is also what makes the confirmation testable.
    delete_armed: Option<String>,
    /// The name a save press would replace, waiting for its own confirmation
    /// (`MainWindow.xaml.cs:1120-1127`), armed by [`PaletteStore::save_draft`] itself.
    overwrite_pending: Option<String>,
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
            // Nothing is armed until a press asks a question.
            delete_armed: None,
            overwrite_pending: None,
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

    /// The drafted text one palette colour box holds, by managed box name.
    ///
    /// The same four names `role` pairs, and the same strings `PaletteDraft::build` reads when
    /// [`PaletteStore::save_draft`] runs the Add press, so a row that paints what this returns
    /// shows the text the press will judge. Anything else answers `None`: a bind that is not one
    /// of the four boxes has no draft field, and guessing a role would paint one row's colour on
    /// another.
    #[must_use]
    pub fn get(&self, bind: &str) -> Option<&str> {
        let draft = &self.draft;
        Some(match role(bind)? {
            Role::Background => draft.background.as_str(),
            Role::Panel => draft.panel.as_str(),
            Role::Accent => draft.accent.as_str(),
            Role::Text => draft.text.as_str(),
        })
    }

    /// Writes one palette role back into the draft, by the names [`PaletteStore::get`] reads.
    ///
    /// The text is stored as given, even when it does not parse: [`PaletteDraft`] deliberately
    /// keeps an invalid entry visible so it can be fixed, and [`PaletteStore::save_draft`] is
    /// where it is refused. An unknown bind writes nothing rather than panicking — the caller
    /// painting a row for it has no text to start from either.
    pub fn set(&mut self, bind: &str, text: impl Into<String>) {
        let text = text.into();
        let field = match role(bind) {
            Some(Role::Background) => &mut self.draft.background,
            Some(Role::Panel) => &mut self.draft.panel,
            Some(Role::Accent) => &mut self.draft.accent,
            Some(Role::Text) => &mut self.draft.text,
            None => return,
        };

        if field.as_str() != text.as_str() {
            *field = text;
        }
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

    /// The theme a palette name paints out of this store's file, or `Slate`.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Theme {
        resolve_in(&self.palettes(), name)
    }

    /// The saved palettes as their cards paint them, in file order.
    ///
    /// One row per palette rather than a row per role, so the card holds no slice of names and
    /// no second parse of the four stored hexes: this is the list's shape, and
    /// [`PaletteRow`] explains why its colours come from the theme the apply builds.
    #[must_use]
    pub fn rows(&self) -> Vec<PaletteRow> {
        self.palettes()
            .into_iter()
            .map(|palette| {
                let theme = Theme::from_custom(&palette.name, roles_of(&palette));
                PaletteRow {
                    name: theme.name,
                    background: theme.background,
                    panel: theme.panel,
                    slider: theme.slider_background,
                    accent: theme.accent,
                }
            })
            .collect()
    }

    /// The theme the current palette paints, which is what the shell renders with.
    #[must_use]
    pub fn theme(&self) -> Theme {
        self.resolve(&self.current)
    }

    /// Makes a palette current, as a preview row's click does
    /// (`GuiPaletteButton_Click`, `MainWindow.xaml.cs:930-960`).
    ///
    /// The name is stored as given; [`PaletteStore::theme`] normalises when it paints. This is
    /// the store half only. Managed sets `clicker.GuiTheme` and then calls `ApplyGuiTheme`, and
    /// the profile is the file that name lives in, so the caller persists it here too; a caller
    /// that does not leaves the store and the profile disagreeing at the next load.
    pub fn apply(&mut self, name: &str) {
        self.current = name.to_owned();
    }

    /// The row a remove press is waiting to confirm, if any.
    #[must_use]
    pub fn delete_armed(&self) -> Option<&str> {
        self.delete_armed.as_deref()
    }

    /// Arms the remove confirmation for one row's cap.
    pub fn request_delete(&mut self, name: &str) {
        self.delete_armed = Some(name.to_owned());
    }

    /// Disarms the confirmation without deleting: the file is untouched.
    pub fn cancel_delete(&mut self) {
        self.delete_armed = None;
    }

    /// The confirmation's own Delete.
    ///
    /// `None` when no cap was pressed. The card cannot reach that — it paints the confirmation only
    /// while one is armed — and it is deliberately not an error, because a refusal would have to
    /// name a palette in a status line for a press that never happened.
    pub fn confirm_delete(&mut self) -> Option<Result<DeleteOutcome, DeleteError>> {
        let name = self.delete_armed.take()?;
        Some(self.delete(&name))
    }

    /// The name a save press would replace, if one is waiting for its own confirmation.
    #[must_use]
    pub fn overwrite_pending(&self) -> Option<&str> {
        self.overwrite_pending.as_deref()
    }

    /// Forgets a pending overwrite, for Cancel on the in-card prompt. Nothing is written.
    pub fn cancel_overwrite(&mut self) {
        self.overwrite_pending = None;
    }

    /// The overwrite prompt's confirm: the `MessageBox` answer, in place
    /// (`MainWindow.xaml.cs:1120-1127`).
    ///
    /// # Errors
    /// As [`PaletteStore::save_draft`].
    pub fn confirm_overwrite(&mut self) -> Result<DraftOutcome, DraftError> {
        self.save_draft(true)
    }

    /// The Add-palette press.
    ///
    /// The managed order (`MainWindow.xaml.cs:1095-1171`), step for step: validate the
    /// name, refuse a built-in, parse the four hexes, ask before replacing an existing
    /// name, write, register, apply, clear the name box, report. `overwrite` is the
    /// answer to the question; a first press passes `false`.
    ///
    /// One step is deliberately stricter than the managed one: the built-in refusal is
    /// case-insensitive, so `slate` never reaches the file (see the module header).
    ///
    /// # Errors
    /// [`DraftError`] with the copy line the card shows; nothing is written.
    pub fn save_draft(&mut self, overwrite: bool) -> Result<DraftOutcome, DraftError> {
        let name = self.draft.name.trim().to_owned();
        if !is_valid_name(&name) {
            return Err(DraftError::NameInvalid);
        }
        if is_builtin_ignoring_case(&name) {
            return Err(DraftError::NameBuiltin(name));
        }
        let palette = self.draft.build(&name).ok_or(DraftError::ColorsInvalid)?;

        let mut palettes = self.palettes();
        if !overwrite && palettes.iter().any(|stored| same_name(&stored.name, &name)) {
            // Stored as well as returned: the card paints the prompt from this field, which is what
            // lets Cancel forget the question without the caller keeping a second copy of the name.
            self.overwrite_pending = Some(name.clone());
            return Ok(DraftOutcome::ConfirmOverwrite {
                prompt: copy::PALETTE_OVERWRITE_PROMPT.replace("{name}", &name),
                title: copy::PALETTE_OVERWRITE_TITLE,
            });
        }
        self.overwrite_pending = None;

        upsert(&mut palettes, palette.clone());
        write_palettes(&self.path, &palettes).map_err(DraftError::Write)?;

        self.current = name.clone();
        self.draft.name.clear();
        Ok(DraftOutcome::Saved {
            palette: Box::new(palette),
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

    /// Deletes a palette by name after the confirmation.
    ///
    /// Deleting the applied palette falls back to `Slate` (`MainWindow.xaml.cs:1202-1207`), so
    /// the outcome names that fallback and the caller writes it to the profile's `GuiTheme`;
    /// returning only the status line would leave the profile pointing at a palette that is no
    /// longer in the file. A name that is not in the file is **not** reported as a deletion:
    /// managed ran a `RemoveAll` and then printed `Deleted palette '<name>'.` regardless
    /// (`:1183-1209`), which claimed a success that did not happen. The port returns
    /// [`DeleteError::NotFound`] and writes nothing. See the module header.
    ///
    /// # Errors
    /// [`DeleteError::NotFound`] for a name the file does not contain, or
    /// [`DeleteError::Write`] when the file cannot be written; nothing is written in
    /// either case.
    pub fn delete(&mut self, name: &str) -> Result<DeleteOutcome, DeleteError> {
        let mut palettes = self.palettes();
        if !palettes.iter().any(|stored| same_name(&stored.name, name)) {
            return Err(DeleteError::NotFound(name.to_owned()));
        }
        delete(&mut palettes, name);
        write_palettes(&self.path, &palettes).map_err(DeleteError::Write)?;

        let fallback = if same_name(&self.current, name) {
            self.current = DEFAULT_PALETTE.to_owned();
            Some(DEFAULT_PALETTE)
        } else {
            None
        };
        Ok(DeleteOutcome {
            status: copy::PALETTE_STATUS_DELETED.replace("{name}", name),
            fallback,
        })
    }
}

/// The theme a palette name paints out of a given palette set, or `Slate`.
///
/// The order is `NormalizeThemeName`'s (`MainWindow.xaml.cs:1292-1306`): the name is
/// trimmed, a built-in wins, then the set is searched case-insensitively, and anything
/// else — including an unknown custom name — is `Slate`. The returned theme's `name` is
/// the stored spelling, which is what the cards and the `Current palette:` line show.
///
/// Takes the set rather than reading it, so the rule is testable without a file; the
/// shell's entry point is [`theme_for`].
#[must_use]
pub fn resolve_in(palettes: &[CustomPalette], name: &str) -> Theme {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Theme::slate();
    }
    if is_builtin_ignoring_case(trimmed) {
        return Theme::by_name(trimmed);
    }
    match palettes
        .iter()
        .find(|stored| same_name(&stored.name, trimmed))
    {
        Some(palette) => Theme::from_custom(&palette.name, roles_of(palette)),
        None => Theme::slate(),
    }
}

/// The theme a profile's `GuiTheme` name paints, reading the palette file.
///
/// This is the one call that replaces `Theme::by_name(&profile.gui_theme)` in the shell,
/// because `by_name` knows the 18 built-ins and cannot see a custom name. A platform
/// that names no app-data directory falls back to the built-in table, which is the same
/// answer `by_name` gives.
#[must_use]
pub fn theme_for(name: &str) -> Theme {
    match PaletteStore::from_system() {
        Ok(store) => store.resolve(name),
        Err(_) => Theme::by_name(name),
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

/// Whether the name is a built-in, under `OrdinalIgnoreCase`.
///
/// Used by both the save-time guard and the resolution order, which is the deviation: the
/// managed guard is `BuiltInPaletteNames.Contains` with the default comparer
/// (`MainWindow.xaml.cs:54-68`, `:1105`) and is therefore ordinal, so managed accepted a
/// palette named `slate` that its own case-insensitive resolution could then never reach.
/// Refusing every casing at save time is the fix; the module header records it. Resolution
/// still matches the built-ins first, because a hand-edited file can contain such a name.
fn is_builtin_ignoring_case(name: &str) -> bool {
    Theme::builtin_names()
        .iter()
        .any(|builtin| same_name(builtin, name))
}

/// `StringComparison.OrdinalIgnoreCase` (`MainWindow.xaml.cs:1105`, `:1298`).
///
/// Lowercasing both sides is the trick `aoko_protocol::profile` already uses for its
/// ordinal-ignore-case name sort, and unlike `eq_ignore_ascii_case` it also folds the
/// non-ASCII letters `char.IsLetterOrDigit` lets into a name.
fn same_name(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

/// `TryParseHex` (`MainWindow.xaml.cs:1227-1253`) for a palette entry: `#RGB`, `#RRGGBB`,
/// either form without the `#`, any case, and nothing else.
///
/// The rule itself is [`Color::parse_rgb_hex`] and lives there, so the text a box accepts,
/// the text [`Theme::from_custom`] accepts out of the stored file and the text the swatch
/// accepts are one implementation — a change to the tolerance cannot land on one path only.
/// The 8-digit ARGB form is **refused** because a palette file stores opaque `#RRGGBB` only
/// and `TryParseHex` refuses anything that is not six digits once the shorthand is expanded;
/// the guard is digit-shaped on purpose, since the alpha in `#FFAABBCC` is opaque and it is
/// still not a palette entry.
///
/// Public because a consumer outside this store has to accept exactly what
/// [`PaletteStore::save_draft`] accepts. The colour swatch in the shell's
/// `Control::ColorField` parses the entry it is handed with this function, so a single role
/// the picker shows as valid cannot then be the reason the save is refused. The alternative
/// is a second copy of the rule in `shell.rs`, which is free to disagree with this one — the
/// defect where a swatch paints a colour the press rejects.
#[must_use]
pub fn parse_entry(text: &str) -> Option<Color> {
    Color::parse_rgb_hex(text)
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
        assert_eq!(store.palettes(), vec![*palette]);
    }

    /// The five derived roles are the managed `double` maths, ties included.
    ///
    /// `AddCustomPaletteButton_Click` (`MainWindow.xaml.cs:1128-1143`) derives them from the
    /// four typed colours, and `RegisterPalette` (`:1013-1016`) re-derives the same five when a
    /// stored palette has lost one — so a wrong step here is both a save and an apply that
    /// disagree with the managed app. A panel whose channels land on a half is where the two
    /// can differ: the amounts are `double`s, so `225 × 0.06` is exactly 13.5 and
    /// `Math.Round` takes 43.5 to the even 44, while the same amount widened from an `f32`
    /// gives 43. The expectations are literals rather than `panel.lighten(..)` on both
    /// sides, so a rounding change fails here instead of agreeing with itself.
    #[test]
    fn the_derived_roles_are_the_managed_double_maths() {
        let draft = PaletteDraft {
            name: "Pastel-ish".to_owned(),
            background: "#0D0A10".to_owned(),
            panel: "#18131E".to_owned(),
            accent: "#F39BB2".to_owned(),
            text: "#FBF3F6".to_owned(),
        };

        let palette = draft.build("Pastel-ish").expect("four valid hexes");

        assert_eq!(palette.slider_background, "#26212C", "225 × 0.06 = 13.5");
        assert_eq!(palette.slider_foreground, "#3D3942", "225 × 0.16 = 36");
        assert_eq!(palette.tab_selected, "#2F2B34", "225 × 0.10 = 22.5");
        assert_eq!(palette.tab_hover, "#211C27", "225 × 0.04 = 9");
        // `230 × 0.45` is exactly 103.5, so 246 - 103.5 = 142.5 goes to the even 142.
        assert_eq!(palette.dim_text, "#908A8E");
        assert_eq!(palette.background, "#0D0A10", "kept as typed");
        assert_eq!(palette.panel, "#18131E");
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

    /// One row's text: `set` writes the draft and `get` reads it back, so a shell row holds no
    /// copy of the four names and cannot show a different hex from the one the Add press judges.
    #[test]
    fn a_written_role_reads_back_through_get() {
        let mut store = store();

        store.set("PaletteBgBox", "#1a2b3c");
        store.set("PalettePanelBox", "#202020");
        store.set("PaletteAccentBox", "#abc");
        store.set("PaletteTextBox", String::from("E8EAEE"));

        assert_eq!(store.get("PaletteBgBox"), Some("#1a2b3c"));
        assert_eq!(store.get("PalettePanelBox"), Some("#202020"));
        assert_eq!(store.get("PaletteAccentBox"), Some("#abc"));
        assert_eq!(
            store.get("PaletteTextBox"),
            Some("E8EAEE"),
            "`set` takes a String as readily as a &str"
        );
        assert_eq!(
            store.draft().background,
            "#1a2b3c",
            "the draft is the owner, not a copy beside it"
        );
        assert_eq!(
            store.get("PaletteBgBox"),
            Some(store.draft().background.as_str())
        );
    }

    #[test]
    fn the_save_path_stores_what_set_wrote() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.set("PaletteBgBox", "#1A2B3C");
        store.set("PalettePanelBox", "#202020");
        store.set("PaletteAccentBox", "#abc");
        store.set("PaletteTextBox", "#E8EAEE");

        let DraftOutcome::Saved { palette, .. } =
            store.save_draft(false).expect("a new name saves")
        else {
            panic!("a new name does not need a confirmation");
        };

        assert_eq!(palette.background, "#1A2B3C");
        assert_eq!(palette.panel, "#202020");
        assert_eq!(palette.accent, "#AABBCC", "the shorthand expands as ever");
        assert_eq!(palette.text, "#E8EAEE");
        assert_eq!(
            store.get("PaletteBgBox"),
            Some("#1A2B3C"),
            "saving does not rewrite the drafts it read"
        );
    }

    #[test]
    fn an_unknown_bind_is_a_no_op_rather_than_a_panic() {
        let mut store = store();
        let before = store.draft().clone();

        assert_eq!(store.get("PaletteBorderBox"), None);
        assert_eq!(store.get(""), None);
        assert_eq!(store.get("palettebgbox"), None, "the names are exact");

        store.set("PaletteBorderBox", "#FF0000");
        store.set("", "#FF0000");

        assert_eq!(store.draft(), &before, "an unknown bind writes nothing");
    }

    /// `TryParseHex` removes **one** leading `#` (`MainWindow.xaml.cs:1233-1234`), so a
    /// second one is the typo it refuses. Accepting `##abc` would make the same text mean a
    /// colour here and nothing on the managed side.
    #[test]
    fn exactly_one_leading_hash_is_stripped() {
        assert_eq!(parse_entry("#abc"), Some(Color::from_hex(0xAA_BB_CC)));
        assert_eq!(parse_entry("#1A2B3C"), Some(Color::from_hex(0x1A_2B_3C)));
        assert_eq!(
            parse_entry("1A2B3C"),
            Some(Color::from_hex(0x1A_2B_3C)),
            "the # is optional"
        );
        assert_eq!(parse_entry("  #abc  "), Some(Color::from_hex(0xAA_BB_CC)));

        assert_eq!(parse_entry("##abc"), None, "the second # is a typo");
        assert_eq!(parse_entry("##123456"), None);
        assert_eq!(parse_entry(" ###123456 "), None);

        // The rest of the managed tolerance is unchanged: five digits, a bare `#` and the
        // ARGB form are all refused.
        assert_eq!(parse_entry("#12345"), None, "five digits");
        assert_eq!(parse_entry(""), None);
        assert_eq!(parse_entry("#"), None);
        assert_eq!(
            parse_entry("#FF1A2B3C"),
            None,
            "the ARGB form is not a palette colour"
        );
    }

    /// The property is *agreement*: the rule lives once in `theme`, so this box and the strict
    /// parser cannot disagree at all — and the strict parser differs from the permissive
    /// `parse_hex` on exactly one thing, the alpha form a palette entry must not carry.
    #[test]
    fn the_entry_parser_agrees_with_the_theme_parser_except_on_argb() {
        for text in ["#abc", "#AABBCC", "abc", "  #AABBCC  "] {
            assert_eq!(
                parse_entry(text),
                Color::parse_rgb_hex(text),
                "{text:?} is the same function, not a copy of it"
            );
            assert_eq!(
                parse_entry(text),
                Color::parse_hex(text),
                "{text:?} must parse the same through both"
            );
        }

        assert_eq!(parse_entry("##abc"), None);
        assert_eq!(
            Color::parse_hex("##abc"),
            None,
            "the second # is refused by both, so neither path repairs the typo"
        );

        assert_eq!(
            parse_entry("#FF1A2B3C"),
            None,
            "the one difference: a palette entry has no alpha channel"
        );
        assert_eq!(
            Color::parse_hex("#FF1A2B3C"),
            Some(Color::rgba(0x1A, 0x2B, 0x3C, 0xFF)),
            "which the permissive parser accepts — the digit count is the whole difference"
        );
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
    }

    /// Deviation, pinned: the save-time guard is case-insensitive.
    ///
    /// Managed's `BuiltInPaletteNames.Contains` (`MainWindow.xaml.cs:1105`) is ordinal, so
    /// it accepted `slate`; resolution is case-insensitive and matches the built-ins first,
    /// so that saved palette could never apply. If someone restores the ordinal guard this
    /// test will save a palette and then fail on the first refusal assertion.
    #[test]
    fn a_builtin_name_is_refused_at_save_time_in_every_casing() {
        for name in ["Slate", "slate", "SLATE", "  sLaTe  "] {
            let mut store = store();
            store.draft_mut().name = name.to_owned();

            let refused = store
                .save_draft(false)
                .expect_err(&format!("{name:?} is a built-in and must not save"));
            assert_eq!(
                refused,
                DraftError::NameBuiltin(name.trim().to_owned()),
                "the message names the palette as typed, trimmed"
            );
            assert_eq!(
                refused.status(),
                format!(
                    "'{}' is a built-in palette name. Choose another.",
                    name.trim()
                )
            );
            assert!(
                !store.path().exists(),
                "{name:?} must not reach the file: a saved palette that cannot apply is worse than a refusal"
            );
            assert_eq!(store.current_name(), DEFAULT_PALETTE);
        }
    }

    #[test]
    fn resolving_out_of_a_set_keeps_the_documented_order_without_a_file() {
        let mut palettes = vec![CustomPalette {
            name: "Mine".to_owned(),
            background: "#1A2B3C".to_owned(),
            ..CustomPalette::default()
        }];

        let custom = resolve_in(&palettes, "mine");
        assert_eq!(custom.name, "Mine", "the stored spelling, was `mine`");
        assert_eq!(custom.background, Color::from_hex(0x1A_2B_3C));
        assert_eq!(resolve_in(&palettes, "Slate").name, "Slate");
        assert_eq!(resolve_in(&palettes, "").name, "Slate", "blank is Slate");
        assert_eq!(resolve_in(&palettes, "   ").name, "Slate");
        assert_eq!(resolve_in(&palettes, "Gone").name, "Slate");

        // A file edited by hand can still hold a palette that shadows a built-in, and
        // resolution keeps the managed order: the built-ins are matched first, and
        // case-insensitively. This is why the save-time guard refuses the name outright.
        palettes.push(CustomPalette {
            name: "slate".to_owned(),
            background: "#1A2B3C".to_owned(),
            ..CustomPalette::default()
        });
        let shadowed = resolve_in(&palettes, "slate");
        assert_eq!(shadowed.name, "Slate");
        assert_ne!(shadowed.background, Color::from_hex(0x1A_2B_3C));
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

        let deleted = store.delete("Mine").expect("deleted");
        assert_eq!(deleted.status, "Deleted palette 'Mine'.");
        assert_eq!(
            deleted.fallback,
            Some(DEFAULT_PALETTE),
            "the applied palette moved, so the caller is told to persist Slate"
        );
        assert!(store.palettes().is_empty());
        assert_eq!(
            store.current_name(),
            DEFAULT_PALETTE,
            "the deleted palette was applied"
        );
        assert_eq!(store.theme().name, "Slate");
    }

    /// Deviation, pinned: a delete of a name that is not saved reports the miss.
    ///
    /// Managed ran a `RemoveAll` that removed nothing and then printed
    /// `Deleted palette '<name>'.` (`MainWindow.xaml.cs:1183-1209`). If someone restores
    /// that, the first assertion fails because the press reports success again.
    #[test]
    fn deleting_a_name_that_is_not_saved_reports_it_and_writes_nothing() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.save_draft(false).expect("save");
        let before = fs::read_to_string(store.path()).expect("the file exists");

        let refused = store.delete("Gone").expect_err("nothing was deleted");
        assert_eq!(refused, DeleteError::NotFound("Gone".to_owned()));
        assert_eq!(
            refused.status(),
            "Could not delete palette 'Gone': it is not in the palette file."
        );
        assert_eq!(
            fs::read_to_string(store.path()).expect("still there"),
            before,
            "a refused delete does not rewrite the file"
        );
        assert_eq!(store.palettes().len(), 1, "the saved palette is untouched");
        assert_eq!(
            store.current_name(),
            "Mine",
            "a refused delete does not change what is applied"
        );

        // A name that differs only by case is a different question: the file is searched
        // case-insensitively, so that one is found and removed.
        let deleted = store.delete("MINE").expect("case-insensitive");
        assert_eq!(deleted.status, "Deleted palette 'MINE'.");
        assert_eq!(
            deleted.fallback,
            Some(DEFAULT_PALETTE),
            "different casing is the applied palette, so it falls back too"
        );
        assert!(store.palettes().is_empty());
    }

    #[test]
    fn deleting_a_palette_that_is_not_applied_leaves_the_current_name_alone() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.save_draft(false).expect("first save");
        store.draft_mut().name = "Other".to_owned();
        store.save_draft(false).expect("second save");
        assert_eq!(store.current_name(), "Other", "the second save is applied");

        let deleted = store.delete("Mine").expect("deleted");
        assert_eq!(
            deleted.fallback, None,
            "what is applied did not move, so the caller has nothing to persist"
        );
        assert_eq!(
            store.current_name(),
            "Other",
            "the applied palette is still the one that was applied"
        );
        assert_eq!(store.palettes().len(), 1);
    }

    #[test]
    fn the_saved_cards_paint_the_name_and_the_four_swatches() {
        let mut store = store();
        store.draft_mut().name = "Mine".to_owned();
        store.set("PaletteBgBox", "#1A2B3C");
        store.set("PalettePanelBox", "#202020");
        store.set("PaletteAccentBox", "#C7625A");
        store.set("PaletteTextBox", "#E8EAEE");
        store.save_draft(false).expect("save");

        let rows = store.rows();
        assert_eq!(rows.len(), 1, "one row per saved palette");
        let row = &rows[0];
        let panel = Color::from_hex(0x20_20_20);

        assert_eq!(
            row.name, "Mine",
            "the stored spelling, as the card shows it"
        );
        assert_eq!(row.background, Color::from_hex(0x1A_2B_3C));
        assert_eq!(row.panel, panel);
        assert_eq!(
            row.slider,
            panel.lighten(0.06),
            "SliderSwatch is the stored slider_background, which the save derived"
        );
        assert_eq!(row.accent, Color::from_hex(0xC7_62_5A));
    }

    /// Deviation, pinned: a card previews the palette the apply would paint.
    ///
    /// Managed built the four swatches with `ParseHexOr(..., Colors.Black)` and
    /// `ParseHexOr(..., Colors.Gray)` (`MainWindow.xaml.cs:1049-1052`), so a hand-edited role
    /// painted black on the card while the apply fell back to a derived shade. If someone
    /// restores the black fallback, the panel assertion below turns black and fails.
    #[test]
    fn a_corrupt_role_previews_what_applying_would_paint() {
        let store = store();
        fs::create_dir_all(store.path().parent().expect("parent")).expect("dir");
        let broken = CustomPalette {
            name: "Broken".to_owned(),
            background: "#1A2B3C".to_owned(),
            panel: "not-a-colour".to_owned(),
            accent: "#C7625A".to_owned(),
            ..CustomPalette::default()
        };
        fs::write(
            store.path(),
            to_palettes_json(std::slice::from_ref(&broken)),
        )
        .expect("write");

        let row = store.rows().pop().expect("the file has one palette");
        assert_eq!(row.name, "Broken");
        assert_eq!(row.background, Color::from_hex(0x1A_2B_3C));
        assert_eq!(
            row.panel,
            Color::from_hex(0x1A_2B_3C),
            "a role that does not parse falls back the way the apply does, not to black"
        );
        assert_eq!(
            row.panel,
            resolve_in(std::slice::from_ref(&broken), "Broken").panel,
            "the card and the apply read one chain"
        );
        assert_ne!(
            row.panel,
            Color::rgb(0, 0, 0),
            "managed's ParseHexOr fallback was black"
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
