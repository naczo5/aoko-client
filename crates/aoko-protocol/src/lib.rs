//! Pure, OS-agnostic types shared between the Aoko host and its bridges.
//!
//! This crate must never depend on a platform, UI or async runtime so that it
//! stays testable, benchable and wire-pinned against `McInjector/`.

pub mod accounts;
pub mod block_esp;
pub mod capabilities;
pub mod codec;
pub mod command_result;
pub mod commands;
pub mod config_payload;
pub mod diagnostics;
pub mod framing;
pub mod gtb;
pub mod hud;
pub mod kill_aura;
pub mod module_catalog;
pub mod netstat;
pub mod nick_hider;
pub mod palettes;
#[cfg(test)]
mod perf;
pub mod profile;
pub mod servers;
pub mod state;
pub mod state_patch;
pub mod stats;
pub mod transport;

/// TCP port the bridge binds and the host dials. See `AGENTS.md`.
pub const BRIDGE_PORT: u16 = 25590;

/// Cadence constants from `Aoko/Core/GameStateClient.cs`.
pub mod cadence {
    /// `ConfigHeartbeatMs`: re-send config even when unchanged.
    pub const CONFIG_HEARTBEAT_MS: u64 = 2000;
    /// `ConfigChangeCoalesceMs`: coalesce slider drags into one push.
    pub const CONFIG_CHANGE_COALESCE_MS: u64 = 25;
    /// `StateNotificationIntervalMs`: coalesce UI notifications.
    pub const STATE_NOTIFICATION_INTERVAL_MS: u64 = 25;
    /// `SendTimeout`: ceiling for one command/config line write.
    pub const SEND_TIMEOUT_MS: u64 = 10_000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_port_matches_the_native_bridges() {
        assert_eq!(BRIDGE_PORT, 25590);
    }

    #[test]
    fn cadence_constants_match_the_managed_client() {
        assert_eq!(cadence::CONFIG_HEARTBEAT_MS, 2000);
        assert_eq!(cadence::CONFIG_CHANGE_COALESCE_MS, 25);
        assert_eq!(cadence::STATE_NOTIFICATION_INTERVAL_MS, 25);
        assert_eq!(cadence::SEND_TIMEOUT_MS, 10_000);
    }
}
