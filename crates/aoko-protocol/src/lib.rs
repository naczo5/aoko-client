//! Pure, OS-agnostic types shared between the Aoko host and its bridges.
//!
//! This crate must never depend on a platform, UI or async runtime so that it
//! stays testable, benchable and wire-pinned against `McInjector/`.

/// TCP port the bridge binds and the host dials. See `AGENTS.md`.
pub const BRIDGE_PORT: u16 = 25590;
