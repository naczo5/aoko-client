//! Profile persistence: the store, the auto-save slot, and the Settings tab's
//! config list.
//!
//! The managed app kept this in `Aoko/Core/Profile.cs` (`ProfileManager`) and drove
//! it from `MainWindow.xaml.cs`. The cadence, which this module matches rather than
//! inventing:
//!
//! * **Load once at startup.** `MainWindow.xaml.cs:407-412` reads the `config` slot
//!   and applies it; nothing reloads a profile when a tab is opened.
//! * **Save on exit.** `MainWindow.OnClosed` (`:1861-1871`) and the title-bar path
//!   `CloseButton_Click` (`:1384-1392`) both write the `config` slot. There is no
//!   save per keystroke and no dirty flag, so a crash loses unsaved changes exactly
//!   as the managed app did. [`Shell`](crate::shell::Shell) hooks both exits.
//! * **Save on an explicit named save.** `TrySaveCurrentAs` (`:946-970`) writes the
//!   name the user chose, which is the Settings card's Save button.
//! * **HUD layout edits quick-save** (`:1414`, `GameStateClient.cs:1642`). That
//!   belongs to the HUD editor, which is not built yet, so it is not claimed here.
//!
//! `Profile::AutoSaveConfigName` is `"config"` (`Profile.cs:371`); the store's
//! `list_profiles` excludes it from the user-facing list.

use aoko_platform::PlatformError;
use aoko_platform::ProfileStore;
use aoko_protocol::profile::{load_profile_from_json, to_profile_json, Profile};

/// The internal auto-save slot (`ProfileManager.AutoSaveConfigName`, `Profile.cs:371`).
pub const AUTO_SAVE_NAME: &str = "config";

/// The Settings card's status line, one message at a time.
///
/// The managed surface is `ConfigStatusText` (`MainWindow.xaml:1536`), set through
/// `SetConfigStatus` (`MainWindow.xaml.cs:807-811`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigStatus {
    pub message: String,
}

impl ConfigStatus {
    /// Whether there is a message to paint.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        !self.message.trim().is_empty()
    }
}

/// Where profiles live and what the list currently holds.
///
/// `None` for the store means "no persistence available in this context": the
/// capture harness and unit tests construct the shell that way on purpose, so a
/// screenshot never depends on the machine's profile directory. A store that exists
/// but failed to open stays `Some`-less too, with [`Profiles::store_error`] saying
/// why — the failure is reported, never replaced by a silent default.
#[derive(Debug, Default)]
pub struct Profiles {
    store: Option<ProfileStore>,
    store_error: Option<String>,
    status: ConfigStatus,
    migrated: usize,
    configs: Vec<String>,
    selection: usize,
}

impl Profiles {
    /// A store over the platform's app-data root, with migration and a first read.
    ///
    /// Returns the profile to start from: the `config` slot when it parses, else
    /// [`Profile::default`]. An unreadable or missing slot is not an error — a first
    /// launch has none, and the managed `LoadProfile` returns `null` for exactly
    /// that case (`Profile.cs:484-489`).
    #[must_use]
    pub fn from_system() -> (Self, Profile) {
        match ProfileStore::from_system() {
            Ok(store) => Self::with_store(store),
            Err(error) => {
                let mut profiles = Self::unavailable();
                profiles.store_error = Some(error.user_facing());
                (profiles, Profile::default())
            }
        }
    }

    /// A store over an explicit app-data root, for tests and for an injected path.
    #[must_use]
    pub fn with_store(store: ProfileStore) -> (Self, Profile) {
        let mut profiles = Self {
            store: Some(store),
            ..Self::default()
        };

        // Migration runs before `initialize` so the count is the real one: the
        // store's own `initialize` migrates too, but only logs the number
        // (`aoko-platform/src/profile.rs:117-144`). Copying first means the second
        // attempt sees a populated directory and copies nothing, which is also the
        // rule a user who already has profiles gets.
        match profiles.migrate() {
            Ok(copied) => profiles.migrated = copied,
            Err(error) => {
                profiles.set_status(format!("Could not migrate: {}", error.user_facing()))
            }
        }

        if let Some(store) = profiles.store.as_ref() {
            if let Err(error) = store.initialize() {
                profiles.store_error = Some(error.user_facing());
                profiles.set_status(format!(
                    "Profiles are read-only this session: {}",
                    error.user_facing()
                ));
            }
        }

        let profile = match profiles.read(AUTO_SAVE_NAME) {
            ReadOutcome::Loaded(profile) => *profile,
            // A first launch has no auto-save slot: not an error, and no status line,
            // because there is nothing the user could do about it.
            ReadOutcome::Absent => Profile::default(),
            ReadOutcome::Failed(error) => {
                profiles.set_status(error);
                Profile::default()
            }
        };

        profiles.refresh();
        (profiles, profile)
    }

