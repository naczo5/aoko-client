//! Target and bridge-version selection for injection.
//!
//! Ported from `Aoko/WindowPickerDialog.xaml.cs`. The managed dialog did three
//! things: list the injectable processes, let the user pick one, and confirm a
//! process that carries no Minecraft marker before handing it to the injector.
//! That last part is the interesting one — it is a *refusal path*, not a happy
//! path, so it is modelled explicitly instead of being a `MessageBox` call buried
//! in a click handler.
//!
//! The process list itself comes from `aoko_platform::DiscoveryTarget`; this module
//! never enumerates processes, which is what keeps it testable without a JVM.

use aoko_platform::DiscoveryTarget;

/// One choice in the bridge-version dropdown: what the user reads and the tag the
/// bridge receives.
pub const VERSION_CHOICES: [(&str, &str); 6] = [
    ("Auto-detect", "auto"),
    ("1.8.9", "1.8.9"),
    ("1.21", "1.21"),
    ("26.1", "26.1"),
    ("26.2", "26.2"),
    ("26.3", "26.3"),
];

/// Tag used when no explicit version is chosen.
pub const AUTO_VERSION: &str = "auto";

/// Dialog title.
pub const TITLE: &str = "Select Java client";

/// The sentence under the title.
pub const SUBTITLE: &str =
    "Multiple Java/Minecraft processes were found. Select the client process to inject into.";

/// Label above the version dropdown.
pub const VERSION_LABEL: &str = "Bridge version";

/// The list was empty and Inject was pressed.
pub const NO_SELECTION_TITLE: &str = "No process selected";
/// The list was empty and Inject was pressed.
pub const NO_SELECTION_MESSAGE: &str = "Select a Java process from the list.";

/// The chosen process carries no client marker.
pub const UNIDENTIFIED_TITLE: &str = "Unidentified Java process";
/// The chosen process carries no client marker.
pub const UNIDENTIFIED_MESSAGE: &str = "A Minecraft/client marker was not found for this Java \
     process. Injection may fail if it is not a Minecraft client.";

/// Buttons.
pub const REFRESH_LABEL: &str = "Refresh list";
/// Cancel.
pub const CANCEL_LABEL: &str = "Cancel";
/// Accept.
pub const INJECT_LABEL: &str = "Inject into selected client";

/// Why Inject did not proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing selected; the dialog stays open with a warning.
    NoSelection,
    /// The process looks like a bare JVM; the dialog asks first.
    UnidentifiedProcess,
}

impl Refusal {
    /// The message box title the managed code used.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::NoSelection => NO_SELECTION_TITLE,
            Self::UnidentifiedProcess => UNIDENTIFIED_TITLE,
        }
    }

    /// The message box body.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::NoSelection => NO_SELECTION_MESSAGE,
            Self::UnidentifiedProcess => UNIDENTIFIED_MESSAGE,
        }
    }
}

/// What pressing Inject decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectDecision {
    /// Nothing to inject: show the refusal and stay open.
    Refused(Refusal),
    /// Ask the user; on OK, call [`WindowPickerState::confirm_inject`].
    NeedsConfirmation(Refusal),
    /// Hand this process and version to the injector.
    Accepted {
        /// Index into [`WindowPickerState::targets`].
        target_index: usize,
        /// Process id, to cross-check the caller's own list.
        pid: u32,
        /// Bridge version tag.
        version: &'static str,
    },
}

/// The picker's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowPickerState {
    targets: Vec<DiscoveryTarget>,
    selected: Option<usize>,
    version_index: usize,
}

impl WindowPickerState {
    /// A picker over an already-discovered list.
    ///
    /// The managed constructor selected the first row when there was one, and this
    /// does the same: an empty list stays unselected so Inject can refuse instead
    /// of injecting into whatever happened to be first.
    #[must_use]
    pub fn new(targets: Vec<DiscoveryTarget>) -> Self {
        let mut state = Self {
            targets,
            selected: None,
            version_index: 0,
        };
        state.select_first();
        state
    }

    /// The listed processes.
    #[must_use]
    pub fn targets(&self) -> &[DiscoveryTarget] {
        &self.targets
    }

