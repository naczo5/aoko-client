//! The runtime owner: the one `AppController` the shell reads and writes.
//!
//! Before this module, `aoko_core::app::AppController` had no reference outside
//! `aoko-core`. The shell kept its own `ModuleToggles`, so every module switch wrote a
//! set that nothing else in the process could see: 43 switches, no runtime on the other
//! end, and `TcpBridgeTransport::connect_bridge()` with no caller.
//!
//! [`Runtime`] closes that gap for module state only, which is this slice's whole scope.
//! It owns the `Arc<AppController>` and resolves the switches' catalog ids onto the
//! controller's own flag set (`AppController::is_module_enabled` /
//! `set_module_enabled`), so there is **one** writable copy of module state in the app.
//! `aoko_core::clicker::toggles::ModuleToggles` is still the clicker loop's type inside
//! `aoko-core`, but `aoko-app` constructs none: a grep for `ModuleToggles` in this crate
//! finds no instance.
//!
//! Nothing here opens a socket, starts a thread or drives a loop. A switch flip is
//! observable through the controller with no bridge present, which is the property the
//! slice asked to be proved.
//!
//! Two constructors, deliberately:
//!
//! * [`Runtime::probed`] asks the platform what this session can do
//!   (`aoko_platform::probe_capabilities`) and is the shipped path, called from
//!   [`crate::shell::Shell::from_system`] where a window exists.
//! * [`Runtime::headless`] takes the default report, so a unit test and the capture
//!   harness never depend on the machine they run on.
//!
//! ```text
//! let runtime = Runtime::headless();
//! runtime.set_module_enabled("autototem", true);
//! assert!(runtime.controller().is_module_enabled(EnabledFlag::AutoTotem));
//! ```

use std::sync::Arc;

use aoko_core::app::AppController;
use aoko_core::clicker::toggles;
use aoko_protocol::module_catalog::{self, EnabledFlag};

/// The application's runtime state: the controller, plus the id mapping the switches
/// need to reach it.
pub struct Runtime {
    controller: Arc<AppController>,
}

impl Runtime {
    /// A runtime over a controller the caller built.
    ///
    /// This is the seam a test uses to hold the same controller the UI writes through.
    #[must_use]
    pub fn new(controller: Arc<AppController>) -> Self {
        Self { controller }
    }

    /// A runtime that probes nothing.
    ///
    /// `AppController::default` is built over `CapabilityReport::transport_only` with
    /// `SessionKind::None` (`app.rs:315-319`), so the report says outright that no session
    /// was discovered rather than being filled in from whatever machine the suite happens
    /// to run on.
    #[must_use]
    pub fn headless() -> Self {
        Self::new(Arc::new(AppController::default()))
    }

    /// A runtime over the real capability report.
    ///
    /// Only [`crate::shell::Shell::from_system`] calls this: it is the one path where a
    /// window exists and the probe's answer is meaningful.
    #[must_use]
    pub fn probed() -> Self {
        Self::new(Arc::new(AppController::new(
            aoko_platform::probe_capabilities(),
        )))
    }

    /// The controller this runtime owns.
    ///
    /// Cloning the `Arc` gives a transport owner or a loop the same state the switches
    /// write, without going through the view.
    #[must_use]
    pub fn controller(&self) -> &Arc<AppController> {
        &self.controller
    }

    /// Whether one module's switch is on, by catalog id (case-insensitive).
    ///
    /// A module id the catalog does not have, or one with no boolean behind it, reads
    /// as off.
    #[must_use]
    pub fn module_enabled(&self, module_id: &str) -> bool {
        flag_for(module_id).is_some_and(|flag| self.controller.is_module_enabled(flag))
    }

    /// Sets one module's switch, by catalog id (case-insensitive).
    ///
    /// An unknown id is ignored, and so is `panic`: the catalog maps it to
    /// [`EnabledFlag::Never`] because it is a keybind-only action with no stored
    /// boolean, which is the same reason `count_enabled_modules` skips it.
    pub fn set_module_enabled(&self, module_id: &str, enabled: bool) {
        if let Some(flag) = flag_for(module_id) {
            self.controller.set_module_enabled(flag, enabled);
        }
    }

    /// Flips one module's switch, returning whether the id named a module with a
    /// boolean behind it.
    ///
    /// The return value is what makes `cx.notify()` honest: a click on a row that has no
    /// state to change does not repaint the window.
    pub fn flip_module(&self, module_id: &str) -> bool {
        let Some(flag) = flag_for(module_id) else {
            return false;
        };
        let enabled = !self.controller.is_module_enabled(flag);
        self.controller.set_module_enabled(flag, enabled);
        true
    }

