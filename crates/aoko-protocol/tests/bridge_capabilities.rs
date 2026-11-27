//! Port of `Aoko.Tests/BridgeCapabilitiesTests.cs` and
//! `Aoko.Tests/BridgeCapabilitiesActionsTests.cs`.

use aoko_protocol::capabilities::{actions, BridgeCapabilities};
use serde_json::{json, Value};

fn fallback(version: &str) -> BridgeCapabilities {
    BridgeCapabilities::for_version_fallback(Some(version))
}

#[test]
fn modern_fallback_exposes_modern_features_for_262() {
    let caps = fallback("26.2");

    assert!(caps.supports_module("triggerbot"));
    assert!(caps.supports_module("killaura"));
    assert!(caps.supports_module("nametags"));
    assert!(caps.supports_module("nickhider"));
    assert!(caps.supports_setting("nickhiderenabled"));
}

#[test]
fn modern_fallback_exposes_modern_features() {
    let caps = fallback("26.1");

    assert!(caps.supports_module("triggerbot"));
    assert!(caps.supports_module("pixelpartyassist"));
    assert!(caps.supports_module("nickhider"));
    assert!(caps.supports_setting("nickhideralias"));
    assert!(caps.supports_module("cheststealer"));
    assert!(caps.supports_setting("pixelpartyassist"));
    assert!(caps.supports_state_field("pixelpartyyawdelta"));
    assert!(caps.supports_setting("cheststealerenabled"));
    assert!(caps.supports_setting("cheststealermenucheck"));
    assert!(caps.supports_setting("cheststealertitlecheck"));
    assert!(caps.supports_setting("cheststealercustomitemscheck"));
    assert!(caps.supports_setting("cheststealerphysicalcheck"));
    assert!(!caps.supports_module("silentaura"));
    assert!(!caps.supports_setting("silentaura"));
    assert!(!caps.supports_setting("silentarange"));
    assert!(caps.supports_module("killaura"));
    assert!(caps.supports_setting("killaurarotmode"));
    assert!(caps.supports_setting("killaurafov"));
    assert!(caps.supports_setting("killauraweaponsonly"));
    assert!(caps.supports_state_field("cheststealerstate"));
    assert!(caps.supports_setting("nametagshowhelditem"));
    assert!(caps.supports_state_field("attackcooldown"));
}

#[test]
fn legacy_fallback_exposes_pixel_party_assist() {
    let caps = fallback("1.8.9");

    assert!(caps.supports_module("pixelpartyassist"));
    assert!(caps.supports_setting("pixelpartyautowalk"));
    assert!(caps.supports_state_field("pixelpartyyawdelta"));
}

#[test]
fn legacy_fallback_does_not_expose_triggerbot() {
    let caps = fallback("1.8.9");

    assert!(!caps.supports_module("triggerbot"));
    assert!(!caps.supports_setting("triggerbot"));
    assert!(caps.supports_state_field("holdingblock"));
}

#[test]
fn legacy_fallback_exposes_kill_aura_but_not_silent_aura() {
    let caps = fallback("1.8.9");

    assert!(caps.supports_module("killaura"));
    assert!(caps.supports_setting("killaurarotmode"));
    assert!(caps.supports_setting("killaurafov"));
    assert!(caps.supports_setting("killauraweaponsonly"));
    assert!(!caps.supports_module("silentaura"));
    assert!(!caps.supports_setting("silentaura"));
}

#[test]
fn legacy_fallback_exposes_chest_stealer() {
    let caps = fallback("1.8.9");

    assert!(caps.supports_module("cheststealer"));
    assert!(caps.supports_setting("cheststealerenabled"));
    assert!(caps.supports_setting("cheststealerdelayms"));
    assert!(caps.supports_setting("cheststealermenucheck"));
    assert!(caps.supports_setting("cheststealertitlecheck"));
    assert!(caps.supports_setting("cheststealercustomitemscheck"));
    assert!(caps.supports_setting("cheststealerphysicalcheck"));
    assert!(caps.supports_setting("keybindcheststealer"));
    assert!(caps.supports_state_field("cheststealerstate"));
}

