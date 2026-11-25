//! Process enumeration and Minecraft instance discovery.

use crate::error::PlatformError;

/// One running process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Executable basename, e.g. `java.exe` / `java`.
    pub name: String,
    /// Full command line where readable. `None` when the OS denies it.
    pub command_line: Option<String>,
}

/// A process that looks like a Minecraft client we could attach to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceCandidate {
    pub pid: u32,
    /// Human label for the window picker, e.g. `1.8.9 Lunar Client`.
    pub label: String,
    /// Detected Minecraft version where derivable from the command line.
    pub version: Option<String>,
    /// Launcher family, e.g. `lunar`, `prism`, `vanilla`.
    pub launcher: Option<String>,
    /// Path to the JVM executable, when known.
    pub executable: Option<String>,
}

pub trait ProcessInspector: Send + Sync {
    fn list_processes(&self) -> Result<Vec<ProcessInfo>, PlatformError>;
    fn command_line(&self, pid: u32) -> Result<String, PlatformError>;
    fn discover_instances(&self) -> Result<Vec<InstanceCandidate>, PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_candidate_allows_unknown_metadata() {
        let candidate = InstanceCandidate {
            pid: 1,
            label: "Minecraft".to_string(),
            version: None,
            launcher: None,
            executable: None,
        };
        assert!(candidate.version.is_none());
    }
}
