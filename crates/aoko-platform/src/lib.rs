//! OS abstraction layer.
//!
//! Every capability is a trait with a Windows and a Linux implementation.
//! Callers never branch on `cfg`; they consume the traits and read
//! [`CapabilityReport`] to decide what to enable in the UI.
//!
//! The design rule behind this module: a missing OS capability must surface as
//! an explicit, actionable diagnostic, never as a panic, a silent no-op, or a
//! pretend-success.

use std::fmt;
use std::path::Path;

mod bridge;
mod capabilities;
mod discord;
mod error;
mod input;
mod netstat;
mod open;
mod process;
mod profile;
mod srv;
mod tcp;
mod version;
mod window;

#[cfg(target_os = "linux")]
mod evdev_table;

#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

pub use bridge::{bridge_library_path, host_executable_dir, inject_bridge, resolve_bridge_library};
pub use capabilities::{BridgeSupport, CapabilityReport, SessionKind};
pub use discord::{
    handshake_payload, op, read_frame_from, write_frame_to, DiscordFrame, DiscordRpcTransport,
    DiscordTransportError, StreamDiscordTransport, FRAME_HEADER_LEN, MAX_FRAME_PAYLOAD,
};
pub use error::PlatformError;
pub use input::{InputBackend, InputEvent, Key, MouseButton, MouseState};
pub use netstat::{protocol_listener_pid, NetstatLineParser, TcpPortProbe};
pub use open::{open_path, open_url};
pub use process::{
    compare_ignore_case, describe_target, is_java_process_name, select_best_window, sort_targets,
    ClientType, DiscoveryTarget, InstanceCandidate, ProcessInfo, ProcessInspector,
};
pub use profile::{app_data_directory, ProfileStore};
pub use srv::{
    Clock as SrvClock, SrvExchange, SrvLookupCache, SrvResolver, SrvTarget, UdpExchange, DNS_PORT,
    SRV_FAILURE_TTL, SRV_SUCCESS_TTL, SRV_TIMEOUT,
};
pub use tcp::{TcpBridgeTransport, TcpBridgeWriter, BRIDGE_ADDRESS, SEND_TIMEOUT};
pub use version::{normalize_detected_version, resolve_bridge_dll_name, try_parse_version};
pub use window::{is_game_window, sort_windows, Rect, WindowHandle, WindowInfo, WindowInspector};

#[cfg(target_os = "linux")]
pub use evdev_table::{code_for_key, key_from_code};

#[cfg(windows)]
pub use windows::{
    install_hooks, process_image_name, read_command_line, uninstall_hooks, window_title, HookGuard,
    WindowsBridgeInjector, WindowsDiscordTransport, WindowsInputBackend, WindowsProcessInspector,
    WindowsWindowInspector,
};

#[cfg(target_os = "linux")]
pub use linux::{
    parse_proc_cmdline, Detected, LinuxBridgeInjector, LinuxDiscordTransport, LinuxInputBackend,
    LinuxProcessInspector, LinuxWindowInspector,
};

/// Which desktop backend a Linux session is actually using.
///
/// Reported rather than assumed: a Wayland session with XWayland looks like X11
/// to some APIs and like Wayland to others, and global input behaves
/// differently on each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    LinuxX11,
    LinuxWayland,
}

#[must_use]
pub const fn compiled_platform() -> Platform {
    #[cfg(windows)]
    {
        Platform::Windows
    }
    #[cfg(target_os = "linux")]
    {
        // Resolved at runtime by the window backend; this is the compile-time floor.
        Platform::LinuxX11
    }
}

/// Injects a native bridge library into a target process.
///
/// On Windows this is `LoadLibrary` inside the target PID, matching
/// `Aoko/Core/NativeInjector.cs`. On Linux there is no bridge yet, so the
/// implementation must report [`BridgeSupport::UnsupportedOnLinuxYet`] instead
/// of failing at call time, and its `inject` answers
/// [`PlatformError::Unsupported`] rather than a panic.
///
/// `library` comes from [`bridge_library_path`], which maps the detected game
/// version onto the name the managed build shipped beside its executable.
/// [`inject_bridge`] is the two together - resolve, then attach - and is what a
/// shell should call, so a name that is not on disk cannot reach this method.
pub trait NativeBridgeInjector: Send + Sync {
    /// Attaches and loads `library` into `pid`.
    fn inject(&self, pid: u32, library: &Path) -> Result<InjectionReport, PlatformError>;

    /// Whether injection can work at all in this build and session.
    fn support(&self) -> BridgeSupport;
}

/// Result of a successful injection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionReport {
    pub pid: u32,
    pub library: String,
    /// Remote address the loaded module was mapped at, when known.
    pub remote_base: Option<usize>,
}

impl fmt::Display for InjectionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.remote_base {
            Some(base) => write!(
                f,
                "injected {} into pid {} at {base:#x}",
                self.library, self.pid
            ),
            None => write!(f, "injected {} into pid {}", self.library, self.pid),
        }
    }
}

/// Bundles every platform service so the app wires one value, not six.
pub struct PlatformServices {
    pub processes: Box<dyn ProcessInspector>,
    pub windows: Box<dyn WindowInspector>,
    pub input: Box<dyn InputBackend>,
    pub injector: Box<dyn NativeBridgeInjector>,
    pub discord: Box<dyn DiscordRpcTransport>,
    /// SRV resolution for a host entered without an explicit port.
    pub srv: SrvResolver,
    pub capabilities: CapabilityReport,
}