    /// No persistence: the capture harness and tests.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::default()
    }

    /// Whether a store is attached.
    #[must_use]
    pub fn has_store(&self) -> bool {
        self.store.is_some()
    }

    /// Why the store could not be opened, if it could not.
    #[must_use]
    pub fn store_error(&self) -> Option<&str> {
        self.store_error.as_deref()
    }

    /// How many legacy profiles this run copied, which the UI reports.
    #[must_use]
    pub fn migrated(&self) -> usize {
        self.migrated
    }

    /// The user-facing config names, sorted by the platform.
    #[must_use]
    pub fn configs(&self) -> &[String] {
        &self.configs
    }

    /// The status line.
    #[must_use]
    pub fn status(&self) -> &ConfigStatus {
        &self.status
    }

    /// The selected config's name, `None` when the list is empty.
    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        if self.configs.is_empty() {
            return None;
        }
        self.configs.get(self.selection).map(String::as_str)
    }

    /// The selection index, clamped to the list.
    #[must_use]
    pub fn selection(&self) -> usize {
        self.selection.min(self.configs.len().saturating_sub(1))
    }

    /// Moves the selection by one, wrapping, as `components::combo` cycling does.
    pub fn select_next(&mut self) {
        if !self.configs.is_empty() {
            self.selection = (self.selection() + 1) % self.configs.len();
        }
    }

    /// Sets the selection by name.
    pub fn select(&mut self, name: &str) -> bool {
        match self.configs.iter().position(|entry| entry == name) {
            Some(index) => {
                self.selection = index;
                true
            }
            None => false,
        }
    }

    /// Re-reads the config list, keeping the selection when it still exists
    /// (`RefreshConfigList`, `MainWindow.xaml.cs:781-805`).
    pub fn refresh(&mut self) {
        let previous = self.selected().map(str::to_owned);
        let Some(store) = self.store.as_ref() else {
            self.configs.clear();
            self.selection = 0;
            return;
        };

        match store.list_profiles() {
            Ok(names) => {
                self.configs = names;
                self.selection = previous
                    .and_then(|name| self.configs.iter().position(|entry| *entry == name))
                    .unwrap_or(0);
            }
            Err(error) => {
                self.set_status(format!("Could not list configs: {}", error.user_facing()));
            }
        }
    }

    /// Reads one profile by name.
    ///
    /// `Ok(Profile::default())` is never returned for a missing file: the caller
    /// decides what an absent profile means, which is what keeps a saved config from
    /// being silently replaced by defaults.
    pub fn load_named(&mut self, name: &str) -> Result<Profile, String> {
        match self.read(name) {
            ReadOutcome::Loaded(profile) => {
                self.set_status(format!("Loaded config '{name}'."));
                Ok(*profile)
            }
            ReadOutcome::Absent | ReadOutcome::Failed(_) => {
                // The managed line is the same for a missing file and a bad one
                // (`LoadConfigButton_Click`, `MainWindow.xaml.cs:820-826`).
                let message = format!("Failed to load config '{name}'.");
                self.set_status(message.clone());
                self.refresh();
                Err(message)
            }
        }
    }

    /// Writes the current settings to a named config, the way `TrySaveCurrentAs`
    /// does: the profile's own name becomes the file name.
    pub fn save_named(&mut self, name: &str, profile: &mut Profile) -> Result<(), String> {
        let display = display_name(name);
        if display.is_empty() {
            let message = "Invalid config name.".to_owned();
            self.set_status(message.clone());
            return Err(message);
        }

        profile.name = display.clone();
        match self.write(&display, profile) {
            Ok(()) => {
                self.set_status(format!("Saved current settings to '{display}'."));
                self.refresh();
                self.select(&display);
                Ok(())
            }
            Err(error) => {
                let message = format!("Could not save config: {error}");
                self.set_status(message.clone());
                Err(message)
            }
        }
    }

    /// Writes the auto-save slot, which is what the app does on exit
    /// (`MainWindow.xaml.cs:1866-1869`).
    pub fn save_auto(&mut self, profile: &mut Profile) -> Result<(), String> {
        profile.name = AUTO_SAVE_NAME.to_owned();
        self.write(AUTO_SAVE_NAME, profile).map_err(|error| {
            let message = format!("Could not save settings: {error}");
            self.set_status(message.clone());
            message
        })
    }

    /// Sets the Settings card's status line.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = ConfigStatus {
            message: message.into(),
        };
    }

    /// Copies legacy profiles and returns how many were written.
    fn migrate(&self) -> Result<usize, PlatformError> {
        match self.store.as_ref() {
            Some(store) => store.migrate_legacy_profiles(),
            None => Ok(0),
        }
    }

    fn read(&self, name: &str) -> ReadOutcome {
        let Some(store) = self.store.as_ref() else {
            return ReadOutcome::Absent;
        };

        let text = match store.read_profile(name) {
            Ok(text) => text,
            Err(PlatformError::NotFound { .. }) => return ReadOutcome::Absent,
            Err(error) => return ReadOutcome::Failed(error.user_facing()),
        };

        match load_profile_from_json(&text) {
            Some(profile) => ReadOutcome::Loaded(Box::new(profile)),
            None => ReadOutcome::Failed(format!("Could not parse profile '{name}'.")),
        }
    }

    fn write(&self, name: &str, profile: &Profile) -> Result<(), String> {
        let Some(store) = self.store.as_ref() else {
            return Err("no profile store".to_owned());
        };

        let json = to_profile_json(profile);
        store
            .write_profile(name, &json)
            .map_err(|error| error.user_facing())
    }
}

