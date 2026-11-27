//! Linux platform implementation.
//!
//! Every capability is native: `/proc` for processes, X11 EWMH for windows,
//! evdev/uinput plus XTest for input and a unix socket for Discord. The native
//! bridge is the one thing that does not exist yet, and it says so through
//! [`BridgeSupport::UnsupportedOnLinuxYet`].
//!
//! Wayland is handled honestly rather than optimistically: an XWayland session
//! gets the X11 paths, and a Wayland-only session reports the capabilities it
//! cannot provide instead of returning empty results that look like "no game
//! running".

mod appdata;
mod discord;
mod evdev;
mod injector;
mod input;
mod open;
mod process;
mod srv;
mod uinput;
mod window;
mod x11;

pub use appdata::app_data_directory;
pub use discord::LinuxDiscordTransport;
pub use injector::LinuxBridgeInjector;
pub use input::LinuxInputBackend;
pub use open::{open_path, open_url, opener_available};
pub use process::{parse_proc_cmdline, LinuxProcessInspector};
pub use window::LinuxWindowInspector;

use crate::capabilities::{BridgeSupport, CapabilityReport, SessionKind};

/// Facts about the session, gathered once so the report is pure and testable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Detected {
    /// `DISPLAY` resolved and an X11 connection was established.
    pub x11: bool,
    /// A Wayland session is advertised (`WAYLAND_DISPLAY` or `XDG_SESSION_TYPE`).
    pub wayland: bool,
    /// procfs is mounted.
    pub procfs: bool,
    /// Readable input nodes that carry keys or relative motion.
    pub evdev_devices: usize,
    /// `/dev/uinput` is writable.
    pub uinput: bool,
    /// Discord IPC sockets that exist right now.
    pub discord_sockets: usize,
    /// Resolvers configured in `/etc/resolv.conf`.
    pub dns_servers: usize,
    /// An executable `xdg-open` is on `PATH`.
    pub file_manager: bool,
    /// An X11 connection exists *and* `XQueryPointer` answered, which is exactly what
    /// [`crate::InputBackend::probe_mouse_state`] gates on. This is deliberately not
    /// the same question as `evdev_devices > 0`: a Wayland session can deliver
    /// global events through evdev while no pointer state can be sampled.
    pub pointer_state: bool,
}

impl Detected {
    /// Probes the live session. Never panics and never blocks on a socket that a
    /// compositor forgot to accept.
    #[must_use]
    pub fn probe() -> Self {
        let connection = x11::X11::connect();
        let pointer_state = connection
            .as_ref()
            .is_ok_and(|connection| connection.pointer().is_some());
        let x11 = connection.is_ok();
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty())
            || std::env::var("XDG_SESSION_TYPE")
                .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"));

