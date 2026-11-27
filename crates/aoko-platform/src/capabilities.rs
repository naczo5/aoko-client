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
    /// This is about the event stream only. It does **not** say whether a held button can be
    /// sampled - see [`CapabilityReport::pointer_state_sampling`], which is the flag a hold
    /// guard reads and the one [`crate::InputBackend::probe_mouse_state`] agrees with.
    pub input_observation: bool,
    /// The automation loops can sample the current pointer state: a hold guard can tell a held
    /// button from a released one, because [`crate::InputBackend::probe_mouse_state`] returns
    /// `Some` rather than `None`.
    ///
    /// This is the flag a hold guard should consult. On Linux the button half needs no display:
    /// the evdev reader latches the user's own button transitions whenever it can open an input
    /// node, and the X11 pointer mask answers only when it is watching none. A session with
    /// neither - a Wayland-only desktop whose `/dev/input` nodes are unreadable - reports
    /// `false`, so the UI must not claim it can see a held button when the loops cannot.
    ///
    /// A held *key* is the narrower question, and it is the one that still needs a display on
    /// Linux: [`crate::InputBackend::probe_key_state`] reads the `XQueryKeymap` bitmap, so a
    /// Wayland session with readable evdev reports this flag `true` while a per-key probe
    /// answers `None`.
    pub pointer_state_sampling: bool,
    /// Synthetic clicks, key presses, relative mouse movement, and absolute pointer placement
    /// ([`crate::InputBackend::move_to`], which the chest stealer needs to land a slot click).
    ///
    /// One flag for the whole surface, because a session that cannot synthesize must not claim
    /// part of it: absolute placement is the one operation that can still refuse on its own, so
    /// a session whose only synthesis is `/dev/uinput` reports this `true` and answers
    /// [`crate::InputBackend::move_to`] with `Unsupported` per call.
    pub input_synthesis: bool,
    /// Discord IPC reachability.
    pub discord_rpc: bool,
    /// SRV resolution for a host entered without an explicit port.
    pub srv_resolution: bool,
    /// A path can be revealed in the OS file manager, i.e. [`crate::open_path`] has a
    /// launcher to call.
    ///
    /// Linux needs `xdg-open` on `PATH`; Windows always has the shell.
    pub open_path: bool,
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
            open_path: false,
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
        assert!(!report.open_path, "and no file manager either");
        assert!(!report.bridge.is_usable());
        assert!(!report.notes.is_empty());
    }
}
