//! The runtime owner: the one `AppController` the shell reads and writes, plus the
//! optional bridge reader that feeds it.
//!
//! Before this module, `aoko_core::app::AppController` had no reference outside
//! `aoko-core`. The shell kept its own `ModuleToggles`, so every module switch wrote a
//! set that nothing else in the process could see: 43 switches, no runtime on the other
//! end, and `TcpBridgeTransport::connect_bridge()` with no caller.
//!
//! [`crate::runtime::Runtime`] closes that gap for module state only, which is this slice's whole scope.
//! It owns the `Arc<AppController>` and resolves the switches' catalog ids onto the
//! controller's own flag set (`AppController::is_module_enabled` /
//! `set_module_enabled`), so there is **one** writable copy of module state in the app.
//! `aoko_core::clicker::toggles::ModuleToggles` is still the clicker loop's type inside
//! `aoko-core`, but `aoko-app` constructs none: a grep for `ModuleToggles` in this crate
//! finds no instance.
//!
//! The bridge link is the second half: `crate::runtime::Runtime::connect_bridge` hands the
//! controller to a `crate::ingest::Ingest` worker, which decodes wire lines and publishes
//! `GameState` into the store, and `crate::runtime::Runtime::ingest_status` reports what
//! that store holds. Nothing dials by itself — the shipped path asks for it once, a
//! test-built runtime never does — so a switch flip is still observable with no bridge
//! present.
//!
//! Two constructors, deliberately:
//!
//! * [`crate::runtime::Runtime::probed`] asks the platform what this session can do
//!   (`aoko_platform::probe_capabilities`) and is the shipped path, called from
//!   [`crate::shell::Shell::from_system`] where a window exists.
//! * [`crate::runtime::Runtime::headless`] takes the default report, so a unit test and the capture
//!   harness never depend on the machine they run on.
//!
//! ```text
//! let runtime = Runtime::headless();
//! runtime.set_module_enabled("autototem", true);
//! assert!(runtime.controller().is_module_enabled(EnabledFlag::AutoTotem));
//! ```

use std::sync::Arc;
use std::time::Instant;

use aoko_core::app::AppController;
use aoko_core::clicker::toggles;
use aoko_protocol::module_catalog::{self, EnabledFlag};

use crate::ingest::{Ingest, IngestStatus};

/// The application's runtime state: the controller, plus the id mapping the switches
/// need to reach it.
pub struct Runtime {
    controller: Arc<AppController>,
    /// The bridge reader, `None` until `connect_bridge` is called and on every
    /// runtime a test builds.
    ingest: Option<Ingest>,
}

impl Runtime {
    /// A runtime over a controller the caller built.
    ///
    /// This is the seam a test uses to hold the same controller the UI writes through.
    /// It builds no bridge reader: `connect_bridge` is the only thing that dials.
    #[must_use]
    pub fn new(controller: Arc<AppController>) -> Self {
        Self {
            controller,
            ingest: None,
        }
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

    /// Starts the bridge reader on its own thread, once.
    ///
    /// Idempotent so a startup path that runs twice (a reload, a second window) cannot
    /// attach two readers to one store and interleave their `connected` writes. The dial
    /// itself happens on the worker (`crate::ingest::Ingest::spawn`), so this call returns
    /// without waiting for a bridge that is not there.
    pub fn connect_bridge(&mut self) {
        if self.ingest.is_some() {
            return;
        }
        self.ingest = Some(Ingest::spawn(Arc::clone(&self.controller)));
    }

    /// What the bridge link currently reports.
    ///
    /// Read out of the controller's store, so it is the same truth the rest of the app
    /// sees; nothing in this crate keeps a second "connected" flag. `fresh` is
    /// `aoko_core::app::UiSnapshot::is_state_fresh`, i.e. the age is inside
    /// `aoko_core::chest_stealer::FRESH_STATE_MS`.
    #[must_use]
    pub fn ingest_status(&self) -> IngestStatus {
        let now = Instant::now();
        let store = self.controller.store();
        IngestStatus {
            connected: store.is_connected(),
            state_age: store.age(now),
            fresh: self.controller.ui_snapshot(now).is_state_fresh(),
        }
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
    use std::time::Duration;

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

    /// A runtime a test builds must not touch the network, and must say so rather than
    /// waiting on a dial.
    ///
    /// The structural half is the decisive one: `ingest` is `None`, so no reader thread
    /// exists that could connect later, and `connect_bridge` is the only thing that
    /// starts one — the shipped path, never this constructor. The elapsed bound is the
    /// only way to observe "does not block" directly: on a machine with a bridge, a
    /// synchronous dial would succeed, and on one without it, a refused connect could
    /// still stall a loaded loopback stack. Two seconds is far above either.
    #[test]
    fn a_headless_runtime_opens_no_socket() {
        let started = Instant::now();
        let runtime = Runtime::headless();
        let status = runtime.ingest_status();

        assert!(runtime.ingest.is_none(), "headless builds no bridge reader");
        assert!(!status.connected, "nothing connected, nothing claimed");
        assert_eq!(status.state_age, None, "no frame has arrived");
        assert!(!status.fresh, "a state that does not exist is not fresh");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "constructing a runtime must not dial"
        );
    }

    /// The status is the store's own truth, and freshness is measured from the frame's
    /// arrival instant rather than from when the question is asked.
    #[test]
    fn status_reports_what_the_store_holds() {
        use aoko_protocol::state::GameState;

        let runtime = Runtime::headless();
        let store = runtime.controller().store();

        store.set_connected(true);
        // Stamped in the future, so the age saturates to zero: the fresh branch is then a
        // fact rather than a race against the 300 ms window.
        store.publish(
            GameState::default(),
            Instant::now() + Duration::from_secs(1),
        );

        let status = runtime.ingest_status();
        assert!(status.connected);
        assert_eq!(status.state_age, Some(Duration::ZERO));
        assert!(status.fresh);

        // A second in the past is outside the window whatever the scheduler does.
        store.publish(
            GameState::default(),
            Instant::now() - Duration::from_secs(1),
        );

        let status = runtime.ingest_status();
        assert!(
            status
                .state_age
                .is_some_and(|age| age >= Duration::from_secs(1)),
            "the age is measured from the publish instant"
        );
        assert!(!status.fresh, "an old frame is stale by definition");
    }
}