/// The name a profile is shown and saved under, sanitised the way the protocol does
/// it, so a name with a path separator cannot be used to leave the directory.
#[must_use]
pub fn display_name(raw: &str) -> String {
    aoko_protocol::profile::sanitize_config_name(Some(raw))
}

/// What reading a profile produced.
///
/// Absent and Failed are kept apart because the managed code treats them
/// differently: a missing `config` slot at startup is silent (`Profile.cs:484-489`),
/// while a missing *named* file reports `Failed to load config`.
#[derive(Debug, Clone, PartialEq)]
enum ReadOutcome {
    /// Boxed: `Profile` is ~900 bytes and `Clippy` is right that the variant
    /// would otherwise dominate every `ReadOutcome`.
    Loaded(Box<Profile>),
    Absent,
    Failed(String),
}

/// A row for the config list, for the click handler to name in a status message.
pub fn describe(store: &ProfileStore) -> String {
    store.profiles_dir().display().to_string()
}

/// The app-data directory the store would use, for the "Open Configs Folder" row.
///
/// Kept here rather than in the view so the path decision lives with the store.
#[must_use]
pub fn profiles_directory() -> Option<String> {
    ProfileStore::from_system()
        .ok()
        .map(|store| describe(&store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn scratch() -> PathBuf {
        let index = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("aoko-profiles-{}-{index}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    fn store_at(root: &Path) -> ProfileStore {
        ProfileStore::new(root)
    }

    #[test]
    fn a_first_launch_yields_defaults_and_no_error() {
        let root = scratch();
        let (profiles, profile) = Profiles::with_store(store_at(&root));

        assert!(profiles.has_store());
        assert_eq!(profiles.store_error(), None);
        assert!(!profiles.status().is_visible(), "absence is not an error");
        assert_eq!(profile, Profile::default());
        assert_eq!(profiles.migrated(), 0);
        assert!(profiles.configs().is_empty());
        assert_eq!(profiles.selected(), None);
    }

    #[test]
    fn the_auto_save_slot_round_trips_through_the_store() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));

        let mut profile = Profile::default();
        profile.min_cps = 13.0;
        profiles.save_auto(&mut profile).expect("write");
        profiles.set_status("");

        // A second shell over the same directory reads it back.
        let (reloaded, restored) = Profiles::with_store(store_at(&root));
        assert_eq!(restored.min_cps, 13.0);
        assert!(!reloaded.status().is_visible());

        // And the auto-save slot stays out of the user-facing list.
        assert!(reloaded.configs().iter().all(|name| name != AUTO_SAVE_NAME));
    }

    #[test]
    fn a_named_save_appears_in_the_list_and_can_be_loaded() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));

        let mut profile = Profile::default();
        profile.min_cps = 7.0;
        profiles.save_named("PvP", &mut profile).expect("save");
        assert_eq!(
            profiles.status().message,
            "Saved current settings to 'PvP'."
        );
        assert_eq!(profiles.configs(), ["PvP".to_owned()]);
        assert_eq!(profiles.selected(), Some("PvP"));

        let loaded = profiles.load_named("PvP").expect("load");
        assert_eq!(loaded.min_cps, 7.0);
        assert_eq!(profiles.status().message, "Loaded config 'PvP'.");
    }

    #[test]
    fn a_missing_named_config_reports_and_refreshes() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));

        let error = profiles.load_named("Nope").expect_err("missing");
        assert_eq!(error, "Failed to load config 'Nope'.");
        assert_eq!(profiles.status().message, "Failed to load config 'Nope'.");
    }

    #[test]
    fn an_unsafe_name_is_sanitised_for_display_and_on_disk() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));

        let mut profile = Profile::default();
        profiles
            .save_named("../escape", &mut profile)
            .expect("sanitised");
        let name = display_name("../escape");
        assert_eq!(profile.name, name);
        assert!(
            !name.contains('/') && !name.contains('\\'),
            "no separator survives"
        );
        assert!(profiles.configs().contains(&name));

        // The written file is a direct child of the profile directory: a name cannot
        // escape it, which is the property the sanitising exists for.
        let store = store_at(&root);
        let path = store.profile_path(&name).expect("sanitised path");
        assert_eq!(path.parent(), Some(store.profiles_dir()));
    }

    #[test]
    fn legacy_profiles_are_migrated_once_and_the_count_is_kept() {
        let root = scratch();
        let store = store_at(&root);
        std::fs::create_dir_all(store.legacy_profiles_dir()).expect("legacy dir");
        let source = store.legacy_profiles_dir().join("Old.json");
        std::fs::write(&source, to_profile_json(&Profile::default())).expect("write legacy");

        let (profiles, _) = Profiles::with_store(store_at(&root));
        assert_eq!(profiles.migrated(), 1, "the count is reported, not dropped");
        assert_eq!(profiles.configs(), ["Old".to_owned()]);

        // A second launch copies nothing: the live directory is already populated.
        let (again, _) = Profiles::with_store(store_at(&root));
        assert_eq!(again.migrated(), 0);
    }

    #[test]
    fn the_selection_wraps_and_survives_a_refresh() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        for name in ["Alpha", "Beta", "Gamma"] {
            let mut profile = Profile::default();
            profiles.save_named(name, &mut profile).expect("save");
        }
        profiles.refresh();
        assert_eq!(profiles.configs().len(), 3);

        profiles.select("Beta");
        assert_eq!(profiles.selected(), Some("Beta"));
        profiles.refresh();
        assert_eq!(profiles.selected(), Some("Beta"), "kept across a refresh");

        profiles.select_next();
        assert_eq!(profiles.selected(), Some("Gamma"));
        profiles.select_next();
        assert_eq!(profiles.selected(), Some("Alpha"), "wraps");
    }

    #[test]
    fn without_a_store_every_write_says_so_instead_of_pretending() {
        let mut profiles = Profiles::unavailable();
        let mut profile = Profile::default();

        assert!(!profiles.has_store());
        assert!(profiles.save_auto(&mut profile).is_err());
        assert!(profiles.save_named("Name", &mut profile).is_err());
        assert!(profiles.load_named("Name").is_err());
        assert!(profiles.configs().is_empty());
    }

    #[test]
    fn an_empty_name_is_refused_with_the_managed_line() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();

        assert_eq!(
            profiles.save_named("   ", &mut profile).unwrap_err(),
            "Invalid config name."
        );
        assert_eq!(display_name(""), "");
    }
}
