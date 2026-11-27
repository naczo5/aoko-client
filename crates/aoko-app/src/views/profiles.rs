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
//!
//! One rule departs from managed, and only where managed lost data: a `config` slot that
//! does not parse is moved aside as `config.json.corrupt-<stamp>` and named in the status
//! line, instead of starting defaults that the exit save then writes over it. The slot has
//! no row in the config list, so it is the one file the user cannot see and cannot rescue.
//! A named config that will not parse is left where it is — it is in the list, and deleting
//! or overwriting it is a press.
//!
//! The second departure is a report rather than a rule: a document that loads *partly*. Managed's
//! `LoadProfile` was all-or-nothing, so one malformed value cost the whole file; the tolerant
//! reader keeps the fields it could read, names the ones it skipped in
//! [`aoko_protocol::profile::ProfileLoad::problems`], and `problems_line` puts that on the same
//! status line — `Could not load config: …` — so a setting back at its default is said out loud
//! rather than passing as the user's own choice.
//!
//! # A press queues its write; the exit save does not
//!
//! `TrySaveCurrentAs` and `CreateConfigButton_Click` used to write inside the click handler: a
//! `create_dir_all`, a create, a flush and a rename on a scanned or slow volume, plus the
//! directory scan `RefreshConfigList` ran afterwards. Both are queued now.
//! [`Profiles::save_named`], [`Profiles::create`] and [`Profiles::delete`] hand the bytes to the
//! process's background writer (`crate::views::file_store::submit`), which runs the same
//! `crate::views::file_store::write_atomically` swap the exit save uses, and the list the card
//! paints moves at once so a repaint never shows a half-written state.
//!
//! [`Profiles::save_auto`] is the one write that stays synchronous, and deliberately: it *is* the
//! exit save (`MainWindow.xaml.cs:1866-1869`), the process is about to leave, and a write handed to
//! a thread at that point would be dropped with the process instead of reaching the disk. It also
//! drains the queue for its file first, so a queued save of the same slot cannot land over it. A
//! *press* that writes the same slot — a palette apply, the palette delete's fallback, the HUD
//! reset — is not leaving the process, so it uses [`Profiles::save_auto_queued`] and the same
//! poll.
//!
//! A failure never disappears with the task: [`Profiles::poll`] settles each queued write and
//! writes its outcome into the same status line the synchronous save used — `Could not save
//! config: …`, `Could not delete '…': …` — and rolls the list back to the names the directory
//! actually holds. Only a write that reached the rename reports the success sentence.

use std::fs;
use std::sync::Arc;

use aoko_platform::PlatformError;
use aoko_platform::ProfileStore;
use aoko_protocol::profile::{load_profile, to_profile_json, Profile, ProfileProblem};

use crate::views::file_store::{
    flush_all_writes, flush_writes, reply, settled, submit, Reply, WriteKind,
};

/// The internal auto-save slot (`ProfileManager.AutoSaveConfigName`, `Profile.cs:371`).
pub const AUTO_SAVE_NAME: &str = "config";

/// Whether `name` is the internal slot, which a scan of the directory never lists.
///
/// The whole list comes from [`ProfileStore::list_profiles`], so it hides this name on its own.
/// The card also paints names whose write is still in flight, without asking the directory, which
/// is why the rule is repeated here: a write of the slot must never turn into a row either.
fn is_auto_save(name: &str) -> bool {
    name.eq_ignore_ascii_case(AUTO_SAVE_NAME)
}

/// Ceiling on a profile file's size, the same one
/// [`crate::views::file_store`] puts on `accounts.json` and `servers.json`.
///
/// A real profile is a few kilobytes. Without a ceiling
/// [`ProfileStore::read_profile`] materialises whatever is on disk, so a runaway or
/// foreign file could size an allocation this app could not have produced.
const MAXIMUM_PROFILE_BYTES: u64 = 8 * 1024 * 1024;

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

/// One config write queued for the background writer, and what the press that made it changed.
#[derive(Debug)]
struct PendingWrite {
    /// Where the rename reports.
    reply: Reply,
    /// The config name the job writes, or removes.
    name: String,
    /// Which of the two the press asked for.
    kind: PendingKind,
    /// The selection before the press, restored when the write fails.
    previous_selection: usize,
    /// The sentence the status line takes when the write lands.
    status: String,
}

/// What a queued config write does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    /// Write the file, creating it when the name is new.
    Save,
    /// Remove the file.
    Delete,
}

/// Where profiles live and what the list currently holds.
///
/// `None` for the store means "no persistence available in this context": the
/// capture harness and unit tests construct the shell that way on purpose, so a
/// screenshot never depends on the machine's profile directory. A store that exists
/// but failed to open stays `Some`-less too, with [`Profiles::store_error`] saying
/// why — the failure is reported, never replaced by a silent default.
#[derive(Debug)]
pub struct Profiles {
    store: Option<ProfileStore>,
    store_error: Option<String>,
    status: ConfigStatus,
    migrated: usize,
    configs: Vec<String>,
    selection: usize,
    /// The config the user pressed Delete on, waiting for the in-card confirmation.
    delete_armed: Option<String>,
    /// The config a Create would overwrite, waiting for its own confirmation.
    create_pending: Option<String>,
    /// The names the directory itself holds, as of the read at startup and the writes that have
    /// landed since. The painted list is this plus whatever is still in flight.
    confirmed: Vec<String>,
    /// Config writes queued and not yet settled, in submission order.
    pending: Vec<PendingWrite>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Drop for Profiles {
    /// Drains the queue on the way out, so a config saved a moment before the app quits is not
    /// lost with the process.
    fn drop(&mut self) {
        flush_all_writes();
    }
}

impl Profiles {
    /// Every field at its starting value, over `store` or over nothing.
    ///
    /// A plain constructor rather than `..Self::default()`: [`Profiles`] implements `Drop`, and the
    /// struct-update form cannot move fields out of a type that does.
    fn new(store: Option<ProfileStore>) -> Self {
        Self {
            store,
            store_error: None,
            status: ConfigStatus::default(),
            migrated: 0,
            configs: Vec::new(),
            selection: 0,
            delete_armed: None,
            create_pending: None,
            confirmed: Vec::new(),
            pending: Vec::new(),
        }
    }

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
        let mut profiles = Self::new(Some(store));