#[test]
fn auto_rod_fallback_supports_module_and_config() {
    for version in ["1.8.9", "1.21.4", "26.1", "26.2"] {
        let caps = fallback(version);
        assert!(caps.supports_module("autorod"), "{version}");
        assert!(caps.supports_setting("autorodenabled"), "{version}");
        assert!(caps.supports_setting("autorodslotmode"), "{version}");
        assert!(
            caps.supports_setting("autorodverifyforcedslot"),
            "{version}"
        );
        assert!(caps.supports_setting("keybindautorod"), "{version}");
        assert!(caps.supports_state_field("inworld"), "{version}");
    }
}

#[test]
fn auto_tool_is_available_on_all_supported_versions() {
    // Port of `AutoToolManagedTests.Capabilities_IncludesAutoToolOnAllSupportedVersions`.
    for version in ["1.8.9", "26.1", "26.2"] {
        let caps = fallback(version);
        assert!(caps.supports_module("autotool"), "{version}");
        for setting in [
            "autotoolenabled",
            "autotoolswapweapon",
            "autotoolinstantswap",
            "autotoolswaptodelay",
            "autotoolswapback",
            "autotoolswapbackdelay",
            "autotoolrequiremousedown",
            "autotoolonlysneaking",
            "autotoolbedwarsmode",
        ] {
            assert!(
                caps.supports_setting(setting),
                "{version} is missing {setting}"
            );
        }
    }
}

#[test]
fn anti_debuff_is_available_on_all_versions() {
    let modern = fallback("26.1");
    let legacy = fallback("1.8.9");

    assert!(modern.supports_module("antidebuff"));
    assert!(modern.supports_setting("antidebuffenabled"));
    assert!(legacy.supports_module("antidebuff"));
    assert!(legacy.supports_setting("antidebuffenabled"));
}

#[test]
fn fight_status_fallback_supports_module_setting_and_keybind() {
    for version in ["1.8.9", "1.21.4", "26.1", "26.2"] {
        let caps = fallback(version);
        assert!(caps.supports_module("fightstatus"), "{version}");
        assert!(caps.supports_setting("fightstatus"), "{version}");
        assert!(caps.supports_setting("keybindfightstatus"), "{version}");
    }
}

#[test]
fn fight_status_from_payload_normalizes_module_setting_and_keybind() {
    let fallback = fallback("1.8.9");
    let payload = json!({
        "modules": ["FIGHTSTATUS"],
        "settings": ["FightStatus", "KeybindFightStatus"]
    });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_module("fightstatus"));
    assert!(parsed.supports_setting("fightstatus"));
    assert!(parsed.supports_setting("keybindfightstatus"));
}

#[test]
fn from_payload_uses_fallback_when_arrays_missing() {
    let fallback = fallback("1.8.9");
    let payload = json!({ "type": "capabilities" });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert_eq!(parsed.module_count(), fallback.module_count());
    assert!(parsed.supports_module("autoclicker"));
    assert!(!parsed.supports_module("triggerbot"));
}

#[test]
fn from_payload_merges_and_normalizes_payload_data() {
    let fallback = fallback("1.8.9");
    let payload = json!({ "modules": ["TRIGGERBOT"], "state": ["ActionBar"] });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_module("triggerbot"));
    assert!(parsed.supports_state_field("actionbar"));
    assert!(parsed.supports_setting("mincps"));
}

// ── actions ──────────────────────────────────────────────────────────────────

#[test]
fn from_payload_parses_actions_case_insensitively() {
    let fallback = fallback("26.2");
    let payload = json!({
        "modules": ["AUTOCLICKER"],
        "actions": [
            "account.getSession",
            "Account.SetSession",
            "ACCOUNT.RESTORESESSION",
            "server.join"
        ]
    });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert_eq!(parsed.action_count(), 4);
    assert!(parsed.supports_action("account.getSession"));
    assert!(parsed.supports_action("ACCOUNT.SETSESSION"));
    assert!(parsed.supports_action(" account.restoreSession "));
    assert!(parsed.supports_action("server.join"));
    assert!(!parsed.supports_action("cosmetics.patch"));
    assert!(!parsed.supports_action(""));
    assert_eq!(parsed.actions().count(), 4);
}

