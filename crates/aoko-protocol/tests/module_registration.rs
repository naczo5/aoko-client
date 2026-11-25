//! Port of `Aoko.Tests/ModuleRegistrationTests.cs`, protocol half.
//!
//! The managed suite is the catalog's cross-surface contract. Everything that touches
//! `MainWindow.xaml`, `MainWindow.xaml.cs`, and `InputHooks` belongs to `aoko-core`,
//! and the Beta-tab check belongs to the GUI crate; those cases are not here. What
//! remains is every surface this crate owns: the catalog itself, the capability
//! fallbacks, `bridge_capabilities.h`, both bridges' `ParseConfig`, and the config
//! payload keys.

use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::config_payload::CONFIG_KEYS;
use aoko_protocol::module_catalog::{
    count_enabled_for_discord, is_dev_only, is_gui_visible, requiring, EnabledFlag, Surfaces, ALL,
};
use std::collections::HashSet;

fn repo_file(relative: &str) -> String {
    let path = format!("{}/../../{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Reads one escaped JSON string array out of a named section of the header.
///
/// The header holds the legacy payload first and the modern one second, and both use
/// the same `\"modules\":[` opening, so the search must start at the section anchor.
fn header_array(section: &str, array: &str) -> HashSet<String> {
    let header = repo_file("McInjector/src/main/cpp/bridge_capabilities.h");
    let anchor = format!("{section}()");
    let section_start = header
        .find(&anchor)
        .unwrap_or_else(|| panic!("{anchor} not found"));

    let opener = format!("\\\"{array}\\\":[");
    let open = header[section_start..]
        .find(&opener)
        .map(|offset| offset + section_start)
        .unwrap_or_else(|| panic!("{array} array not found in {section}"));
    let body = &header[open + opener.len()..];
    let end = body.find(']').expect("array terminator");

    body[..end]
        .split("\\\"")
        .map(|token| token.trim_matches(',').to_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

fn missing(expected: &HashSet<String>, actual: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = expected.difference(actual).cloned().collect();
    out.sort();
    out
}

#[test]
fn catalog_has_unique_ids() {
    let mut seen = HashSet::new();
    let duplicates: Vec<&str> = ALL
        .iter()
        .map(|entry| entry.id)
        .filter(|id| !seen.insert(id.to_lowercase()))
        .collect();

    assert!(duplicates.is_empty(), "duplicate ids: {duplicates:?}");
    assert_eq!(ALL.len(), 42);
}

#[test]
fn catalog_capability_modules_match_modern_bridge_capabilities() {
    let catalog: HashSet<String> = requiring(Surfaces::CAPABILITY)
        .map(|entry| entry.id.to_lowercase())
        .collect();
    let fallback = BridgeCapabilities::for_version_fallback(Some("26.1"));
    let bridge: HashSet<String> = fallback
        .modules()
        .map(|module| module.to_lowercase())
        .collect();

    assert!(
        missing(&bridge, &catalog).is_empty(),
        "catalog is missing {:?}",
        missing(&bridge, &catalog)
    );
    assert!(
        missing(&catalog, &bridge).is_empty(),
        "modern fallback is missing {:?}",
        missing(&catalog, &bridge)
    );
}

#[test]
fn catalog_capability_modules_match_native_modern_capabilities_json() {
    let native = header_array("ModernCapabilitiesJson", "modules");

    let catalog: HashSet<String> = requiring(Surfaces::CAPABILITY)
        .filter(|entry| !entry.managed_only)
        .map(|entry| entry.id.to_lowercase())
        .collect();

    assert!(
        missing(&native, &catalog).is_empty(),
        "catalog is missing {:?}",
        missing(&native, &catalog)
    );
    assert!(
        missing(&catalog, &native).is_empty(),
        "native header is missing {:?}",
        missing(&catalog, &native)
    );
}

#[test]
fn native_modern_capabilities_advertise_stable_managed_fields() {
    let header = repo_file("McInjector/src/main/cpp/bridge_capabilities.h").to_lowercase();

    for field in [
        "holdingblock",
        "lookingatblock",
        "fov",
        "viewportwidth",
        "viewportheight",
        "autototembehaviormode",
        "nametagshowhelditem",
    ] {
        assert!(
            header.contains(&format!("\\\"{field}\\\"")),
            "header is missing {field}"
        );
    }
}

#[test]
fn native_capabilities_keep_the_kill_aura_settings_macro_in_both_versions() {
    let header = repo_file("McInjector/src/main/cpp/bridge_capabilities.h");

    for anchor in ["LegacyCapabilitiesJson", "ModernCapabilitiesJson"] {
        let start = header
            .find(anchor)
            .unwrap_or_else(|| panic!("{anchor} not found"));
        let settings = header[start..]
            .find("\\\"settings\\\":[")
            .map(|offset| offset + start)
            .unwrap_or_else(|| panic!("{anchor} settings list not found"));
        let state = header[settings..]
            .find("\\\"state\\\":[")
            .map(|offset| offset + settings)
            .unwrap_or_else(|| panic!("{anchor} state list not found"));

        assert!(settings > start && state > settings);
        let slice = &header[settings..state];
        assert!(slice.contains("LC_KILLAURA_SETTINGS"), "{anchor}");
    }
}

#[test]
fn catalog_toggle_modules_have_managed_config_payload_fields() {
    // The canonical enable flags consumed by both bridge families. Several UI ids
    // deliberately do not match their wire key (autoclicker -> armed). Kept verbatim.
    let expected: [(&str, &str); 41] = [
        ("autoclicker", "armed"),
        ("rightclick", "right"),
        ("jitter", "jitter"),
        ("clickinchests", "clickInChests"),
        ("breakblocks", "breakBlocks"),
        ("aimassist", "aimAssist"),
        ("triggerbot", "triggerbot"),
        ("killaura", "killAura"),
        ("speedbridge", "speedBridge"),
        ("gtbhelper", "gtbHelper"),
        ("pixelpartyassist", "pixelPartyAssist"),
        ("nametags", "nametags"),
        ("nickhider", "nickHiderEnabled"),
        ("closestplayer", "closestPlayerInfo"),
        ("fightstatus", "fightStatus"),
        ("chestesp", "chestEsp"),
        ("cheststealer", "chestStealerEnabled"),
        ("refill", "refillEnabled"),
        ("blockesp", "blockEspEnabled"),
        ("bedplates", "bedPlatesEnabled"),
        ("reach", "reachEnabled"),
        ("velocity", "velocityEnabled"),
        ("autototem", "autoTotemEnabled"),
        ("autorod", "autoRodEnabled"),
        ("throwpot", "throwpotEnabled"),
        ("autoheal", "autoHealEnabled"),
        ("autotool", "autoToolEnabled"),
        ("antidebuff", "antiDebuffEnabled"),
        ("hitdelayfix", "hitDelayFixEnabled"),
        ("fireballwarning", "fireballWarning"),
        ("fastplace", "fastPlace"),
        ("nohurtcam", "noHurtCam"),
        ("sprint", "sprint"),
        ("tnttimer", "tntTimer"),
        ("wtap", "wTap"),
        ("jumpreset", "jumpReset"),
        ("hitselect", "hitSelect"),
        ("trajectories", "trajectories"),
        ("murdermystery", "murderMystery"),
        ("breakprogress", "breakProgress"),
        ("hudeditoractive", "hudEditorActive"),
    ];

    let catalog: HashSet<String> = requiring(Surfaces::OVERLAY_LIST)
        .map(|entry| entry.id.to_lowercase())
        .collect();
    let expected_ids: HashSet<String> = expected
        .iter()
        .filter(|(id, _)| *id != "hudeditoractive")
        .map(|(id, _)| (*id).to_owned())
        .collect();

    assert_eq!(
        catalog, expected_ids,
        "the config payload map must cover exactly the catalog OverlayList modules"
    );

    let keys: HashSet<&str> = CONFIG_KEYS.iter().copied().collect();
    let absent: Vec<&str> = expected
        .iter()
        .filter(|(id, _)| *id != "hudeditoractive")
        .filter(|(_, key)| !keys.contains(key))
        .map(|(id, _)| *id)
        .collect();

    match absent.is_empty() {
        true => {}
        false => {
            let detail: Vec<String> = absent
                .iter()
                .map(|id| {
                    let key = expected
                        .iter()
                        .find(|(candidate, _)| candidate == id)
                        .map(|(_, key)| *key)
                        .unwrap_or("");
                    format!("{id} -> {key}")
                })
                .collect();
            panic!("config payload is missing module enable field(s): {detail:?}");
        }
    }
}

#[test]
fn every_enabled_flag_is_reachable_or_explicitly_never() {
    // `Never` means "no enable flag": the module is always live or HUD-owned.
    let mut flags: Vec<EnabledFlag> = ALL.iter().map(|entry| entry.enabled_flag).collect();
    flags.sort_by_key(|flag| format!("{flag:?}"));
    flags.dedup_by_key(|flag| format!("{flag:?}"));

    // 42 entries, 42 flag variants: every flag names exactly one module.
    assert_eq!(ALL.len(), 42);
    assert!(flags.len() >= 40, "{flags:?}");
    assert!(ALL
        .iter()
        .any(|entry| entry.enabled_flag == EnabledFlag::Never));
}

#[test]
fn setting_specs_are_wired_on_every_surface() {
    let config_keys: HashSet<&str> = CONFIG_KEYS.iter().copied().collect();
    let legacy = repo_file("McInjector/src/main/cpp/bridge.cpp");
    let modern = repo_file("McInjector/src/main/cpp/bridge_261.cpp");

    for entry in ALL {
        for spec in entry.settings {
            assert!(
                config_keys.contains(spec.wire_key),
                "config payload is missing {}",
                spec.wire_key
            );
            assert!(
                legacy.contains(&format!("\"{}\"", spec.wire_key)),
                "legacy bridge does not parse {}",
                spec.wire_key
            );
            assert!(
                modern.contains(&format!("\"{}\"", spec.wire_key)),
                "modern bridge does not parse {}",
                spec.wire_key
            );
            assert!(
                spec.min <= spec.default,
                "{} default below min",
                spec.wire_key
            );
            assert!(
                spec.default <= spec.max,
                "{} default above max",
                spec.wire_key
            );
        }
    }
}

#[test]
fn overlay_list_bridge_261_push_mod_covers_every_catalog_module() {
    let bridge = repo_file("McInjector/src/main/cpp/bridge_261.cpp");

    for entry in ALL {
        assert!(
            bridge.contains(entry.overlay_probe),
            "bridge_261.cpp is missing the probe {:?} for {}",
            entry.overlay_probe,
            entry.id
        );
    }
}

#[test]
fn overlay_list_legacy_bridge_push_mod_covers_legacy_capable_overlay_modules() {
    let bridge = repo_file("McInjector/src/main/cpp/bridge.cpp");

    let legacy_modules: Vec<&str> = requiring(Surfaces::OVERLAY_LIST)
        .filter(|entry| !entry.managed_only)
        .map(|entry| entry.id)
        .collect();

    // The legacy bridge ships its own `pushMod` list; every OverlayList module it
    // advertises must have a probe there too.
    let legacy = BridgeCapabilities::for_version_fallback(Some("1.8.9"));
    for id in legacy_modules {
        if !legacy.supports_module(id) {
            continue;
        }
        let probe = ALL
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(id))
            .map(|entry| entry.overlay_probe)
            .unwrap_or_default();
        assert!(
            bridge.contains(probe),
            "bridge.cpp is missing the probe {probe:?} for {id}"
        );
    }
}

#[test]
fn keybind_maps_profile_module_keys_cover_every_catalog_module() {
    use aoko_protocol::profile::Profile;

    let profile = Profile::default();
    for entry in requiring(Surfaces::KEYBIND_MAPS) {
        assert!(
            profile.module_keys.contains_key(entry.id),
            "profile defaults are missing {}",
            entry.id
        );
        assert_eq!(
            profile.module_keys.get(entry.id),
            Some(&0),
            "{} must default to key 0",
            entry.id
        );
    }
}

#[test]
fn discord_rpc_count_tracks_every_catalog_module() {
    // Nothing enabled: no modules counted.
    assert_eq!(count_enabled_for_discord(false, |_| false), 0);

    // Everything enabled: the count is the Discord surface minus the modules the GUI
    // hides while Dev Mode is off.
    let visible = ALL
        .iter()
        .filter(|entry| entry.requires(Surfaces::DISCORD_RPC))
        .filter(|entry| is_gui_visible(entry, false))
        .count();
    assert_eq!(count_enabled_for_discord(false, |_| true), visible);

    let visible_with_dev = ALL
        .iter()
        .filter(|entry| entry.requires(Surfaces::DISCORD_RPC))
        .count();
    assert_eq!(count_enabled_for_discord(true, |_| true), visible_with_dev);
    assert!(visible_with_dev >= visible);

    // A single enabled flag is counted once, and a disabled one not at all.
    let only_kill_aura = count_enabled_for_discord(true, |flag| flag == EnabledFlag::KillAura);
    assert_eq!(only_kill_aura, 1);
}

#[test]
fn catalog_capability_modules_match_native_legacy_capabilities_json() {
    let native = header_array("LegacyCapabilitiesJson", "modules");

    let legacy = BridgeCapabilities::for_version_fallback(Some("1.8.9"));
    let capability: HashSet<String> = legacy
        .modules()
        .map(|module| module.to_lowercase())
        .collect();

    assert!(
        missing(&native, &capability).is_empty(),
        "legacy fallback is missing {:?}",
        missing(&native, &capability)
    );
    assert!(
        missing(&capability, &native).is_empty(),
        "legacy header is missing {:?}",
        missing(&capability, &native)
    );
}

#[test]
fn catalog_dev_only_membership_matches_the_flags() {
    for entry in ALL {
        assert_eq!(is_dev_only(entry.id), entry.dev_only, "{}", entry.id);
    }

    // No entry is `ManagedOnly`: architecture decision 7 keeps it false everywhere so
    // the native capability payload stays the authority.
    assert!(ALL.iter().all(|entry| !entry.managed_only));
}