        Self {
            x11,
            pointer_state,
            wayland,
            procfs: std::path::Path::new("/proc/self/cmdline").exists(),
            evdev_devices: evdev::enumerate_devices().len(),
            uinput: uinput::is_available(),
            discord_sockets: LinuxDiscordTransport::available_socket_count(),
            dns_servers: srv::collect_system_dns_servers().len(),
            file_manager: opener_available(),
        }
    }

    /// Turns the probes into the report the UI renders banners from.
    #[must_use]
    pub fn report(&self) -> CapabilityReport {
        let session = if self.x11 {
            SessionKind::X11
        } else if self.wayland {
            SessionKind::Wayland
        } else {
            SessionKind::None
        };

        let mut notes = Vec::new();
        if !self.procfs {
            notes.push("/proc is not mounted, so process inspection is unavailable".to_string());
        }
        if !self.x11 {
            if self.wayland {
                notes.push(
                    "Wayland session without XWayland: window enumeration and client bounds need \
                     a compositor-specific IPC that GNOME and KDE do not offer"
                        .to_string(),
                );
            } else {
                notes.push(
                    "no X11 display found: set DISPLAY, or run inside a graphical session"
                        .to_string(),
                );
            }
        }
        if self.evdev_devices == 0 {
            notes.push(
                "global input observation needs read access to /dev/input/event*: add your user \
                 to the 'input' group and re-login"
                    .to_string(),
            );
        } else if !self.pointer_state {
            notes.push(
                "input events are observed through /dev/input, but held-button state needs an \
                 X11 display: without one the automation loops cannot tell a held button from \
                 a released one"
                    .to_string(),
            );
        }
        if !self.uinput && !self.x11 {
            notes.push(
                "input synthesis needs either X11 (XTest) or write access to /dev/uinput"
                    .to_string(),
            );
        }
        // A third question, distinct from "events are observed" and "state can be
        // sampled": no Linux session can say whether the pointer is drawn, so the
        // managed `GetCursorInfo` test has no counterpart here. Reported on every
        // session because the limit is a property of the platform, not of this one.
        notes.push(
            "cursor visibility cannot be queried on Linux: X11 has no request for it and \
             Wayland exposes nothing to clients, so a 'cursor hidden' test always answers \
             unknown here - do not read that as hidden, or the loops act on a guess"
                .to_string(),
        );
        if self.discord_sockets == 0 {
            notes.push("no Discord IPC socket found; Discord is probably not running".to_string());
        }
        if !self.file_manager {
            notes.push(
                "no xdg-open on PATH, so a folder cannot be revealed from here; install \
                 xdg-utils to enable the configs-folder button"
                    .to_string(),
            );
        }
        if self.dns_servers == 0 {
            notes.push(
                "no nameserver in /etc/resolv.conf; SRV lookups will ask the public resolvers \
                 8.8.8.8 and 1.1.1.1"
                    .to_string(),
            );
        }
        notes.push(
            "native bridge injection is unavailable on Linux: the shipped bridges are Windows DLLs"
                .to_string(),
        );

        CapabilityReport {
            session,
            process_inspection: self.procfs,
            window_inspection: self.x11,
            // Events and sampling are different questions: evdev reports whether the
            // event stream can be read, `pointer_state` whether the loops can sample a
            // held button. Conflating them is how a UI ends up claiming it can see a
            // mouse the clicker refuses to trust.
            input_observation: self.evdev_devices > 0,
            pointer_state_sampling: self.pointer_state,
            input_synthesis: self.x11 || self.uinput,
            discord_rpc: self.discord_sockets > 0,
            // UDP DNS needs no session at all, so a host with no display and no
            // input devices can still resolve a record.
            srv_resolution: true,
            open_path: self.file_manager,
            bridge: BridgeSupport::UnsupportedOnLinuxYet,
            notes,
        }
    }
}

/// The live session's capability report.
#[must_use]
pub fn probe_capabilities() -> CapabilityReport {
    Detected::probe().report()
}

