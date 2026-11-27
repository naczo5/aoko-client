//! Windows platform implementation.
//!
//! Full parity with the managed host: ToolHelp process enumeration plus PEB
//! command lines, `EnumWindows`/`GetClientRect` geometry, low-level input hooks
//! with `SendInput` synthesis, `LoadLibrary` bridge injection and named-pipe
//! Discord IPC.

mod appdata;
mod discord;
mod handles;
mod injector;
mod input;
mod open;
mod process;
mod raw;
mod srv;
mod window;

pub use appdata::app_data_directory;
pub use discord::WindowsDiscordTransport;
pub use injector::WindowsBridgeInjector;
pub use input::{install_hooks, uninstall_hooks, HookGuard, WindowsInputBackend};
pub use open::{open_path, open_url, opener_available};
pub use process::{read_command_line, WindowsProcessInspector};
pub use window::{process_image_name, window_title, WindowsWindowInspector};

use crate::capabilities::{BridgeSupport, CapabilityReport, SessionKind};

/// Windows exposes every capability this host needs; only Discord and injection
/// can be missing at runtime, and each reports that itself.
#[must_use]
pub fn probe_capabilities() -> CapabilityReport {
    let discord_pipes = WindowsDiscordTransport::available_pipe_count();
    let dns_servers = srv::collect_system_dns_servers().len();
    let mut notes = Vec::new();

    if discord_pipes == 0 {
        notes.push("no Discord IPC pipe found; Discord is probably not running".to_string());
    }
    if dns_servers == 0 {
        notes.push(
            "no DNS server on an active adapter; SRV lookups will ask the public resolvers \
             8.8.8.8 and 1.1.1.1"
                .to_string(),
        );
    }
    notes.push(
        "bridge injection needs the host and the game at the same integrity level".to_string(),
    );

    CapabilityReport {
        session: SessionKind::Windows,
        process_inspection: true,
        window_inspection: true,
        input_observation: true,
        // `GetAsyncKeyState` always has an answer, so sampling is part of the same
        // claim on Windows.
        pointer_state_sampling: true,
        input_synthesis: true,
        discord_rpc: discord_pipes > 0,
        srv_resolution: true,
        // `shell32` is always loaded in an interactive session.
        open_path: opener_available(),
        bridge: BridgeSupport::Supported,
        notes,
    }
}

/// Full service set for this OS.
#[must_use]
pub fn services() -> crate::PlatformServices {
    crate::PlatformServices {
        processes: Box::new(WindowsProcessInspector::new()),
        windows: Box::new(WindowsWindowInspector::new()),
        input: Box::new(WindowsInputBackend::new()),
        injector: Box::new(WindowsBridgeInjector::new()),
        discord: Box::new(WindowsDiscordTransport::new()),
        srv: crate::SrvResolver::new(srv::collect_system_dns_servers()),
        capabilities: probe_capabilities(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_windows_capability_is_reported_available() {
        let report = probe_capabilities();
        assert_eq!(report.session, SessionKind::Windows);
        assert!(report.process_inspection);
        assert!(report.window_inspection);
        assert!(report.input_observation);
        assert!(report.pointer_state_sampling);
        assert!(report.input_synthesis);
        assert!(report.bridge.is_usable());
    }

    #[test]
    fn the_bridge_support_is_the_load_library_path() {
        assert_eq!(
            crate::NativeBridgeInjector::support(&WindowsBridgeInjector::new()),
            BridgeSupport::Supported
        );
    }

    #[test]
    fn services_expose_every_trait_object() {
        let services = services();
        assert_eq!(services.capabilities.session, SessionKind::Windows);
        let _ = services.processes.list_processes();
        let _ = services.windows.enumerate();
        let _ = services.input.mouse_state();
        assert!(services.injector.support().is_usable());
    }
}
