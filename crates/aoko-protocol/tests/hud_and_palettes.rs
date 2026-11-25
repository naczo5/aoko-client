//! `Aoko/Core/HudLayout.cs` and `Aoko/Core/CustomPalette.cs` have no managed test
//! suite of their own; their contract is in `WIRE_CONTRACT.md` and in the C++
//! `hud_layout.h` defaults. These cases pin the parts the wire depends on: emission
//! order, the scale clamp, and the palette file round-trip.

use aoko_protocol::hud::{
    element_id, HudElementLayout, HudLayout, KNOWN_ELEMENT_IDS, MAX_SCALE, MIN_SCALE,
};
use aoko_protocol::palettes::{
    delete, from_palettes_json, to_palettes_json, upsert, CustomPalette,
};
use serde_json::Value;

#[test]
fn every_known_element_id_is_present_in_emission_order() {
    let layout = HudLayout::new();

    assert_eq!(KNOWN_ELEMENT_IDS.len(), 7);
    assert_eq!(
        KNOWN_ELEMENT_IDS,
        [
            element_id::MODULE_LIST,
            element_id::CLOSEST_PLAYER,
            element_id::PIXEL_PARTY,
            element_id::CHEST_ESP_LIST,
            element_id::GTB_HINT,
            element_id::NAMETAGS,
            element_id::BLOCK_ESP_LIST,
        ]
    );

    let mut wire = String::new();
    layout.write_wire(&mut wire);
    let mut cursor = 0;
    for id in KNOWN_ELEMENT_IDS {
        let found = wire[cursor..]
            .find(id)
            .unwrap_or_else(|| panic!("{id} missing from {wire}"));
        cursor += found;
    }
}

#[test]
fn element_defaults_are_the_canonical_anchors() {
    let element = HudElementLayout::default();

    assert_eq!(element.x, 0.0);
    assert_eq!(element.y, 0.0);
    assert_eq!(element.scale, 1.0);
}

#[test]
fn scale_is_clamped_to_the_documented_range() {
    let mut layout = HudLayout::new();

    layout.set(
        element_id::MODULE_LIST,
        HudElementLayout::new(0.1, 0.2, 9.0),
    );
    let tall = layout.get(element_id::MODULE_LIST);
    assert_eq!(tall.scale, MAX_SCALE);
    assert_eq!(MAX_SCALE, 2.0);

    layout.set(
        element_id::MODULE_LIST,
        HudElementLayout::new(0.1, 0.2, 0.01),
    );
    let small = layout.get(element_id::MODULE_LIST);
    assert_eq!(small.scale, MIN_SCALE);
    assert_eq!(MIN_SCALE, 0.5);

    // Anchors are clamped into `[0, 1]`.
    layout.set(
        element_id::MODULE_LIST,
        HudElementLayout::new(-3.0, 4.0, 1.0),
    );
    let pinned = layout.get(element_id::MODULE_LIST);
    assert_eq!(pinned.x, 0.0);
    assert_eq!(pinned.y, 1.0);
}

#[test]
fn reset_element_and_reset_all_restore_the_canonical_layout() {
    let mut layout = HudLayout::new();
    layout.set(element_id::GTB_HINT, HudElementLayout::new(0.7, 0.8, 1.5));

    assert_ne!(layout, HudLayout::new());
    layout.reset_element(element_id::GTB_HINT);
    assert_eq!(layout, HudLayout::new());

    layout.set(element_id::NAMETAGS, HudElementLayout::new(0.4, 0.5, 0.75));
    layout.reset_all();
    assert_eq!(layout, HudLayout::new());
}

#[test]
fn unknown_element_ids_are_ignored_rather_than_invented() {
    let mut layout = HudLayout::new();

    layout.set("future-element", HudElementLayout::new(0.5, 0.5, 1.0));

    assert_eq!(layout.get("future-element"), HudElementLayout::default());
    assert_eq!(layout, HudLayout::new());
}

#[test]
fn hud_layout_json_round_trips_and_compares_by_value() {
    let mut layout = HudLayout::new();
    layout.set(
        element_id::MODULE_LIST,
        HudElementLayout::new(0.25, 0.75, 1.25),
    );

    let json = layout.to_json_value();
    let restored = HudLayout::from_json(&json);

    assert_eq!(restored, layout);
    assert!(layout.equals_layout(&restored));
    assert_eq!(json["modulelist"]["x"], 0.25);
}

#[test]
fn hud_layout_from_json_tolerates_missing_and_odd_values() {
    // A partial echo from an older bridge must not move every element.
    let json: Value = serde_json::from_str(r#"{"modulelist":{"x":0.3}}"#).expect("json");
    let layout = HudLayout::from_json(&json);

    assert_eq!(layout.get(element_id::MODULE_LIST).x, 0.3);
    assert_eq!(
        layout.get(element_id::MODULE_LIST).scale,
        1.0,
        "a missing scale falls back to the canonical one"
    );
    assert_eq!(
        layout.get(element_id::NAMETAGS),
        HudElementLayout::default()
    );
}

#[test]
fn palettes_round_trip_through_their_file_format() {
    let palette = CustomPalette {
        name: "Stream".to_owned(),
        accent: "#FF00AA".to_owned(),
        ..CustomPalette::default()
    };

    let json = to_palettes_json(std::slice::from_ref(&palette));
    let restored = from_palettes_json(&json);

    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].name, "Stream");
    assert_eq!(restored[0].accent, "#FF00AA");
}

#[test]
fn malformed_palette_files_degrade_to_empty() {
    assert!(from_palettes_json("").is_empty());
    assert!(from_palettes_json("   ").is_empty());
    assert!(from_palettes_json("{not json").is_empty());
}

#[test]
fn upsert_replaces_by_name_case_insensitively_and_delete_removes() {
    let mut palettes = Vec::new();
    upsert(
        &mut palettes,
        CustomPalette {
            name: "Stream".to_owned(),
            accent: "#111111".to_owned(),
            ..CustomPalette::default()
        },
    );
    upsert(
        &mut palettes,
        CustomPalette {
            name: "stream".to_owned(),
            accent: "#222222".to_owned(),
            ..CustomPalette::default()
        },
    );

    assert_eq!(palettes.len(), 1);
    assert_eq!(palettes[0].accent, "#222222");

    upsert(
        &mut palettes,
        CustomPalette {
            name: "Night".to_owned(),
            ..CustomPalette::default()
        },
    );
    assert_eq!(palettes.len(), 2);

    delete(&mut palettes, "STREAM");
    assert_eq!(palettes.len(), 1);
    assert_eq!(palettes[0].name, "Night");
}