        // Migration runs before `initialize` so the count is the real one: the
        // store's own `initialize` migrates too, but only logs the number
        // (`aoko-platform/src/profile.rs:117-144`). Copying first means the second
        // attempt sees a populated directory and copies nothing, which is also the
        // rule a user who already has profiles gets.
        // Both sentences in this block are new copy. WPF never migrated anything — the profiles
        // directory is this port's own — and its configs were always writable, so a store that
        // came up read-only had no sentence at all. The shape is managed's: a
        // `SetConfigStatus($"Could not list configs: {ex.Message}")` line
        // (`MainWindow.xaml.cs:795`), "could not …" with the reason appended.
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
            ReadOutcome::Loaded(profile, problems) => {
                // A slot that loaded with fields the decoder had to skip says so before the
                // migration sentence: the skipped settings are the louder news.
                if !problems.is_empty() {
                    profiles.set_status(problems_line(&problems));
                }
                *profile
            }
            // A first launch has no auto-save slot: not an error, and no status line,
            // because there is nothing the user could do about it.
            ReadOutcome::Absent => Profile::default(),
            // A slot that exists and does not parse is the one read whose bytes would be lost by
            // carrying on: the slot has no row in the list, so the exit save would write the
            // defaults over settings the user cannot see and could not have recovered. The copy is
            // moved aside and the status line names it, which is the rule
            // [`crate::views::file_store`] already applies to a corrupt accounts or servers file.
            ReadOutcome::Unparsable => {
                let message = match profiles.quarantine(AUTO_SAVE_NAME) {
                    Some(kept) => crate::views::store_recovery::message(&kept),
                    // The rename itself failed, so the bytes are still where they were and the
                    // sentence keeps saying so rather than claiming a copy was kept.
                    None => format!("Could not parse profile '{AUTO_SAVE_NAME}'."),
                };
                profiles.set_status(message);
                Profile::default()
            }
            ReadOutcome::Failed(error) => {
                profiles.set_status(error);
                Profile::default()
            }
        };

        profiles.refresh();

        // New copy with no managed analogue: the WPF app never migrated anything. It
        // goes in the card, not only into `tracing`, because this binary installs no
        // subscriber yet and a log line would reach nobody.
        if profiles.migrated > 0 && profiles.status.message.is_empty() {
            let count = profiles.migrated;
            let noun = if count == 1 { "config" } else { "configs" };
            profiles.set_status(format!(
                "Migrated {count} {noun} from the previous version."
            ));
        }

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
    ///
    /// This is the explicit re-read, not a paint path: the constructor runs it once, before the
    /// first frame, and every press after that moves the list from what it wrote instead of
    /// scanning the directory again — so no frame and no press waits on a `read_dir` of a slow
    /// volume. A caller that wants the directory as it is now can still ask for it.
    pub fn refresh(&mut self) {
        let Some(store) = self.store.as_ref() else {
            self.confirmed.clear();
            self.rebuild();
            return;
        };

        match store.list_profiles() {
            Ok(names) => {
                self.confirmed = names;
                self.rebuild();
            }
            Err(error) => {
                self.set_status(format!("Could not list configs: {}", error.user_facing()));
            }
        }
    }

    /// Settles every config write that has finished, answering whether anything changed.
    ///
    /// A landed write confirms its name and paints the sentence the press earned; a failed one
    /// paints the reason and drops back to the names the directory holds, so the card cannot show
    /// a config the file refused. The caller repaints when this answers `true`.
    pub fn poll(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }

        let mut changed = false;
        let mut index = 0;
        while index < self.pending.len() {
            let Some(outcome) = settled(&self.pending[index].reply) else {
                index += 1;
                continue;
            };

            let done = self.pending.remove(index);
            changed = true;
            match outcome.error {
                Some(error) => {
                    let message = match done.kind {
                        PendingKind::Save => format!("Could not save config: {error}"),
                        PendingKind::Delete => {
                            format!("Could not delete '{}': {error}", done.name)
                        }
                    };
                    self.set_status(message);
                    self.selection = done.previous_selection;
                }
                None => {
                    match done.kind {
                        PendingKind::Save => {
                            if !is_auto_save(&done.name) && !self.confirmed.contains(&done.name) {
                                self.confirmed.push(done.name.clone());
                            }
                        }
                        PendingKind::Delete => {
                            self.confirmed.retain(|name| name != &done.name);
                        }
                    }
                    self.set_status(done.status);
                }
            }
        }

