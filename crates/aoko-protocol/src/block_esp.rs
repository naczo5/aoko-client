//! Block ESP targets, presets, and the delimited wire encoding.
//!
//! Ported from `Aoko/Core/BlockEsp.cs`. The bridge config transport uses a naive
//! string parser that cannot handle JSON arrays, so the enabled block list is
//! encoded as one delimited string: `id=RRGGBB;id=RRGGBB`. Block ids never
//! contain `=` or `;`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Colour used when a target's colour is missing or malformed.
pub const DEFAULT_COLOR_HEX: &str = "00E5FF";

/// Hard cap on the encoded string, to bound the TCP payload.
pub const MAX_SERIALIZED_LENGTH: usize = 4000;

/// A single tracked block type for Block ESP.
///
/// `registry_id` is a Minecraft block identifier (`minecraft:diamond_ore`);
/// `color_hex` is a 6-digit uppercase `RRGGBB` string with no leading `#`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEspTarget {
    registry_id: String,
    display_name: String,
    color_hex: String,
    enabled: bool,
}

impl BlockEspTarget {
    #[must_use]
    pub fn new(
        registry_id: impl Into<String>,
        display_name: impl Into<String>,
        color_hex: Option<&str>,
        enabled: bool,
    ) -> Self {
        Self {
            registry_id: registry_id.into(),
            display_name: display_name.into(),
            color_hex: normalize_color(color_hex),
            enabled,
        }
    }

    #[must_use]
    pub fn registry_id(&self) -> &str {
        &self.registry_id
    }

    /// Friendly label, falling back to the registry id when empty.
    #[must_use]
    pub fn display_name(&self) -> &str {
        if self.display_name.trim().is_empty() {
            &self.registry_id
        } else {
            &self.display_name
        }
    }

    #[must_use]
    pub fn color_hex(&self) -> &str {
        &self.color_hex
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        true
    }

    /// Stores the normalized colour; returns whether it changed.
    pub fn set_color_hex(&mut self, color_hex: Option<&str>) -> bool {
        let normalized = normalize_color(color_hex);
        if self.color_hex == normalized {
            return false;
        }
        self.color_hex = normalized;
        true
    }

    #[must_use]
    pub fn to_data(&self) -> BlockEspTargetData {
        BlockEspTargetData {
            registry_id: self.registry_id.clone(),
            display_name: self.display_name.clone(),
            color_hex: self.color_hex.clone(),
            enabled: self.enabled,
            unknown: BTreeMap::new(),
        }
    }
}

/// Plain serialisable form stored inside a profile.
///
/// The four fields the GUI edits, plus the keys this build does not know, which travel with the
/// entry they were read on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEspTargetData {
    #[serde(rename = "registryId", default)]
    pub registry_id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(rename = "colorHex", default = "default_color")]
    pub color_hex: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Keys this build does not know on this entry, written back on save.
    ///
    /// One entry is where a newer writer is likeliest to add a setting: the list is a JSON array
    /// inside the profile, so a key with no field here has nowhere else to go, and a save would
    /// otherwise prune it. This is the shape
    /// [`crate::profile::HudElementLayoutValue`] gives one stored HUD element.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

fn default_color() -> String {
    DEFAULT_COLOR_HEX.to_owned()
}

fn default_true() -> bool {
    true
}

impl Default for BlockEspTargetData {
    fn default() -> Self {
        Self {
            registry_id: String::new(),
            display_name: String::new(),
            color_hex: DEFAULT_COLOR_HEX.to_owned(),
            enabled: true,
            unknown: BTreeMap::new(),
        }
    }
}

impl BlockEspTargetData {
    /// The bindable target, with the colour normalised on construction.
    ///
    /// The registry id is kept as stored, exactly as `BlockEspTarget.RegistryId` is;
    /// [`serialize_targets`] is where it is reduced to the wire's path token.
    ///
    /// The live target holds the four fields the GUI edits, so [`BlockEspTarget::to_data`]
    /// builds an entry with an empty [`Self::unknown`]: a key this build does not know stays on
    /// the stored entry, not on the form the GUI binds to.
    #[must_use]
    pub fn to_target(&self) -> BlockEspTarget {
        BlockEspTarget::new(
            self.registry_id.clone(),
            self.display_name.clone(),
            Some(&self.color_hex),
            self.enabled,
        )
    }

    /// Every preset present, only Diamond Ore enabled.
    #[must_use]
    pub fn build_defaults() -> Vec<Self> {
        build_default_targets()
            .iter()
            .map(BlockEspTarget::to_data)
            .collect()
    }
}