#[test]
fn from_payload_parses_only_the_actions_the_bridge_advertises() {
    let fallback = fallback("1.8.9");
    let payload = json!({ "state": ["inworld"], "actions": ["server.join"] });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_action("server.join"));
    assert!(!parsed.supports_action("account.setSession"));
}

#[test]
fn from_payload_without_actions_inherits_the_fallback_actions() {
    let fallback = fallback("26.2");
    let payload = json!({
        "modules": ["triggerbot"],
        "settings": ["mincps"],
        "state": ["actionbar"]
    });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    // Both shipped bridges advertise the four actions, so a frame without the list
    // comes from a bridge that predates it.
    assert_eq!(parsed.action_count(), fallback.action_count());
    assert!(parsed.supports_action(actions::ACCOUNT_GET_SESSION));
    assert!(parsed.supports_action(actions::ACCOUNT_SET_SESSION));
    assert!(parsed.supports_action(actions::ACCOUNT_RESTORE_SESSION));
    assert!(parsed.supports_action(actions::SERVER_JOIN));
    assert!(parsed.supports_module("triggerbot"));
    assert!(parsed.supports_setting("mincps"));
    assert!(parsed.supports_state_field("actionbar"));
}

#[test]
fn from_payload_empty_document_still_returns_the_fallback() {
    let fallback = fallback("1.8.9");
    let payload = json!({ "type": "capabilities" });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert_eq!(parsed, fallback);
    assert_eq!(parsed.action_count(), fallback.action_count());
    assert!(parsed.supports_action(actions::SERVER_JOIN));
}

#[test]
fn from_payload_empty_document_keeps_the_fallback_when_actions_present() {
    let fallback = fallback("1.8.9");
    let payload = json!({ "type": "capabilities", "actions": ["account.getSession"] });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_action("account.getSession"));
    assert!(parsed.supports_module("autoclicker"));
    assert!(parsed.supports_setting("mincps"));
    assert!(parsed.supports_state_field("holdingblock"));
}

#[test]
fn version_fallback_advertises_the_shipped_actions() {
    for version in ["1.8.9", "1.21.4", "26.1", "26.2"] {
        let caps = fallback(version);
        assert_eq!(caps.action_count(), 4, "{version}");
        assert!(caps.supports_action(actions::ACCOUNT_GET_SESSION));
        assert!(caps.supports_action(actions::ACCOUNT_SET_SESSION));
        assert!(caps.supports_action(actions::ACCOUNT_RESTORE_SESSION));
        assert!(caps.supports_action(actions::SERVER_JOIN));
        assert!(!caps.supports_action("cosmetics.patch"));
    }
}

#[test]
fn actions_are_not_modules_settings_or_state_fields() {
    let fallback = fallback("26.2");
    let payload = json!({
        "modules": ["autoclicker"],
        "settings": ["mincps"],
        "state": ["inworld"],
        "actions": [
            "account.getSession",
            "account.setSession",
            "account.restoreSession",
            "server.join"
        ]
    });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert!(parsed.supports_action("account.setSession"));
    assert!(!parsed.supports_module("account.setSession"));
    assert!(!parsed.supports_setting("account.setSession"));
    assert!(!parsed.supports_state_field("account.setSession"));
    assert!(!parsed.supports_module("server.join"));
    assert_eq!(parsed.module_count(), 1);
    assert_eq!(parsed.setting_count(), 1);
    assert_eq!(parsed.state_field_count(), 1);
    assert_eq!(parsed.action_count(), 4);
}

#[test]
fn from_payload_empty_actions_array_also_inherits_the_fallback() {
    let fallback = fallback("26.2");
    let payload = json!({ "modules": ["triggerbot"], "actions": [] });

    let parsed = BridgeCapabilities::from_payload(&payload, &fallback);

    assert_eq!(parsed.action_count(), fallback.action_count());
    assert!(parsed.supports_action(actions::ACCOUNT_GET_SESSION));
    assert!(parsed.supports_module("triggerbot"));
    assert!(parsed.supports_setting("mincps"));
}

