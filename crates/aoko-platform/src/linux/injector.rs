//! Native bridge injection on Linux.
//!
//! There is no Linux bridge yet. `McInjector/` produces two Windows DLLs pinned
//! to the JVM's Win32 JNI surface, and `MIGRATION_ARCHITECTURE.md` records "a
//! Linux native bridge is a trait stub, by design" as an accepted trade-off, so
//! this implementation reports [`BridgeSupport::UnsupportedOnLinuxYet`] up front
//! instead of failing at injection time.
//!
//! The host stays fully usable without it: every other capability is independent
//! and the UI must treat "no game state" as a normal state.

use std::path::Path;

use crate::{BridgeSupport, InjectionReport, NativeBridgeInjector, PlatformError};

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxBridgeInjector;

impl LinuxBridgeInjector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl NativeBridgeInjector for LinuxBridgeInjector {
    fn inject(&self, pid: u32, library: &Path) -> Result<InjectionReport, PlatformError> {
        // Reported as "unsupported", not "denied": nothing about the session would
        // change the answer, and retrying helps nobody.
        Err(PlatformError::unsupported(format!(
            "cannot load {} into pid {pid}: no Linux bridge exists in this build \
             (the shipped bridges are Windows DLLs; see MIGRATION_ARCHITECTURE.md)",
            library.display()
        )))
    }

    fn support(&self) -> BridgeSupport {
        BridgeSupport::UnsupportedOnLinuxYet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_is_reported_before_any_call() {
        let injector = LinuxBridgeInjector::new();
        assert_eq!(injector.support(), BridgeSupport::UnsupportedOnLinuxYet);
        assert!(!injector.support().is_usable());
    }

    #[test]
    fn injection_reports_unsupported_with_the_library_in_the_message() {
        let injector = LinuxBridgeInjector::new();
        let err = injector
            .inject(4242, Path::new("/tmp/libaoko_bridge.so"))
            .expect_err("no linux bridge");
        assert!(matches!(err, PlatformError::Unsupported { .. }));
        let message = err.user_facing();
        assert!(message.contains("libaoko_bridge.so"));
        assert!(message.contains("4242"));
    }
}