    /// The highlighted process.
    #[must_use]
    pub fn selected(&self) -> Option<&DiscoveryTarget> {
        self.selected.and_then(|index| self.targets.get(index))
    }

    /// Index of the highlighted process.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// Highlights one row; an out-of-range index is ignored rather than clamped,
    /// so a stale index cannot silently inject into a different process.
    pub fn select(&mut self, index: Option<usize>) {
        match index {
            None => self.selected = None,
            Some(index) if index < self.targets.len() => self.selected = Some(index),
            Some(_) => {}
        }
    }

    /// `RefreshWindowList`: replaces the list and re-selects the first row.
    pub fn refresh(&mut self, targets: Vec<DiscoveryTarget>) {
        self.targets = targets;
        self.select_first();
    }

    /// The version dropdown's entries.
    #[must_use]
    pub const fn version_options(&self) -> &'static [(&'static str, &'static str)] {
        &VERSION_CHOICES
    }

    /// Index of the chosen version.
    #[must_use]
    pub const fn version_index(&self) -> usize {
        self.version_index
    }

    /// The chosen version tag; out-of-range falls back to `auto`.
    #[must_use]
    pub fn selected_version(&self) -> &'static str {
        VERSION_CHOICES
            .get(self.version_index)
            .map_or(AUTO_VERSION, |(_, tag)| *tag)
    }

    /// Chooses a version; out-of-range falls back to `auto`, which is what
    /// `ResolveSelectedVersion` did when the combo had no selection.
    pub fn set_version_index(&mut self, index: usize) {
        self.version_index = if index < VERSION_CHOICES.len() {
            index
        } else {
            0
        };
    }

    /// `InjectButton_Click` up to the confirmation.
    #[must_use]
    pub fn request_inject(&self) -> InjectDecision {
        let Some(index) = self.selected else {
            return InjectDecision::Refused(Refusal::NoSelection);
        };

        let Some(target) = self.targets.get(index) else {
            return InjectDecision::Refused(Refusal::NoSelection);
        };

        if !target.is_likely_minecraft() {
            return InjectDecision::NeedsConfirmation(Refusal::UnidentifiedProcess);
        }

        InjectDecision::Accepted {
            target_index: index,
            pid: target.pid,
            version: self.selected_version(),
        }
    }

    /// The user answered the confirmation. `confirmed == false` keeps the dialog
    /// open, which is what the managed `MessageBoxResult.OK` check did.
    #[must_use]
    pub fn confirm_inject(&self, confirmed: bool) -> InjectDecision {
        if !confirmed {
            return InjectDecision::Refused(Refusal::UnidentifiedProcess);
        }

        match self.selected.and_then(|index| self.targets.get(index)) {
            Some(target) => InjectDecision::Accepted {
                target_index: self.selected.unwrap_or(0),
                pid: target.pid,
                version: self.selected_version(),
            },
            None => InjectDecision::Refused(Refusal::NoSelection),
        }
    }

    fn select_first(&mut self) {
        self.selected = if self.targets.is_empty() {
            None
        } else {
            Some(0)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_platform::ClientType;

    fn target(pid: u32, client: ClientType, confidence: u8) -> DiscoveryTarget {
        DiscoveryTarget {
            pid,
            window: None,
            title: String::new(),
            process_name: "javaw.exe".to_owned(),
            client_type: client,
            detected_version: None,
            detection_reason: "test".to_owned(),
            confidence,
        }
    }

    #[test]
    fn a_new_picker_selects_the_first_row_but_an_empty_list_selects_nothing() {
        let state = WindowPickerState::new(vec![
            target(1, ClientType::LunarClient, 95),
            target(2, ClientType::JavaProcess, 20),
        ]);
        assert_eq!(state.selected_index(), Some(0));

        let empty = WindowPickerState::new(Vec::new());
        assert_eq!(empty.selected_index(), None);
    }

    #[test]
    fn refreshing_replaces_the_list_and_resets_the_selection() {
        let mut state = WindowPickerState::new(vec![target(1, ClientType::LunarClient, 95)]);
        state.select(Some(0));

        state.refresh(vec![
            target(7, ClientType::Badlion, 90),
            target(8, ClientType::JavaProcess, 10),
        ]);

        assert_eq!(state.targets().len(), 2);
        assert_eq!(state.selected().map(|target| target.pid), Some(7));
    }

    #[test]
    fn an_out_of_range_selection_is_ignored_rather_than_clamped() {
        let mut state = WindowPickerState::new(vec![target(1, ClientType::LunarClient, 95)]);
        state.select(Some(9));
        assert_eq!(state.selected_index(), Some(0));
    }

    #[test]
    fn injecting_with_nothing_selected_asks_the_user_to_pick() {
        let state = WindowPickerState::new(Vec::new());
        let decision = state.request_inject();

        assert_eq!(decision, InjectDecision::Refused(Refusal::NoSelection));
        assert_eq!(Refusal::NoSelection.title(), "No process selected");
        assert_eq!(
            Refusal::NoSelection.message(),
            "Select a Java process from the list."
        );
    }

    #[test]
    fn a_recognised_client_injects_without_a_prompt() {
        let state = WindowPickerState::new(vec![target(42, ClientType::LunarClient, 95)]);

        assert_eq!(
            state.request_inject(),
            InjectDecision::Accepted {
                target_index: 0,
                pid: 42,
                version: "auto",
            }
        );
    }

    #[test]
    fn a_bare_jvm_asks_first_and_a_declined_prompt_injects_nothing() {
        let state = WindowPickerState::new(vec![target(7, ClientType::JavaProcess, 20)]);

        assert_eq!(
            state.request_inject(),
            InjectDecision::NeedsConfirmation(Refusal::UnidentifiedProcess)
        );
        assert_eq!(
            Refusal::UnidentifiedProcess.title(),
            "Unidentified Java process"
        );
        assert!(Refusal::UnidentifiedProcess
            .message()
            .contains("may fail if it is not a Minecraft client"));

        assert_eq!(
            state.confirm_inject(false),
            InjectDecision::Refused(Refusal::UnidentifiedProcess)
        );
        assert_eq!(
            state.confirm_inject(true),
            InjectDecision::Accepted {
                target_index: 0,
                pid: 7,
                version: "auto",
            }
        );
    }

    #[test]
    fn the_version_choices_are_the_managed_ones_in_order() {
        let state = WindowPickerState::new(Vec::new());

        let labels: Vec<&str> = state
            .version_options()
            .iter()
            .map(|(label, _)| *label)
            .collect();
        assert_eq!(
            labels,
            vec!["Auto-detect", "1.8.9", "1.21", "26.1", "26.2", "26.3"]
        );
        assert_eq!(
            state
                .version_options()
                .iter()
                .map(|(_, tag)| *tag)
                .collect::<Vec<_>>(),
            vec!["auto", "1.8.9", "1.21", "26.1", "26.2", "26.3"]
        );
    }

    #[test]
    fn the_chosen_version_reaches_the_decision() {
        let mut state = WindowPickerState::new(vec![target(42, ClientType::MinecraftJava, 80)]);

        state.set_version_index(2);
        assert_eq!(state.selected_version(), "1.21");
        assert_eq!(
            state.request_inject(),
            InjectDecision::Accepted {
                target_index: 0,
                pid: 42,
                version: "1.21",
            }
        );
    }

    #[test]
    fn an_out_of_range_version_falls_back_to_auto() {
        let mut state = WindowPickerState::new(Vec::new());

        state.set_version_index(99);
        assert_eq!(state.version_index(), 0);
        assert_eq!(state.selected_version(), "auto");
    }

    #[test]
    fn the_chrome_copy_matches_the_managed_dialog() {
        assert_eq!(TITLE, "Select Java client");
        assert_eq!(VERSION_LABEL, "Bridge version");
        assert_eq!(REFRESH_LABEL, "Refresh list");
        assert_eq!(CANCEL_LABEL, "Cancel");
        assert_eq!(INJECT_LABEL, "Inject into selected client");
        assert!(SUBTITLE.starts_with("Multiple Java/Minecraft processes were found."));
    }
}