        if changed {
            self.rebuild();
        }
        changed
    }

    /// Reads one profile by name.
    ///
    /// `Ok(Profile::default())` is never returned for a missing file: the caller
    /// decides what an absent profile means, which is what keeps a saved config from
    /// being silently replaced by defaults.
    ///
    /// A save queued a moment ago is the newest state of this file, so a read waits for the
    /// writes of that path first: pressing Save and then Load is a round trip, and it would be a
    /// lie to answer it from the bytes the save is about to replace. With nothing in flight the
    /// wait touches nothing.
    ///
    /// A document that loaded with fields the reader had to skip answers the profile it could read
    /// and names what it lost (`problems_line`) in place of the `Loaded config` sentence: the
    /// settings that did load are in use, so the press did land.
    pub fn load_named(&mut self, name: &str) -> Result<Profile, String> {
        if let Some(path) = self.path(name) {
            flush_writes(&path);
        }

        match self.read(name) {
            ReadOutcome::Loaded(profile, problems) => {
                if problems.is_empty() {
                    self.set_status(format!("Loaded config '{name}'."));
                } else {
                    self.set_status(problems_line(&problems));
                }
                Ok(*profile)
            }
            // Absent, unparsable and unreadable are one answer here, which is the managed
            // `LoadConfigButton_Click` line for both of its failures
            // (`MainWindow.xaml.cs:820-826`). A named config that will not parse is left where it
            // is rather than quarantined: it is in the list, the user can see it, and deleting or
            // overwriting it is a press — the auto-save slot is the one read that has no row and
            // therefore no other way to keep its bytes (see [`Profiles::with_store`]).
            ReadOutcome::Absent | ReadOutcome::Unparsable | ReadOutcome::Failed(_) => {
                let message = format!("Failed to load config '{name}'.");
                self.set_status(message.clone());
                Err(message)
            }
        }
    }

    /// Writes the current settings to a named config, the way `TrySaveCurrentAs`
    /// does: the profile's own name becomes the file name.
    ///
    /// `Ok` means the bytes were queued and the row is in the list; the outcome — `Saved current
    /// settings to '…'.` or `Could not save config: …` — lands in [`Profiles::poll`], because a
    /// press that answered the success sentence before the rename would be claiming a file it has
    /// not written.
    pub fn save_named(&mut self, name: &str, profile: &mut Profile) -> Result<(), String> {
        let display = display_name(name);
        if display.is_empty() {
            let message = "Invalid config name.".to_owned();
            self.set_status(message.clone());
            return Err(message);
        }

        profile.name = display.clone();
        match self.queue(&display, profile, saved_line(&display)) {
            Ok(()) => {
                // The list moves now, the sentence lands with the rename (see `poll`): a success
                // reported before the swap is the defect a queued write makes easy to write.
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
    ///
    /// The failure line is the managed one with the noun changed. `save_named` reports
    /// `Could not save config: {ex.Message}` (`:964`), which is the same write under the name the
    /// user chose; the auto-save slot is the same write under the port's other name for it, and
    /// managed had no line for it because it wrote the slot silently on exit.
    ///
    /// **This write stays synchronous**, alone among the config saves. It is the exit save: the
    /// close hook runs it while the process is on its way out, a queued write would be dropped
    /// with the process, and `save_profile` is called for exactly that reason by `OnClosed`
    /// (`:1861-1871`), the title bar's button (`:1384-1392`) and the panic path. It drains the
    /// queue for this slot first, so a write still in flight cannot land over it. A *press* that
    /// writes the same slot is not an exit and takes [`Profiles::save_auto_queued`] instead.
    pub fn save_auto(&mut self, profile: &mut Profile) -> Result<(), String> {
        profile.name = AUTO_SAVE_NAME.to_owned();
        self.write_now(AUTO_SAVE_NAME, profile).map_err(|error| {
            let message = format!("Could not save settings: {error}");
            self.set_status(message.clone());
            message
        })
    }

    /// Queues the auto-save slot, which is what a press that changes settings writes.
    ///
    /// The palette presses and the HUD editor's reset save the slot the exit save writes, and none
    /// of them is leaving the process: they hand the bytes to the background writer the named
    /// saves already use, so a press returns instead of spending its frame on a create, a flush
    /// and a rename of `config.json`. `Ok` means queued; the sentence — `Saved current
    /// settings to 'config'.`, or `Could not save config: …` — is [`Profiles::poll`]'s,
    /// because a success reported before the rename would be claiming a file nobody wrote.
    ///
    /// The name is stamped first, as [`Profiles::save_auto`] stamps it, so the file this queues is
    /// the slot a restart loads. The slot has no row in the list, which `rebuild` and `poll`
    /// already refuse to give it.
    pub fn save_auto_queued(&mut self, profile: &mut Profile) -> Result<(), String> {
        profile.name = AUTO_SAVE_NAME.to_owned();
        self.queue(AUTO_SAVE_NAME, profile, saved_line(AUTO_SAVE_NAME))
            .map_err(|error| {
                let message = format!("Could not save config: {error}");
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

    /// The config waiting for the in-card Delete confirmation, if any.
    #[must_use]
    pub fn delete_armed(&self) -> Option<&str> {
        self.delete_armed.as_deref()
    }

    /// The name a Create would overwrite, waiting for its own confirmation.
    #[must_use]
    pub fn create_pending_name(&self) -> Option<&str> {
        self.create_pending.as_deref()
    }

    /// Arms the Delete confirmation.
    ///
    /// The managed app asked in a `MessageBox` (`DeleteConfigButton_Click`,
    /// `MainWindow.xaml.cs:864-893`); the card asks in place, so the armed name is
    /// state — which is also what makes the confirmation testable.
    pub fn request_delete(&mut self, name: &str) {
        self.delete_armed = Some(name.to_owned());
    }

    /// Disarms the confirmation without deleting.
    pub fn cancel_delete(&mut self) {
        self.delete_armed = None;
    }

    /// The confirmation's own Delete.
    pub fn confirm_delete(&mut self) -> Result<(), String> {
        let Some(name) = self.delete_armed.take() else {
            let message = "Select a config to delete.".to_owned();
            self.set_status(message.clone());
            return Err(message);
        };
        self.delete(&name)
    }

    /// Deletes one config.
    ///
    /// `ProfileStore` has no delete, so the path comes from `profile_path` — which
    /// sanitises the name — and the removal is queued on the same writer the saves use, which is
    /// what keeps a delete from landing in front of a save of the same name that is still in
    /// flight. A missing file is a failure rather than a quiet success: the managed
    /// `DeleteProfile` (`Profile.cs:556-561`) returns without touching the disk and the caller then
    /// reports "Deleted config", which is exactly the kind of lie this port removes. The check is
    /// one `is_file` on the press — not a scan — and it is skipped for a name a save is still
    /// writing, whose file does not exist yet.
    ///
    /// Deleting the **last** config is allowed: the list empties, the trigger shows
    /// nothing selected, and Load/Save/Delete report "Select a config to ...". The
    /// auto-save slot is separate (`list_profiles` hides it), so the current settings
    /// are still saved on exit and a restart restores them; nothing else is lost.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let display = display_name(name);
        if display.is_empty() {
            let message = "Select a config to delete.".to_owned();
            self.set_status(message.clone());
            return Err(message);
        }
        if display.eq_ignore_ascii_case(AUTO_SAVE_NAME) {
            let message = format!("'{display}' is a reserved name. Choose another.");
            self.set_status(message.clone());
            return Err(message);
        }

        let Some(path) = self.path(&display) else {
            let message = format!("Could not delete '{display}': not a config name.");
            self.set_status(message.clone());
            return Err(message);
        };

        let saving = self
            .pending
            .iter()
            .any(|pending| pending.name == display && pending.kind == PendingKind::Save);
        if !saving && !path.is_file() {
            // New copy for the one case `ex.Message` could not name: managed reported
            // `Could not delete '{name}': {ex.Message}` (`MainWindow.xaml.cs:886`), and a file
            // that vanished between the list and the press has no message worth quoting.
            let message = format!("Could not delete '{display}': it no longer exists");
            self.set_status(message.clone());
            // The file went behind the store's back, so the row the list still carries is a row
            // the disk no longer has: re-read before answering, or the press leaves a config the
            // user is told does not exist sitting in the list until something else refreshes it.
            self.refresh();
            return Err(message);
        }

        let previous_selection = self.selection();
        self.queue_delete(&display, previous_selection);
        Ok(())
    }

    /// Creates a named config from the current settings.
    ///
    /// The refusals are the managed ones, in the managed order and wording
    /// (`CreateConfigButton_Click`, `MainWindow.xaml.cs:894-933`): an empty or fully
    /// sanitised-away name, then the reserved auto-save name, then the overwrite
    /// confirmation the managed app asked in a `MessageBox`. `overwrite` is the
    /// caller saying the user confirmed; without it an existing name is untouched.
    ///
    /// A name whose own save is still in flight counts as existing, so a press cannot slip past
    /// the prompt and overwrite a config the queue has not written yet.
    pub fn create(&mut self, raw: &str, profile: &mut Profile, overwrite: bool) -> CreateOutcome {
        let name = display_name(raw);
        if name.is_empty() {
            let message =
                "Enter a valid config name (letters, numbers, spaces, - or _).".to_owned();
            self.set_status(message.clone());
            return CreateOutcome::Refused(message);
        }
        if name.eq_ignore_ascii_case(AUTO_SAVE_NAME) {
            let message = format!("'{name}' is a reserved name. Choose another.");
            self.set_status(message.clone());
            return CreateOutcome::Refused(message);
        }

        let exists = self
            .store
            .as_ref()
            .is_some_and(|store| store.profile_exists(&name))
            || self
                .pending
                .iter()
                .any(|pending| pending.name == name && pending.kind == PendingKind::Save);
        if exists && !overwrite {
            // The card's own prompt row carries the managed `MessageBox` copy and the
            // two buttons, so the status line stays free for the outcome.
            self.create_pending = Some(name.clone());
            return CreateOutcome::Overwrite(name);
        }
        self.create_pending = None;

        profile.name = name.clone();
        match self.queue(&name, profile, created_line(&name)) {
            Ok(()) => {
                self.select(&name);
                CreateOutcome::Created(name)
            }
            Err(error) => {
                let message = format!("Could not save config: {error}");
                self.set_status(message.clone());
                CreateOutcome::Refused(message)
            }
        }
    }

    /// Forgets a pending overwrite, for Cancel on the in-card prompt. The status line
    /// is left alone: cancelling says nothing the user did not just do.
    pub fn cancel_create(&mut self) {
        self.create_pending = None;
    }

    /// The directory the attached store reads and writes, for "Open Configs Folder".
    #[must_use]
    pub fn directory(&self) -> Option<String> {
        self.store.as_ref().map(describe)
    }

    /// The on-disk path for a sanitised name, or `None` when there is no store.
    fn path(&self, name: &str) -> Option<std::path::PathBuf> {
        self.store.as_ref()?.profile_path(name)
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

        // A file this app could not have written is not a profile: the ceiling is checked on the
        // metadata, so a runaway file cannot size an allocation, and it is the same rule and the
        // same number [`crate::views::file_store`] applies to the two data files.
        let too_large = self
            .path(name)
            .and_then(|path| fs::metadata(path).ok())
            .is_some_and(|metadata| metadata.len() > MAXIMUM_PROFILE_BYTES);
        if too_large {
            return ReadOutcome::Unparsable;
        }

        let text = match store.read_profile(name) {
            Ok(text) => text,
            Err(PlatformError::NotFound { .. }) => return ReadOutcome::Absent,
            Err(error) => return ReadOutcome::Failed(error.user_facing()),
        };

        let load = load_profile(&text);
        let Some(profile) = load.profile else {
            return ReadOutcome::Unparsable;
        };

        // The document's own problems, then the ones the object inside it reported: a malformed
        // `killAuraSettings` value is skipped by the same rule as a malformed top-level field,
        // and it decodes as one field of the document, so its report would otherwise reach
        // nobody.
        let mut problems = load.problems;
        problems.extend(profile.kill_aura_settings.problems().iter().cloned());
        ReadOutcome::Loaded(Box::new(profile), problems)
    }

    /// Moves an unparsable profile aside and answers where it went.
    ///
    /// The name comes from [`crate::views::file_store::quarantine`], so a copy of a profile and a
    /// copy of a data file are stamped and marked the same way and
    /// [`crate::views::store_recovery`] can name either. `None` when there is no store, when the
    /// name is not a file name, or when the rename failed — in which case the bytes are still where
    /// they were.
    fn quarantine(&self, name: &str) -> Option<String> {
        let path = self.path(name)?;
        crate::views::file_store::quarantine(&path).map(|kept| kept.to_string_lossy().into_owned())
    }

    /// The serialised profile, or the refusal that keeps the file that is already there.
    ///
    /// `to_profile_json` answers with an empty string when serde fails, and writing that would
    /// replace a config that loads with a file that does not: the next read would find nothing
    /// and the next save would write the defaults over it.
    fn json_for(&self, profile: &Profile) -> Result<String, String> {
        if self.store.is_none() {
            return Err("no profile store".to_owned());
        }

        let json = to_profile_json(profile);
        if json.trim().is_empty() {
            return Err("the profile could not be serialized".to_owned());
        }
        Ok(json)
    }

    /// Queues one config write and moves the list to what the press asked for.
    fn queue(&mut self, name: &str, profile: &Profile, status: String) -> Result<(), String> {
        // The store is checked before the name so a view with no store answers the reason it is
        // missing rather than blaming the name.
        let json = self.json_for(profile)?;
        let path = self
            .path(name)
            .ok_or_else(|| format!("'{name}' is not a config name"))?;

        let reply = reply();
        submit(path, WriteKind::Save(json), Arc::clone(&reply));
        self.pending.push(PendingWrite {
            reply,
            name: name.to_owned(),
            kind: PendingKind::Save,
            previous_selection: self.selection(),
            status,
        });
        self.rebuild();
        Ok(())
    }

    /// Queues the removal of one config file.
    fn queue_delete(&mut self, name: &str, previous_selection: usize) {
        let Some(path) = self.path(name) else {
            return;
        };

        let reply = reply();
        submit(path, WriteKind::Delete, Arc::clone(&reply));
        self.pending.push(PendingWrite {
            reply,
            name: name.to_owned(),
            kind: PendingKind::Delete,
            previous_selection,
            status: deleted_line(name),
        });
        self.rebuild();
    }

    /// Writes one config on this thread, for the exit save [`Profiles::save_auto`].
    ///
    /// The queue for that path is drained first. A write still in flight would otherwise land
    /// after this one, and the settings the process saved on its way out would be the state the
    /// app lost: draining makes this write the last word on the file.
    fn write_now(&self, name: &str, profile: &Profile) -> Result<(), String> {
        let Some(store) = self.store.as_ref() else {
            return Err("no profile store".to_owned());
        };
        let json = self.json_for(profile)?;
        if let Some(path) = store.profile_path(name) {
            flush_writes(&path);
        }

        store
            .write_profile(name, &json)
            .map_err(|error| error.user_facing())
    }

    /// Rebuilds the painted list from the directory's names plus the writes still in flight, and
    /// keeps the selection on the name it was on.
    ///
    /// The sort is `ProfileStore::list_profiles`' (`aoko-platform/src/profile.rs:250`): ordinal
    /// ignore case, with the spelling as a tie-break, so a name a press added sits where the next
    /// scan of the directory would put it.
    fn rebuild(&mut self) {
        let selected = self.selected().map(str::to_owned);

        let mut names = self.confirmed.clone();
        for pending in &self.pending {
            match pending.kind {
                PendingKind::Save => {
                    if !is_auto_save(&pending.name) && !names.contains(&pending.name) {
                        names.push(pending.name.clone());
                    }
                }
                PendingKind::Delete => names.retain(|name| name != &pending.name),
            }
        }
        names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));

        self.configs = names;
        self.selection = selected
            .and_then(|name| self.configs.iter().position(|entry| *entry == name))
            .unwrap_or_else(|| self.selection.min(self.configs.len().saturating_sub(1)));
    }
}

/// What pressing Create produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// Written and selected.
    Created(String),
    /// The name exists untouched: the card shows the overwrite prompt, and a
    /// confirmed second call passes `overwrite = true`.
    Overwrite(String),
    /// Refused; the status line already carries the reason.
    Refused(String),
}

/// The name a profile is shown and saved under, sanitised the way the protocol does
/// it, so a name with a path separator cannot be used to leave the directory.
#[must_use]
pub fn display_name(raw: &str) -> String {
    aoko_protocol::profile::sanitize_config_name(Some(raw))
}

/// `Saved current settings to '{name}'` for a write that has landed.
///
/// The sentence is `TrySaveCurrentAs`'s (`MainWindow.xaml.cs:968`), moved from the press to the
/// completion: a save reported before its rename is a success the file has not got.
fn saved_line(name: &str) -> String {
    format!("Saved current settings to '{name}'.")
}

/// `Created config '{name}'.` for a write that has landed (`MainWindow.xaml.cs:940`).
fn created_line(name: &str) -> String {
    format!("Created config '{name}'.")
}

/// `Deleted config '{name}'.` for a queued removal that has landed.
fn deleted_line(name: &str) -> String {
    format!("Deleted config '{name}'.")
}

/// What reading a profile produced.
///
/// Absent, Unparsable and Failed are kept apart because the three are treated
/// differently: a missing `config` slot at startup is silent (`Profile.cs:484-489`),
/// while a missing *named* file reports `Failed to load config`. Only an unparsable
/// document has bytes worth keeping, which is what the quarantine rule keys on.
///
/// A loaded profile carries the fields the tolerant decoder had to skip, which is what
/// [`problems_line`] turns into the status line: the settings that did load are in use, so the
/// read is neither a failure nor silent.
#[derive(Debug, Clone, PartialEq)]
enum ReadOutcome {
    /// Boxed: `Profile` is ~900 bytes and `Clippy` is right that the variant
    /// would otherwise dominate every `ReadOutcome`.
    Loaded(Box<Profile>, Vec<ProfileProblem>),
    /// No such file.
    Absent,
    /// The file is there, but its bytes are not a profile: too large to be one this
    /// app wrote, or JSON that does not fit the schema.
    Unparsable,
    /// The file exists and could not be read for an environmental reason, with the
    /// classified reason ready to paint.
    Failed(String),
}

/// `Could not load config: …` for a document that loaded with fields the decoder had to skip.
///
/// New copy, and the sentence [`ReadOutcome::Loaded`]'s report exists for: the managed
/// `LoadProfile` was all-or-nothing (`Aoko/Core/Profile.cs:484-506`, one `catch` around the whole
/// deserialise), so a malformed value either lost the file or, once the reader became tolerant,
/// cost a setting with nothing said. This says it, in the `Could not …: reason` shape the card's
/// other failures use, and closes with what the user still has. Only the first problem is spelt
/// out and the rest are counted: this is a report on one line, not a dialog.
fn problems_line(problems: &[ProfileProblem]) -> String {
    let Some(first) = problems.first() else {
        return String::new();
    };

    match problems.len() {
        1 => format!("Could not load config: {first}. The rest of the config is in use."),
        count => format!(
            "Could not load config: {first} (and {} more). The rest of the config is in use.",
            count - 1
        ),
    }
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
    use aoko_protocol::profile::load_profile_from_json;
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

    /// Settles every queued config write, the way the frames after a press do.
    ///
    /// A press queues its write, so a test that reads the disk or the status line immediately
    /// after one would be racing the writer thread. This waits for the outcome the card is going
    /// to paint, which is the only ordering the view promises.
    fn settled(profiles: &mut Profiles) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !profiles.pending.is_empty() {
            profiles.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "the queued config write never reported"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
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
            profiles.configs(),
            ["PvP".to_owned()],
            "the row is the press's, painted before the file is"
        );
        assert_eq!(profiles.selected(), Some("PvP"));
        assert!(
            !profiles.status().is_visible(),
            "a queued write is not a save yet"
        );

        settled(&mut profiles);
        assert_eq!(
            profiles.status().message,
            "Saved current settings to 'PvP'."
        );

        // A load waits for the write of that name, which is what makes the round trip honest.
        let loaded = profiles.load_named("PvP").expect("load");
        assert_eq!(loaded.min_cps, 7.0);
        assert_eq!(profiles.status().message, "Loaded config 'PvP'.");
    }

    #[test]
    fn a_missing_named_config_reports_the_managed_line() {
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
        assert_eq!(
            profiles.status().message,
            "Migrated 1 config from the previous version."
        );

        // A second launch copies nothing: the live directory is already populated.
        let (again, _) = Profiles::with_store(store_at(&root));
        assert_eq!(again.migrated(), 0);
        assert_eq!(
            again.status().message,
            "",
            "nothing to report, nothing said"
        );
    }

    #[test]
    fn the_selection_wraps_and_survives_a_refresh() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        for name in ["Alpha", "Beta", "Gamma"] {
            let mut profile = Profile::default();
            profiles.save_named(name, &mut profile).expect("save");
        }
        settled(&mut profiles);
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

    #[test]
    fn create_refuses_a_name_that_sanitises_to_nothing() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();

        assert_eq!(
            profiles.create("   ", &mut profile, false),
            CreateOutcome::Refused(
                "Enter a valid config name (letters, numbers, spaces, - or _).".to_owned()
            )
        );
        assert_eq!(
            profiles.status().message,
            "Enter a valid config name (letters, numbers, spaces, - or _)."
        );
        assert!(profiles.configs().is_empty());
    }

    #[test]
    fn create_refuses_the_reserved_auto_save_name() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();

        assert_eq!(
            profiles.create("Config", &mut profile, false),
            CreateOutcome::Refused("'Config' is a reserved name. Choose another.".to_owned())
        );
        assert!(!store_at(&root).profile_exists(AUTO_SAVE_NAME));
    }

    #[test]
    fn create_writes_the_named_config_and_selects_it() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profile.gui_theme = "Ink".to_owned();

        assert_eq!(
            profiles.create("  PvP  ", &mut profile, false),
            CreateOutcome::Created("PvP".to_owned())
        );
        assert_eq!(profiles.selected(), Some("PvP"));
        assert_eq!(profiles.create_pending_name(), None);

        settled(&mut profiles);
        assert_eq!(profiles.status().message, "Created config 'PvP'.");

        let text = store_at(&root).read_profile("PvP").expect("written");
        let loaded = load_profile_from_json(&text).expect("parses");
        assert_eq!(loaded.gui_theme, "Ink", "the live settings were written");
    }

    #[test]
    fn create_asks_before_overwriting_and_writes_only_when_confirmed() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut first = Profile::default();
        first.gui_theme = "Ink".to_owned();
        profiles.save_named("PvP", &mut first).expect("seed");

        let mut second = Profile::default();
        second.gui_theme = "Coral".to_owned();
        assert_eq!(
            profiles.create("PvP", &mut second, false),
            CreateOutcome::Overwrite("PvP".to_owned()),
            "a save still in flight is a name that already exists"
        );
        assert_eq!(profiles.create_pending_name(), Some("PvP"));

        // Confirmation is required before the file changes.
        settled(&mut profiles);
        let text = store_at(&root).read_profile("PvP").expect("still there");
        assert_eq!(
            load_profile_from_json(&text).expect("parses").gui_theme,
            "Ink"
        );

        assert_eq!(
            profiles.create("PvP", &mut second, true),
            CreateOutcome::Created("PvP".to_owned())
        );
        settled(&mut profiles);
        let text = store_at(&root).read_profile("PvP").expect("overwritten");
        assert_eq!(
            load_profile_from_json(&text).expect("parses").gui_theme,
            "Coral"
        );
        assert_eq!(profiles.create_pending_name(), None);
    }

    #[test]
    fn cancelling_the_overwrite_prompt_leaves_the_file_alone() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profiles.save_named("PvP", &mut profile).expect("seed");
        settled(&mut profiles);

        let _ = profiles.create("PvP", &mut profile, false);
        profiles.cancel_create();
        assert_eq!(profiles.create_pending_name(), None);
        assert!(store_at(&root).profile_exists("PvP"));
    }

    #[test]
    fn the_delete_confirmation_removes_the_file_only_when_confirmed() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        for name in ["Alpha", "Beta"] {
            let mut profile = Profile::default();
            profiles.save_named(name, &mut profile).expect("seed");
        }
        settled(&mut profiles);

        profiles.request_delete("Alpha");
        assert_eq!(profiles.delete_armed(), Some("Alpha"));
        assert!(
            store_at(&root).profile_exists("Alpha"),
            "arming is not deleting"
        );

        profiles.cancel_delete();
        assert_eq!(profiles.delete_armed(), None);
        assert!(store_at(&root).profile_exists("Alpha"));

        profiles.request_delete("Alpha");
        profiles.confirm_delete().expect("queued");
        assert_eq!(
            profiles.configs(),
            ["Beta".to_owned()],
            "the row goes at once"
        );

        settled(&mut profiles);
        assert!(!store_at(&root).profile_exists("Alpha"));
        assert_eq!(profiles.status().message, "Deleted config 'Alpha'.");
        assert_eq!(profiles.configs(), ["Beta".to_owned()]);
    }

    #[test]
    fn deleting_the_last_config_empties_the_list_and_the_selection() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profiles.save_named("Only", &mut profile).expect("seed");
        settled(&mut profiles);

        profiles.request_delete("Only");
        profiles.confirm_delete().expect("queued");

        assert!(profiles.configs().is_empty());
        assert_eq!(profiles.selected(), None, "the trigger shows nothing");

        settled(&mut profiles);
        assert_eq!(profiles.status().message, "Deleted config 'Only'.");

        // The auto-save slot is separate, so the live settings still survive a
        // restart; deleting every named config is not a reset.
        profiles.save_auto(&mut profile).expect("auto-save slot");
        assert!(store_at(&root).profile_exists(AUTO_SAVE_NAME));
        assert!(
            !profiles.configs().contains(&AUTO_SAVE_NAME.to_owned()),
            "the internal slot never appears in the list"
        );
    }

    #[test]
    fn delete_reports_a_file_that_is_already_gone_instead_of_claiming_success() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profiles.save_named("PvP", &mut profile).expect("seed");
        settled(&mut profiles);

        let path = store_at(&root).profile_path("PvP").expect("path");
        std::fs::remove_file(path).expect("removed behind the store's back");

        let error = profiles.delete("PvP").unwrap_err();
        assert_eq!(error, "Could not delete 'PvP': it no longer exists");
        assert_eq!(profiles.status().message, error);
        assert!(profiles.configs().is_empty(), "the list matches the disk");
    }

    #[test]
    fn delete_refuses_the_reserved_auto_save_slot() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profiles.save_auto(&mut profile).expect("auto-save slot");

        assert_eq!(
            profiles.delete(AUTO_SAVE_NAME).unwrap_err(),
            "'config' is a reserved name. Choose another."
        );
        assert!(store_at(&root).profile_exists(AUTO_SAVE_NAME));
    }

    /// An auto-save slot that will not parse keeps its bytes instead of being overwritten by the
    /// exit save, and the status line names the copy.
    ///
    /// The slot has no row in the config list, so before this the only trace of the user's
    /// settings was a sentence; the next save wrote the defaults over them.
    #[test]
    fn a_corrupt_auto_save_slot_is_moved_aside_rather_than_replaced_by_defaults() {
        let root = scratch();
        let store = store_at(&root);
        store.initialize().expect("initialize");
        let slot = store.profile_path(AUTO_SAVE_NAME).expect("path");
        std::fs::write(&slot, "{ this is not a profile").expect("corrupting the slot");

        let (profiles, profile) = Profiles::with_store(store_at(&root));

        assert_eq!(profile, Profile::default(), "this run starts from defaults");
        assert!(
            !slot.exists(),
            "the unreadable slot is no longer there to be overwritten"
        );
        let message = profiles.status().message.clone();
        assert!(
            message.starts_with("config.json could not be read."),
            "{message}"
        );
        assert!(message.contains("config.json.corrupt-"), "{message}");
        // The copy is a sibling named the way `store_recovery::find_quarantined` matches, and it
        // is not a `*.json` file, so it stays out of the config list.
        let kept: Vec<String> = std::fs::read_dir(store.profiles_dir())
            .expect("list the profile directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("config.json.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!(profiles.configs().is_empty());
    }

    /// A field the tolerant decoder had to skip is a sentence on the card, and the fields that
    /// did fit are in use: the read is neither a failure nor silent.
    #[test]
    fn a_skipped_field_is_reported_at_startup() {
        let root = scratch();
        let store = store_at(&root);
        store.initialize().expect("initialize");
        let slot = store.profile_path(AUTO_SAVE_NAME).expect("path");
        std::fs::write(
            &slot,
            r#"{"minCPS":"9","maxCPS":16,"killAuraSettings":{"maxCps":"nope"}}"#,
        )
        .expect("writing a malformed field");

        let (profiles, profile) = Profiles::with_store(store_at(&root));

        assert_eq!(profile.max_cps, 16.0, "the fields that fit are in use");
        let message = profiles.status().message.clone();
        assert!(message.starts_with("Could not load config: "), "{message}");
        assert!(message.contains("minCPS"), "{message}");
        assert!(
            message.ends_with("The rest of the config is in use."),
            "{message}"
        );
        assert!(
            slot.is_file(),
            "a file with one bad field is not moved aside"
        );
    }

    /// The named load reports the same skip instead of claiming the file loaded whole, and still
    /// answers the profile it could read.
    #[test]
    fn a_skipped_field_is_reported_on_a_named_load() {
        let root = scratch();
        let store = store_at(&root);
        store.initialize().expect("initialize");
        let path = store.profile_path("PvP").expect("path");
        std::fs::write(&path, r#"{"minCPS":"9","maxCPS":16}"#).expect("malformed field");

        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let loaded = profiles
            .load_named("PvP")
            .expect("the document still loads");

        assert_eq!(loaded.max_cps, 16.0);
        let message = profiles.status().message.clone();
        assert!(message.starts_with("Could not load config: "), "{message}");
        assert!(message.contains("minCPS"), "{message}");
    }

    /// A named config that will not parse is left where it is: it is in the list, so the user can
    /// delete or overwrite it, and the load reports the managed failure line.
    #[test]
    fn a_corrupt_named_config_is_reported_and_left_in_place() {
        let root = scratch();
        let store = store_at(&root);
        store.initialize().expect("initialize");
        let path = store.profile_path("PvP").expect("path");
        std::fs::write(&path, "[1, 2, 3]").expect("corrupting the config");

        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        assert_eq!(profiles.configs(), ["PvP".to_owned()], "it still lists");

        assert_eq!(
            profiles.load_named("PvP").unwrap_err(),
            "Failed to load config 'PvP'."
        );
        assert!(path.is_file(), "the user's bytes stay where they put them");
    }

    /// A file far too large to be a profile is not read into memory: it is unparsable.
    #[test]
    fn a_profile_over_the_size_ceiling_is_quarantined_rather_than_read() {
        let root = scratch();
        let store = store_at(&root);
        store.initialize().expect("initialize");
        let path = store.profile_path(AUTO_SAVE_NAME).expect("path");
        // Sparse rather than eight mebibytes of bytes: the metadata is what the ceiling reads.
        std::fs::File::create(&path)
            .expect("create")
            .set_len(MAXIMUM_PROFILE_BYTES + 1)
            .expect("extend");

        let (profiles, profile) = Profiles::with_store(store_at(&root));

        assert_eq!(profile, Profile::default());
        let message = profiles.status().message.clone();
        assert!(
            message.starts_with("config.json could not be read."),
            "{message}"
        );
    }

    /// The exit save is the one write that must be on disk when it returns.
    ///
    /// `Shell::close` and the close hook call it and then end the process: a queued write would be
    /// dropped there, so this one is deliberately synchronous and the file is readable at once.
    #[test]
    fn the_exit_save_is_on_disk_before_it_returns() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profile.min_cps = 21.0;

        profiles.save_auto(&mut profile).expect("exit save");

        let text = store_at(&root)
            .read_profile(AUTO_SAVE_NAME)
            .expect("written before the call returned");
        let restored = load_profile_from_json(&text).expect("parses");
        assert_eq!(restored.min_cps, 21.0);
    }

    /// A repaint between a save and its rename paints the press, not a half-written file.
    #[test]
    fn the_list_moves_before_the_write_lands() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profile.min_cps = 4.0;

        profiles.save_named("Quick", &mut profile).expect("save");

        // No settle: this is the frame after the press.
        assert_eq!(profiles.configs(), ["Quick".to_owned()]);
        assert_eq!(profiles.selected(), Some("Quick"));

        settled(&mut profiles);
        assert_eq!(
            profiles.status().message,
            "Saved current settings to 'Quick'."
        );
    }

    /// Two commits in quick succession: the last one is the file, and neither is lost to the
    /// temporary the two of them share.
    #[test]
    fn two_quick_saves_end_on_the_last_settings() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));

        let mut first = Profile::default();
        first.min_cps = 2.0;
        profiles.save_named("Slider", &mut first).expect("first");
        let mut second = Profile::default();
        second.min_cps = 9.0;
        profiles.save_named("Slider", &mut second).expect("second");

        settled(&mut profiles);

        let text = store_at(&root).read_profile("Slider").expect("written");
        assert_eq!(
            load_profile_from_json(&text).expect("parses").min_cps,
            9.0,
            "the last commit is what the file holds"
        );
        assert_eq!(profiles.configs(), ["Slider".to_owned()]);
    }

    /// A write that cannot land says so and takes the row back out of the list.
    #[test]
    fn a_failed_save_reports_the_reason_and_rolls_the_list_back() {
        let root = scratch();
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let store = store_at(&root);
        store.initialize().expect("initialize");

        // A directory where the sibling temporary belongs fails the swap, which is the shape a
        // full or read-only volume takes here.
        let path = store.profile_path("Doomed").expect("path");
        let mut temporary = path.into_os_string();
        temporary.push(".tmp");
        std::fs::create_dir_all(PathBuf::from(&temporary)).expect("a blocking directory");

        let mut profile = Profile::default();
        profiles.save_named("Doomed", &mut profile).expect("queued");
        assert_eq!(profiles.configs(), ["Doomed".to_owned()]);

        settled(&mut profiles);

        let message = profiles.status().message.clone();
        assert!(message.starts_with("Could not save config: "), "{message}");
        assert!(
            profiles.configs().is_empty(),
            "a config the file refused is not painted as saved"
        );
        assert!(!store.profile_exists("Doomed"));
    }

    /// A profile directory that is not valid UTF-8 is an ordinary directory here.
    #[cfg(unix)]
    #[test]
    fn a_save_into_a_non_utf8_directory_lands() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = scratch().join(OsStr::from_bytes(b"root\xFF"));
        std::fs::create_dir_all(&root).expect("a root that is not UTF-8");
        let (mut profiles, _) = Profiles::with_store(store_at(&root));
        let mut profile = Profile::default();
        profile.min_cps = 6.0;

        profiles.save_named("Mine", &mut profile).expect("queued");
        settled(&mut profiles);

        assert_eq!(
            profiles.status().message,
            "Saved current settings to 'Mine'."
        );
        let text = store_at(&root).read_profile("Mine").expect("written");
        let restored = load_profile_from_json(&text).expect("parses");
        assert_eq!(restored.min_cps, 6.0);
    }
}