/// One curated preset: friendly name, registry id, colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEspPreset {
    pub display_name: &'static str,
    pub registry_id: &'static str,
    pub color_hex: &'static str,
}

/// Curated catalog of common blocks/ores offered in the Block ESP GUI.
pub const PRESETS: [BlockEspPreset; 20] = [
    BlockEspPreset {
        display_name: "Diamond Ore",
        registry_id: "minecraft:diamond_ore",
        color_hex: "00E5FF",
    },
    BlockEspPreset {
        display_name: "Deepslate Diamond Ore",
        registry_id: "minecraft:deepslate_diamond_ore",
        color_hex: "00E5FF",
    },
    BlockEspPreset {
        display_name: "Emerald Ore",
        registry_id: "minecraft:emerald_ore",
        color_hex: "2BE07A",
    },
    BlockEspPreset {
        display_name: "Deepslate Emerald Ore",
        registry_id: "minecraft:deepslate_emerald_ore",
        color_hex: "2BE07A",
    },
    BlockEspPreset {
        display_name: "Gold Ore",
        registry_id: "minecraft:gold_ore",
        color_hex: "FFD700",
    },
    BlockEspPreset {
        display_name: "Deepslate Gold Ore",
        registry_id: "minecraft:deepslate_gold_ore",
        color_hex: "FFD700",
    },
    BlockEspPreset {
        display_name: "Iron Ore",
        registry_id: "minecraft:iron_ore",
        color_hex: "D8AF93",
    },
    BlockEspPreset {
        display_name: "Deepslate Iron Ore",
        registry_id: "minecraft:deepslate_iron_ore",
        color_hex: "D8AF93",
    },
    BlockEspPreset {
        display_name: "Redstone Ore",
        registry_id: "minecraft:redstone_ore",
        color_hex: "FF4040",
    },
    BlockEspPreset {
        display_name: "Deepslate Redstone Ore",
        registry_id: "minecraft:deepslate_redstone_ore",
        color_hex: "FF4040",
    },
    BlockEspPreset {
        display_name: "Lapis Ore",
        registry_id: "minecraft:lapis_ore",
        color_hex: "2D5BFF",
    },
    BlockEspPreset {
        display_name: "Deepslate Lapis Ore",
        registry_id: "minecraft:deepslate_lapis_ore",
        color_hex: "2D5BFF",
    },
    BlockEspPreset {
        display_name: "Coal Ore",
        registry_id: "minecraft:coal_ore",
        color_hex: "404040",
    },
    BlockEspPreset {
        display_name: "Deepslate Coal Ore",
        registry_id: "minecraft:deepslate_coal_ore",
        color_hex: "404040",
    },
    BlockEspPreset {
        display_name: "Copper Ore",
        registry_id: "minecraft:copper_ore",
        color_hex: "E08552",
    },
    BlockEspPreset {
        display_name: "Deepslate Copper Ore",
        registry_id: "minecraft:deepslate_copper_ore",
        color_hex: "E08552",
    },
    BlockEspPreset {
        display_name: "Ancient Debris",
        registry_id: "minecraft:ancient_debris",
        color_hex: "9B6A4A",
    },
    BlockEspPreset {
        display_name: "Nether Gold Ore",
        registry_id: "minecraft:nether_gold_ore",
        color_hex: "FFD700",
    },
    BlockEspPreset {
        display_name: "Nether Quartz Ore",
        registry_id: "minecraft:nether_quartz_ore",
        color_hex: "EDE5DC",
    },
    BlockEspPreset {
        display_name: "Spawner",
        registry_id: "minecraft:spawner",
        color_hex: "C03030",
    },
];

/// The default working list: every preset present, only Diamond Ore enabled.
#[must_use]
pub fn build_default_targets() -> Vec<BlockEspTarget> {
    PRESETS
        .iter()
        .map(|preset| {
            let enabled = preset
                .registry_id
                .eq_ignore_ascii_case("minecraft:diamond_ore");
            BlockEspTarget::new(
                preset.registry_id,
                preset.display_name,
                Some(preset.color_hex),
                enabled,
            )
        })
        .collect()
}