    /// `Clicker.IsArmed`.
    ///
    /// The catalog models the AutoClicker's armed state as
    /// [`EnabledFlag::LeftClick`]: its entry's overlay probe is `cfg.armed`
    /// (`module_catalog.rs:228-233`), which is the string the injected bridge reports
    /// for `IsArmed`. There is no separate armed bit on the controller, so this flag is
    /// the only honest source and a second copy would be exactly the split this slice
    /// removes.
    #[must_use]
    pub fn armed(&self) -> bool {
        self.controller.is_module_enabled(EnabledFlag::LeftClick)
    }

    /// How many catalog modules are on, which is what the title chip counts.
    ///
    /// Counted from the catalog rather than from `AppController::count_enabled_modules`:
    /// that method is `ModuleCatalog.CountEnabledForDiscord` and answers a different
    /// question (Discord-RPC surface, dev-mode visibility), and this number is shown to
    /// the user as "N on".
    #[must_use]
    pub fn enabled_count(&self) -> usize {
        module_catalog::ALL
            .iter()
            .filter(|entry| entry.enabled_flag != EnabledFlag::Never)
            .filter(|entry| self.controller.is_module_enabled(entry.enabled_flag))
            .count()
    }
}

/// The controller flag behind a module id.
///
/// `None` for an id the catalog does not have, and for the entries whose flag is
/// [`EnabledFlag::Never`] — `panic` is the only one — because those have no stored
/// boolean to read or write.
fn flag_for(module_id: &str) -> Option<EnabledFlag> {
    let canonical = toggles::canonical_id(module_id)?;
    module_catalog::ALL
        .iter()
        .find(|entry| entry.id == canonical)
        .map(|entry| entry.enabled_flag)
        .filter(|flag| *flag != EnabledFlag::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suite must not depend on the machine: a headless runtime reports that it
    /// probed nothing, and its module state still works.
    #[test]
    fn a_headless_runtime_probes_nothing_and_still_holds_state() {
        let runtime = Runtime::headless();

        // `AppController::default` is `transport_only(SessionKind::None)`. A probed
        // report on this machine would name X11 or Wayland instead.
        assert_eq!(
            runtime.controller().capabilities().session,
            aoko_platform::SessionKind::None
        );

        assert!(!runtime.module_enabled("autototem"));
        runtime.set_module_enabled("autototem", true);
        assert!(runtime.module_enabled("autototem"));
        assert!(runtime
            .controller()
            .is_module_enabled(EnabledFlag::AutoTotem));
        assert_eq!(runtime.enabled_count(), 1);

        runtime.set_module_enabled("AutoTotem", false);
        assert!(!runtime.module_enabled("autototem"));
        assert_eq!(runtime.enabled_count(), 0);
    }

    /// The AutoClicker row is the armed bit, not an ordinary module, and flipping it
    /// reaches the controller's `LeftClick` flag.
    #[test]
    fn the_autoclicker_switch_is_the_armed_flag() {
        let runtime = Runtime::headless();

        assert!(!runtime.armed());
        assert!(runtime.flip_module("autoclicker"));
        assert!(runtime.armed());
        assert!(runtime.module_enabled("autoclicker"));
        assert!(runtime
            .controller()
            .is_module_enabled(EnabledFlag::LeftClick));

        assert!(runtime.flip_module("AUTOCLICKER"));
        assert!(!runtime.armed());
        assert!(!runtime
            .controller()
            .is_module_enabled(EnabledFlag::LeftClick));
    }

    /// `panic` has no boolean (`EnabledFlag::Never`), and an unknown id names nothing:
    /// both are ignored, and neither of them reports a change that would repaint.
    #[test]
    fn ids_without_a_boolean_are_ignored() {
        let runtime = Runtime::headless();

        runtime.set_module_enabled("panic", true);
        assert!(!runtime.module_enabled("panic"));
        assert!(!runtime.flip_module("panic"));
        assert!(!runtime.flip_module("no-such-module"));
        assert_eq!(runtime.enabled_count(), 0);
    }

    /// Ids are matched case-insensitively, as `canonical_id` is, and a key that is not
    /// a catalog module is refused.
    ///
    /// The second half is a narrowing worth stating: `ModuleToggles::set_enabled`
    /// accepted non-catalog keys so the panic reset could clear the AutoClicker
    /// sub-settings, but both of the shell's write paths canonicalised their id first, so
    /// no switch ever wrote one. The controller stores catalog flags only, and this view
    /// refuses what it cannot represent rather than pretending the write landed.
    #[test]
    fn ids_are_case_insensitive_and_only_catalog_modules_are_written() {
        let runtime = Runtime::headless();

        runtime.set_module_enabled("ClickInChests", true);
        assert!(runtime.module_enabled("clickinchests"));

        runtime.set_module_enabled("pixelpartyautolook", true);
        assert!(!runtime.module_enabled("pixelpartyautolook"));
        assert_eq!(runtime.enabled_count(), 1);
    }
}
