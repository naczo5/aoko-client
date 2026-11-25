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
        matches!(self, Self::Supported | Self::SupportedWithLibrary { .. })
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
    /// Global input **events** are delivered: a low-level hook is attached, or the
    /// evdev reader can open the input nodes.
    ///
    /// This is about the event stream only. It does **not** imply that current
    /// pointer and key state can be sampled - see
    /// [`CapabilityReport::pointer_state_sampling`], which is the flag that answers
    /// that question and the only one `probe_mouse_state`/`probe_key_state` agree
    /// with. On Wayland with readable `/dev/input` this is `true` while sampling is
    /// unavailable, because evdev needs no display but `XQueryPointer` does.
    pub input_observation: bool,
    /// The automation loops can sample current pointer and key state, i.e.
    /// [`crate::InputBackend::probe_mouse_state`] and
    /// [`crate::InputBackend::probe_key_state`] return `Some` rather than `None`.
    ///
    /// This is the flag a hold guard should consult. On Linux it needs an X11
    /// display; a Wayland-only session reports `false` even when
    /// [`CapabilityReport::input_observation`] is `true`, so the UI must not claim it
    /// can see a held button when the loops cannot.
    pub pointer_state_sampling: bool,
    /// Synthetic clicks, key presses and relative mouse movement.
    pub input_synthesis: bool,
    /// Discord IPC reachability.
    pub discord_rpc: bool,
    /// SRV resolution for a host entered without an explicit port.
    pub srv_resolution: bool,
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
            pointer_state_sampling: false,
            input_synthesis: false,
            discord_rpc: false,
            srv_resolution: true,
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
        assert!(BridgeSupport::SupportedWithLibrary {
            path: "b.so".into()
        }
        .is_usable());
        assert!(!BridgeSupport::UnsupportedOnLinuxYet.is_usable());
        assert!(!BridgeSupport::Blocked { detail: "x".into() }.is_usable());
    }

    #[test]
    fn transport_only_reports_every_os_service_off() {
        let report = CapabilityReport::transport_only(SessionKind::Wayland);
        assert_eq!(report.session, SessionKind::Wayland);
        assert!(!report.input_synthesis);
        assert!(!report.pointer_state_sampling);
        assert!(report.srv_resolution, "UDP needs no OS service");
        assert!(!report.bridge.is_usable());
        assert!(!report.notes.is_empty());
    }
}