/// Full service set for this OS.
#[must_use]
pub fn services() -> crate::PlatformServices {
    crate::PlatformServices {
        processes: Box::new(LinuxProcessInspector::new()),
        windows: Box::new(LinuxWindowInspector::new()),
        input: Box::new(LinuxInputBackend::new()),
        injector: Box::new(LinuxBridgeInjector::new()),
        discord: Box::new(LinuxDiscordTransport::new()),
        srv: crate::SrvResolver::new(srv::collect_system_dns_servers()),
        capabilities: probe_capabilities(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_x11_session_reports_view_and_input_capabilities() {
        let report = Detected {
            x11: true,
            wayland: false,
            procfs: true,
            evdev_devices: 3,
            uinput: false,
            discord_sockets: 1,
            dns_servers: 1,
            pointer_state: true,
            file_manager: true,
        }
        .report();

        assert_eq!(report.session, SessionKind::X11);
        assert!(report.process_inspection);
        assert!(report.window_inspection);
        assert!(report.input_observation);
        assert!(report.pointer_state_sampling);
        assert!(report.input_synthesis);
        assert!(report.discord_rpc);
        assert_eq!(report.bridge, BridgeSupport::UnsupportedOnLinuxYet);
        assert!(report.open_path, "the fake session has a launcher");
        // The bridge note is always present so the UI can explain the gap.
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("native bridge")));
    }

    #[test]
    fn a_wayland_session_with_readable_evdev_reports_events_without_sampling() {
        // The case that made one boolean a lie: evdev needs no display, so the event
        // stream works, while `XQueryPointer` needs X11, so a held button cannot be
        // sampled. The two flags must disagree here, deliberately.
        let report = Detected {
            x11: false,
            wayland: true,
            procfs: true,
            evdev_devices: 3,
            uinput: true,
            discord_sockets: 1,
            dns_servers: 1,
            pointer_state: false,
            file_manager: false,
        }
        .report();

        assert_eq!(report.session, SessionKind::Wayland);
        assert!(
            report.input_observation,
            "evdev delivers events without a display"
        );
        assert!(
            !report.pointer_state_sampling,
            "and nothing can be sampled, which is what the probes answer"
        );
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("held-button state needs an X11 display")));
    }

    #[test]
    fn a_wayland_only_session_reports_what_it_cannot_do() {
        let report = Detected {
            x11: false,
            wayland: true,
            procfs: true,
            evdev_devices: 0,
            uinput: false,
            discord_sockets: 0,
            dns_servers: 0,
            pointer_state: false,
            file_manager: false,
        }
        .report();

        assert_eq!(report.session, SessionKind::Wayland);
        assert!(report.process_inspection);
        assert!(!report.window_inspection);
        assert!(!report.input_observation);
        assert!(!report.pointer_state_sampling);
        assert!(!report.input_synthesis);
        assert!(!report.discord_rpc);
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("Wayland session without XWayland")));
        assert!(report
            .notes
            .iter()
            .any(|note| note.contains("/dev/input/event*")));
        assert!(report.notes.iter().any(|note| note.contains("/dev/uinput")));
    }

    #[test]
    fn a_headless_session_reports_nothing_but_procfs() {
        let report = Detected {
            procfs: true,
            ..Detected::default()
        }
        .report();

        assert_eq!(report.session, SessionKind::None);
        assert!(report.process_inspection);
        assert!(!report.window_inspection);
        assert!(!report.input_synthesis);
        assert!(report.notes.iter().any(|note| note.contains("DISPLAY")));
    }

    #[test]
    fn uinput_alone_still_enables_synthesis() {
        let report = Detected {
            wayland: true,
            procfs: true,
            uinput: true,
            ..Detected::default()
        }
        .report();

        assert!(report.input_synthesis);
        assert!(!report
            .notes
            .iter()
            .any(|note| note.contains("input synthesis needs")));
    }

    #[test]
    fn xwayland_prefers_the_x11_backend() {
        let report = Detected {
            x11: true,
            wayland: true,
            procfs: true,
            ..Detected::default()
        }
        .report();
        assert_eq!(report.session, SessionKind::X11);
        assert!(report.window_inspection);
    }

    #[test]
    fn every_report_names_the_unobservable_cursor_as_a_platform_limit() {
        // `WindowInspector::cursor_visible` returns `None` on Linux, so the report
        // has to carry the reason on any session, X11 or not.
        for detected in [
            Detected::default(),
            Detected {
                x11: true,
                procfs: true,
                pointer_state: true,
                ..Detected::default()
            },
            Detected {
                wayland: true,
                procfs: true,
                ..Detected::default()
            },
        ] {
            let report = detected.report();
            assert!(
                report
                    .notes
                    .iter()
                    .any(|note| note.contains("cursor visibility cannot be queried")),
                "a {:?} session must name the cursor limit",
                report.session
            );
        }
    }

    #[test]
    fn the_live_report_is_self_consistent() {
        let report = probe_capabilities();
        // No capability may be claimed without the note that explains its absence.
        assert!(!report.notes.is_empty());
        if !report.window_inspection {
            assert!(report.session != SessionKind::X11);
        }
        if report.session == SessionKind::Windows {
            panic!("a Linux build must not report a Windows session");
        }
    }

    #[test]
    fn a_sampling_claim_is_backed_by_the_sampler_itself() {
        use crate::InputBackend as _;

        // The report and the backend are built independently; if the report says a
        // held button is visible, `probe_mouse_state` must agree rather than deny it.
        let report = Detected::probe().report();
        let backend = LinuxInputBackend::new();
        if report.pointer_state_sampling {
            assert!(
                backend.probe_mouse_state().is_some(),
                "the report claimed sampling while the sampler answered None"
            );
        }
        if backend.probe_mouse_state().is_none() {
            assert!(
                !report.pointer_state_sampling,
                "the sampler answered None while the report claimed sampling"
            );
        }
    }

    #[test]
    fn services_expose_every_trait_object() {
        let mut services = services();
        // Building the service set must not panic and must be internally
        // consistent with a fresh probe.
        assert_eq!(services.capabilities.session, probe_capabilities().session);
        let _ = services.processes.list_processes();
        let _ = services.windows.enumerate();
        let _ = services.input.mouse_state();
        let _ = services.discord.write_frame(0, b"{}");
        assert!(!services.injector.support().is_usable());
    }
}