/// Probes what this session can actually do.
///
/// Constructing a service set calls this, so a UI never has to guess why a
/// control is disabled.
#[must_use]
pub fn probe_capabilities() -> CapabilityReport {
    #[cfg(windows)]
    {
        windows::probe_capabilities()
    }
    #[cfg(target_os = "linux")]
    {
        linux::probe_capabilities()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        CapabilityReport::transport_only(SessionKind::None)
    }
}

/// Full service set for this OS: processes, windows, input, injector, Discord.
#[must_use]
pub fn default_services() -> PlatformServices {
    #[cfg(windows)]
    {
        windows::services()
    }
    #[cfg(target_os = "linux")]
    {
        linux::services()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        // Unsupported targets keep the transport and report every OS service off,
        // so cross-compiles of the workspace still link.
        PlatformServices {
            processes: Box::new(UnsupportedProcesses),
            windows: Box::new(UnsupportedWindows),
            input: Box::new(UnsupportedInput),
            injector: Box::new(UnsupportedInjector),
            discord: Box::new(UnsupportedDiscord),
            // SRV needs only a UDP socket, so the public resolvers still work
            // where no OS service exists.
            srv: SrvResolver::new(Vec::new()),
            capabilities: probe_capabilities(),
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
struct UnsupportedProcesses;

#[cfg(not(any(windows, target_os = "linux")))]
impl ProcessInspector for UnsupportedProcesses {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError> {
        Err(PlatformError::unsupported(
            "no process backend for this target",
        ))
    }

    fn command_line(&self, _pid: u32) -> Result<String, PlatformError> {
        Err(PlatformError::unsupported(
            "no process backend for this target",
        ))
    }

    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError> {
        Err(PlatformError::unsupported(
            "no process backend for this target",
        ))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
struct UnsupportedWindows;

#[cfg(not(any(windows, target_os = "linux")))]
impl WindowInspector for UnsupportedWindows {
    fn enumerate(&self) -> Result<Vec<WindowInfo>, PlatformError> {
        Err(PlatformError::unsupported(
            "no window backend for this target",
        ))
    }

    fn foreground(&self) -> Result<Option<WindowInfo>, PlatformError> {
        Err(PlatformError::unsupported(
            "no window backend for this target",
        ))
    }

    fn client_bounds(&self, _handle: WindowHandle) -> Result<Rect, PlatformError> {
        Err(PlatformError::unsupported(
            "no window backend for this target",
        ))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
struct UnsupportedInput;

#[cfg(not(any(windows, target_os = "linux")))]
impl InputBackend for UnsupportedInput {
    fn mouse_state(&self) -> MouseState {
        MouseState::default()
    }

    fn key_state(&self, _key: Key) -> bool {
        false
    }

    fn click(&self, _button: MouseButton) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(
            "no input backend for this target",
        ))
    }

    fn move_relative(&self, _dx: i32, _dy: i32) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(
            "no input backend for this target",
        ))
    }

    fn key_press(&self, _key: Key) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(
            "no input backend for this target",
        ))
    }

    fn poll_events(&self, _out: &mut Vec<InputEvent>) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(
            "no input backend for this target",
        ))
    }

    fn probe_mouse_state(&self) -> Option<MouseState> {
        // Nothing can be observed here, and `None` is the answer that says so.
        None
    }

    fn probe_key_state(&self, _key: Key) -> Option<bool> {
        None
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
struct UnsupportedInjector;

#[cfg(not(any(windows, target_os = "linux")))]
impl NativeBridgeInjector for UnsupportedInjector {
    fn inject(&self, _pid: u32, _library: &Path) -> Result<InjectionReport, PlatformError> {
        Err(PlatformError::unsupported("no injector for this target"))
    }

    fn support(&self) -> BridgeSupport {
        BridgeSupport::UnsupportedOnLinuxYet
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
struct UnsupportedDiscord;

#[cfg(not(any(windows, target_os = "linux")))]
impl DiscordRpcTransport for UnsupportedDiscord {
    fn connect(&mut self, _app_id: &str) -> Result<(), DiscordTransportError> {
        Err(DiscordTransportError::NotRunning)
    }

    fn write_frame(&mut self, _opcode: u32, _payload: &[u8]) -> Result<(), DiscordTransportError> {
        Err(DiscordTransportError::NotRunning)
    }

    fn read_frame(
        &mut self,
        _out: &mut Vec<u8>,
    ) -> Result<Option<DiscordFrame>, DiscordTransportError> {
        Err(DiscordTransportError::NotRunning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_report_renders_without_a_base() {
        let report = InjectionReport {
            pid: 4242,
            library: "bridge.dll".to_string(),
            remote_base: None,
        };
        assert_eq!(report.to_string(), "injected bridge.dll into pid 4242");
    }

    #[test]
    fn injection_report_renders_hex_base() {
        let report = InjectionReport {
            pid: 7,
            library: "bridge.dll".to_string(),
            remote_base: Some(0x1_0000),
        };
        assert_eq!(
            report.to_string(),
            "injected bridge.dll into pid 7 at 0x10000"
        );
    }

    #[test]
    fn the_compiled_platform_matches_the_target() {
        #[cfg(target_os = "linux")]
        assert_eq!(compiled_platform(), Platform::LinuxX11);
        #[cfg(windows)]
        assert_eq!(compiled_platform(), Platform::Windows);
    }

    #[test]
    fn default_services_are_consistent_with_the_probe() {
        let services = default_services();
        assert_eq!(services.capabilities.session, probe_capabilities().session);
    }
}