#[test]
fn from_payload_absent_actions_inherit_the_version_fallback() {
    let modern = fallback("26.2");
    let legacy = fallback("1.8.9");

    let from_modern = BridgeCapabilities::from_payload(
        &serde_json::from_str::<Value>(r#"{"type":"capabilities","modules":["TRIGGERBOT"]}"#)
            .expect("json"),
        &modern,
    );
    let from_legacy = BridgeCapabilities::from_payload(
        &serde_json::from_str::<Value>(r#"{"type":"capabilities","state":["ACTIONBAR"]}"#)
            .expect("json"),
        &legacy,
    );

    assert_eq!(from_modern.action_count(), modern.action_count());
    assert_eq!(from_legacy.action_count(), legacy.action_count());
    assert!(from_modern.supports_action(actions::SERVER_JOIN));
    assert!(from_legacy.supports_action(actions::ACCOUNT_GET_SESSION));
}

#[test]
fn from_payload_reads_the_native_compact_payload() {
    // Exactly the single-line shape the native capabilities writer emits.
    let native_payload = concat!(
        r#"{"type":"capabilities","modules":["AUTOCLICKER"],"#,
        r#""settings":["mincps"],"state":["inworld"],"#,
        r#""actions":["account.getSession","account.setSession","#,
        r#""account.restoreSession","server.join"]}"#
    );
    let fallback = fallback("26.2");
    let parsed = BridgeCapabilities::from_payload(
        &serde_json::from_str::<Value>(native_payload).expect("json"),
        &fallback,
    );

    assert_eq!(parsed.action_count(), 4);
    assert!(parsed.supports_action("account.getSession"));
    assert!(parsed.supports_action("account.setSession"));
    assert!(parsed.supports_action("account.restoreSession"));
    assert!(parsed.supports_action("server.join"));
    assert!(parsed.supports_module("autoclicker"));
    assert!(!parsed.supports_module("account.setSession"));
}

#[test]
fn actions_survive_a_type_only_payload_alongside_fallback_surfaces() {
    let fallback = fallback("26.2");
    let parsed = BridgeCapabilities::from_payload(
        &serde_json::from_str::<Value>(
            r#"{"type":"capabilities","actions":["ACCOUNT.GETSESSION","future.command"]}"#,
        )
        .expect("json"),
        &fallback,
    );

    assert!(parsed.supports_action("account.getsession"));
    assert!(parsed.supports_action("future.command"));
    assert!(parsed.supports_module("triggerbot"));
}

#[test]
fn native_capabilities_header_advertises_exactly_the_actions_we_parse() {
    // The managed suite read `bridge_capabilities.h` and asserted both literals
    // carry exactly the four actions. Keeping the check means extending the native
    // list without extending `actions` cannot pass silently.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../McInjector/src/main/cpp/bridge_capabilities.h"
    );
    let header = std::fs::read_to_string(path).expect("native capabilities header");

    let mut fragments = Vec::new();
    let mut offset = 0;
    while let Some(start) = header[offset..].find(r#"\"actions\":["#) {
        let start = offset + start;
        let end = header[start..].find(']').expect("actions array terminator") + start;
        fragments.push(&header[start..=end]);
        offset = end;
    }

    assert_eq!(fragments.len(), 2, "one actions array per bridge family");
    for fragment in fragments {
        assert!(
            !fragment.contains(": "),
            "compact literal style: {fragment}"
        );
        assert!(
            !fragment.contains(", "),
            "compact literal style: {fragment}"
        );

        let json = format!("{{{}}}", fragment.replace(r#"\""#, "\""));
        let parsed = BridgeCapabilities::from_payload(
            &serde_json::from_str::<Value>(&json).expect("json"),
            &fallback("1.8.9"),
        );

        assert!(
            parsed.supports_action(actions::ACCOUNT_GET_SESSION),
            "{json}"
        );
        assert!(
            parsed.supports_action(actions::ACCOUNT_SET_SESSION),
            "{json}"
        );
        assert!(
            parsed.supports_action(actions::ACCOUNT_RESTORE_SESSION),
            "{json}"
        );
        assert!(parsed.supports_action(actions::SERVER_JOIN), "{json}");
        assert_eq!(parsed.action_count(), 4, "{json}");
    }
}
