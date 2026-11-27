//! Port of `Aoko.Tests/GameStateClientInjectionTests.cs`, protocol half.
//!
//! The two managed cases call `GameStateClient.ResolveInjectionTarget(pid)`, which
//! opens a Win32 process handle and is owned by `aoko-platform`. What this crate owes
//! injection is the decision that follows a resolved target — which bridge DLL to
//! load — plus the guarantee that it never takes over the OS part by accident. Both
//! are pinned here.

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::codec::Decoder;
use aoko_protocol::config_payload::{normalize_detected_version, resolve_bridge_dll_name};

#[test]
fn resolve_injection_target_returns_a_usable_version_when_the_pid_matches() {
    // The platform half hands back a process; the protocol half turns the version it
    // discovered into the bridge it injects. A current-process target on a modern
    // client resolves to the 26.1 bridge.
    let detected = normalize_detected_version(Some("26.2")).expect("a supported version");

    assert_eq!(detected, "26.2");
    assert_eq!(resolve_bridge_dll_name(detected), "bridge_261.dll");
    assert!(Decoder::with_injected_version(detected)
        .capabilities()
        .supports_module("triggerbot"));
}

#[test]
fn resolve_injection_target_returns_null_for_an_invalid_pid() {
    // An unresolvable target yields no version at all, and the host then injects the
    // legacy bridge with the legacy capability surface.
    assert_eq!(normalize_detected_version(None), None);
    assert_eq!(resolve_bridge_dll_name(""), "bridge.dll");

    let decoder = Decoder::with_injected_version("");
    assert_eq!(
        decoder.capabilities(),
        &BridgeCapabilities::for_version_fallback(None)
    );
    assert!(!decoder.capabilities().supports_module("triggerbot"));
}

#[test]
fn the_protocol_crate_never_spawns_or_inspects_processes() {
    // Architectural pin: the crate's description promises "No OS or UI
    // dependencies". `netstat` line parsing was split out precisely so the process
    // spawn stays in `aoko-platform`; a `std::process` import here would undo that.
    let source_dir = format!("{}/src", env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;

    let mut pending = vec![std::path::PathBuf::from(&source_dir)];
    while let Some(path) = pending.pop() {
        let entries = std::fs::read_dir(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                continue;
            }

            let text =
                std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
            for banned in ["std::process", "Command::new", "std::net::TcpStream"] {
                assert!(
                    !text.contains(banned),
                    "{} must not reference {banned}",
                    path.display()
                );
            }
            checked += 1;
        }
    }

    assert!(checked > 10, "only scanned {checked} source files");
}
