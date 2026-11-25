//! Runtime capability reporting.
//!
//! The UI renders banners from this, so adding a capability must not change
//! call sites: extend [`CapabilityReport`] rather than adding trait methods.

/// Whether the native bridge can be injected at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSupport {
    /// Full support, e.g. Windows `LoadLibrary` injection.
    Supported,
    /// A Linux bridge library is configured and loadable.
    SupportedWithLibrary { path: String },
    /// Linux: no `.so` bridge exists yet. The host stays fully usable.
    UnsupportedOnLinuxYet,
    /// Present but blocked by the session (permissions, protected target).
    Blocked { detail: String },
}

impl BridgeSupport {
    #[must_use]
    pub fn is_usable(&self) -> bool {
        matches!(
            self,
            Self::Supported | Self::SupportedWithLibrary { .. }
        )
    }
}

/// Which desktop session the process is actually talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Windows,
    X11,
    Wayland,
    /// No display server discovered (headless or pure TTY).
    None,
}

/// Everything the UI needs to explain what works in this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityReport {
    pub session: SessionKind,
    /// Process enumeration and full command lines.
    pub process_inspection: bool,
    /// Window enumeration, including foreground and client bounds.
    pub window_inspection: bool,
    /// Global keyboard/mouse state and hook events.
    pub input_observation: bool,
    /// Synthetic clicks, key presses and relative mouse movement.
    pub input_synthesis: bool,
    /// Discord IPC reachability.
    pub discord_rpc: bool,
    /// Native bridge injection status.
    pub bridge: BridgeSupport,
    /// Human-readable notes explaining any `false` above.
    pub notes: Vec<String>,
}

impl CapabilityReport {
    /// A report for a host that can talk TCP but has no OS services yet.
    #[must_use]
    pub fn transport_only(session: SessionKind) -> Self {
        Self {
            session,
            process_inspection: false,
            window_inspection: false,
            input_observation: false,
            input_synthesis: false,
            discord_rpc: false,
            bridge: BridgeSupport::UnsupportedOnLinuxYet,
            notes: vec!["OS services unavailable in this build".to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_supported_variants_are_usable() {
        assert!(BridgeSupport::Supported.is_usable());
        assert!(BridgeSupport::SupportedWithLibrary { path: "b.so".into() }.is_usable());
        assert!(!BridgeSupport::UnsupportedOnLinuxYet.is_usable());
        assert!(!BridgeSupport::Blocked { detail: "x".into() }.is_usable());
    }

    #[test]
    fn transport_only_reports_every_os_service_off() {
        let report = CapabilityReport::transport_only(SessionKind::Wayland);
        assert_eq!(report.session, SessionKind::Wayland);
        assert!(!report.input_synthesis);
        assert!(!report.bridge.is_usable());
        assert!(!report.notes.is_empty());
    }
}