/// Reduces any block identifier form to a lowercase path token.
///
/// * `minecraft:diamond_ore` -> `diamond_ore`
/// * `diamond_ore` -> `diamond_ore`
/// * `block.minecraft.diamond_ore` -> `diamond_ore`
#[must_use]
pub fn normalize_id(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return String::new();
    };
    if raw.trim().is_empty() {
        return String::new();
    }

    let mut value = raw.trim().to_lowercase();

    // Translation-key form: "block.<namespace>.<path>" -> drop the "block." prefix.
    if let Some(stripped) = value.strip_prefix("block.") {
        value = stripped.to_owned();
    }

    value = match value.find(':') {
        Some(colon) => value[colon + 1..].to_owned(),
        None => match value.find('.') {
            // After stripping "block.", a translation key looks like
            // "minecraft.diamond_ore": drop the leading namespace component.
            Some(dot) => value[dot + 1..].to_owned(),
            None => value,
        },
    };

    // Keep only [a-z0-9_]; '.' (sub-paths) and '/' collapse to '_'.
    let mut cleaned = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
            cleaned.push(character);
        } else if character == '.' || character == '/' {
            cleaned.push('_');
        }
    }

    cleaned.trim_matches('_').to_owned()
}

/// Normalizes a colour to a 6-digit uppercase `RRGGBB` string with no `#`.
///
/// Returns [`DEFAULT_COLOR_HEX`] when the input is invalid.
#[must_use]
pub fn normalize_color(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return DEFAULT_COLOR_HEX.to_owned();
    };
    if raw.trim().is_empty() {
        return DEFAULT_COLOR_HEX.to_owned();
    }

    let mut value = raw.trim();
    if let Some(stripped) = value.strip_prefix('#') {
        value = stripped;
    }

    if value.len() != 6 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return DEFAULT_COLOR_HEX.to_owned();
    }

    value.to_ascii_uppercase()
}

/// Serialises the enabled targets to the delimited wire string.
///
/// Disabled entries are skipped; ids are normalised and de-duplicated. Once the next entry would
/// take the string past [`MAX_SERIALIZED_LENGTH`] the rest are dropped, which is the managed
/// `Serialize`'s own `break` (`Aoko/Core/BlockEsp.cs:264-265`): the cap bounds the bridge payload,
/// and it is the one way a target leaves the bridge's list without the GUI saying so.
#[must_use]
pub fn serialize_targets(targets: &[BlockEspTarget]) -> String {
    let mut seen: Vec<String> = Vec::with_capacity(targets.len());
    let mut out = String::with_capacity(targets.len() * 24);

    for target in targets {
        if !target.enabled() {
            continue;
        }

        let id = normalize_id(Some(target.registry_id()));
        if id.is_empty() || seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());

        let color = normalize_color(Some(target.color_hex()));
        let entry_length = id.len() + 1 + color.len();

        // Stop before exceeding the cap (account for the ';' separator).
        let projected = out.len() + usize::from(!out.is_empty()) + entry_length;
        if projected > MAX_SERIALIZED_LENGTH {
            break;
        }

        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(&id);
        out.push('=');
        out.push_str(&color);
    }

    out
}

/// Parses the delimited wire string back into `(normalized id, colour)` pairs.
///
/// A malformed entry — no `=`, or nothing before it — is skipped, and so is an id that reduces to
/// nothing or that an earlier entry already claimed: the first spelling of a block wins, which is
/// the order [`serialize_targets`] writes.
#[must_use]
pub fn parse_targets(serialized: Option<&str>) -> Vec<(String, String)> {
    let Some(serialized) = serialized else {
        return Vec::new();
    };
    if serialized.trim().is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    for raw_entry in serialized.split(';') {
        if raw_entry.is_empty() {
            continue;
        }

        let Some(equals) = raw_entry.find('=') else {
            continue;
        };
        if equals == 0 {
            continue;
        }

        let id = normalize_id(Some(&raw_entry[..equals]));
        if id.is_empty() || result.iter().any(|(existing, _)| *existing == id) {
            continue;
        }

        let color = normalize_color(Some(&raw_entry[equals + 1..]));
        result.push((id, color));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A key this build does not know on one entry survives the load and the save after it.
    #[test]
    fn an_unknown_key_survives_inside_one_entry() {
        let text = r#"[{"registryId":"minecraft:gold_ore","enabled":true,"futureMarker":"x"}]"#;
        let entries: Vec<BlockEspTargetData> = serde_json::from_str(text).expect("deserialise");
        let entry = entries.first().expect("one entry");

        assert!(entry.enabled, "the fields this build knows still load");
        assert_eq!(
            entry.unknown.get("futureMarker"),
            Some(&Value::String("x".to_owned()))
        );

        let saved = serde_json::to_value(&entries).expect("serialise");
        assert_eq!(saved[0]["futureMarker"], json!("x"));
        assert_eq!(saved[0]["registryId"], json!("minecraft:gold_ore"));

        let again: Vec<BlockEspTargetData> = serde_json::from_value(saved).expect("deserialise");
        assert_eq!(again, entries, "the entry round-trips as it was read");
    }
}
