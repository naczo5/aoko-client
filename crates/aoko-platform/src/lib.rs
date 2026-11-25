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

mod capabilities;
mod discord;
mod error;
mod input;
mod process;
mod window;

pub use capabilities::{BridgeSupport, CapabilityReport, SessionKind};
pub use discord::{DiscordFrame, DiscordRpcTransport, DiscordTransportError};
pub use error::PlatformError;
pub use input::{InputBackend, InputEvent, Key, MouseButton, MouseState};
pub use process::{InstanceCandidate, ProcessInfo, ProcessInspector};
pub use window::{Rect, WindowHandle, WindowInfo, WindowInspector};

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
/// of failing at call time.
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
    pub capabilities: CapabilityReport,
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
}
