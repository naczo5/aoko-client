//! Port of `Aoko.Tests/LegacyRenderOptimizationTests.cs`.
//!
//! Every case is a source pin against `McInjector/src/main/cpp/bridge.cpp`: the
//! optimisations are not representable in Rust data, so the contract is "the native
//! legacy bridge still reads the way the perf work left it". Those files live in this
//! repository, so the pins run here.

fn bridge_source() -> String {
    let path = format!(
        "{}/../../McInjector/src/main/cpp/bridge.cpp",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn slice<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source
        .find(start)
        .unwrap_or_else(|| panic!("missing start marker: {start}"));
    let to = source[from..]
        .find(end)
        .map(|offset| offset + from)
        .unwrap_or_else(|| panic!("missing end marker: {end}"));
    &source[from..to]
}

#[test]
fn menu_close_does_not_destroy_valid_renderer_resources() {
    let source = bridge_source();

    assert!(
        !source.contains("ResetImGuiBackendsForReinit(\"minecraft screen closed\")"),
        "closing a menu must not throw away the GL backend"
    );
    assert!(source.contains("keep the expensive ImGui context, font atlas, and GL backend"));
}

#[test]
fn closest_player_renderer_consumes_prepared_snapshot_without_jni() {
    let source = bridge_source();
    let renderer = slice(
        &source,
        "void RenderClosestPlayerInfo(",
        "static void UpdateChestListLegacy(",
    );

    assert!(renderer.contains("const ClosestPlayerDrawSnapshot18& snapshot"));
    assert!(!renderer.contains("ScopedJNIEnv"));
    assert!(!renderer.contains("CallObjectMethod"));
}

#[test]
fn render_entry_captures_config_and_hud_layout_together() {
    let source = bridge_source();
    let render_entry = slice(&source, "BOOL WINAPI HookedSwapBuffers(", "// IAT hook:");

    assert!(render_entry.contains("renderConfig = g_config;"));
    assert!(render_entry.contains("renderHudLayout = g_hudLayout;"));
    assert!(render_entry.contains("RenderBlockESP(w, h, renderConfig, renderHudLayout);"));
}

#[test]
fn chest_esp_sorts_by_distance_and_uses_chunk_range() {
    let source = bridge_source();
    let chest_logic = slice(
        &source,
        "static void UpdateChestListLegacy(",
        "// ===================== BLOCK ESP",
    );

    assert!(chest_logic.contains("ChestEspSortNearestFirst"));
    assert!(chest_logic.contains("ChestEspInChunkRange"));
    assert!(chest_logic.contains("chestEspRange"));
    assert!(
        !chest_logic.contains("chestEspMaxCount"),
        "the chest list is range-bounded, not count-bounded"
    );
    assert!(!chest_logic.contains("drawn < chestEspMaxCount"));
}

#[test]
fn nametags_use_chunk_range_instead_of_a_count_cap() {
    let source = bridge_source();
    let nametag_logic = slice(
        &source,
        "static void UpdatePlayerListOverlayLegacy(",
        "void RenderClosestPlayerInfo(",
    );

    assert!(nametag_logic.contains("nametagRange"));
    assert!(nametag_logic.contains("InChunkRange"));
    assert!(!nametag_logic.contains("nametagMaxCount"));
}

#[test]
fn section_scans_share_one_chunk_visitor() {
    let source = bridge_source();

    assert!(source.contains("UpdateSectionChunkScansLegacy"));
    assert!(!source.contains("UpdateBlockEspListLegacy"));
    assert!(!source.contains("UpdateBedPlatesListLegacy"));
    assert!(source.contains("CombinedSectionChunkBudget"));
    assert!(source.contains("CombinedSectionChunkBudget(blockOn, bedsOn)"));
    assert!(source.contains("if (!chunk) continue"));
    assert!(source.contains("if (bedsDue) PublishBedPlatesFromCache18"));
}

#[test]
fn held_item_nametag_setting_is_parsed_and_applied() {
    let source = bridge_source();

    assert!(source.contains("nametagShowHeldItem"));
    assert!(source.contains("showHeldItem"));
    assert!(source.contains("GetEntityHeldItemInfo"));
    assert!(source.contains("if (!heldText.empty())"));
}
