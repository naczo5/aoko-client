//! HUD layout sync, ported from `Aoko/Core/HudLayout.cs`.
//!
//! X and Y are screen-fraction anchors in `[0, 1]`; Scale is a size multiplier in
//! `[0.5, 2.0]`. Interaction constraints (movable/resizable) live bridge-side and
//! are deliberately not modelled here.
//!
//! Serialisation order is fixed to the element-id order the managed host emits,
//! because the bridge current-layout comparison is order-insensitive but the
//! byte-level config line is not.

use serde_json::Value;

/// Stable lowercase element ids shared with the C++ bridge (`hud_layout.h`).
pub mod element_id {
    pub const MODULE_LIST: &str = "modulelist";
    pub const CLOSEST_PLAYER: &str = "closestplayer";
    pub const PIXEL_PARTY: &str = "pixelparty";
    pub const CHEST_ESP_LIST: &str = "chestesplist";
    pub const GTB_HINT: &str = "gtbhint";
    pub const NAMETAGS: &str = "nametags";
    pub const BLOCK_ESP_LIST: &str = "blockesplist";
}

/// Emission order. Also the order `ToJson` guarantees.
pub const KNOWN_ELEMENT_IDS: [&str; 7] = [
    element_id::MODULE_LIST,
    element_id::CLOSEST_PLAYER,
    element_id::PIXEL_PARTY,
    element_id::CHEST_ESP_LIST,
    element_id::GTB_HINT,
    element_id::NAMETAGS,
    element_id::BLOCK_ESP_LIST,
];

/// Lower bound on a scale multiplier.
pub const MIN_SCALE: f64 = 0.5;
/// Upper bound on a scale multiplier.
pub const MAX_SCALE: f64 = 2.0;

/// Tolerance used by the echo guard.
const EQUALITY_TOLERANCE: f64 = 1e-9;

/// Normalized placement for a single HUD element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudElementLayout {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

impl HudElementLayout {
    #[must_use]
    pub fn new(x: f64, y: f64, scale: f64) -> Self {
        Self { x, y, scale }
    }
}

impl Default for HudElementLayout {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
        }
    }
}

/// Whole-HUD layout keyed by stable element id.
#[derive(Debug, Clone, PartialEq)]
pub struct HudLayout {
    elements: [HudElementLayout; 7],
}

impl Default for HudLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl HudLayout {
    /// Canonical default layout, exactly as `HudLayout.BuildDefaults`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: [
                HudElementLayout::new(0.985, 0.020, 1.0),
                HudElementLayout::new(0.500, 0.780, 1.0),
                HudElementLayout::new(0.500, 0.660, 1.0),
                HudElementLayout::new(0.015, 0.300, 1.0),
                HudElementLayout::new(0.500, 0.620, 1.0),
                HudElementLayout::new(0.000, 0.000, 1.0),
                HudElementLayout::new(0.015, 0.560, 1.0),
            ],
        }
    }

    /// The layout for one element, or its canonical default when unknown.
    #[must_use]
    pub fn get(&self, element_id: &str) -> HudElementLayout {
        index_of(element_id).map_or_else(HudElementLayout::default, |index| self.elements[index])
    }

    /// Stores a clamped copy of `value`. NaN/Infinity become the element default.
    ///
    /// Unknown ids are ignored: `HudElementId` is a closed set shared with the
    /// bridge, so routing an unknown id here is a programming error rather than a
    /// wire case.
    pub fn set(&mut self, element_id: &str, value: HudElementLayout) {
        let Some(index) = index_of(element_id) else {
            return;
        };
        let default = HudLayout::new().elements[index];
        self.elements[index] = HudElementLayout {
            x: clamp_coord(value.x, default.x),
            y: clamp_coord(value.y, default.y),
            scale: clamp_scale(value.scale, default.scale),
        };
    }

    /// Restores one element to its canonical default anchor and scale.
    pub fn reset_element(&mut self, element_id: &str) {
        if let Some(index) = index_of(element_id) {
            self.elements[index] = HudLayout::new().elements[index];
        }
    }

    /// Restores every element to its canonical defaults.
    pub fn reset_all(&mut self) {
        *self = Self::new();
    }

    /// Emits all seven element ids as a JSON object.
    pub fn write_wire(&self, out: &mut String) {
        out.push('{');
        for (index, id) in KNOWN_ELEMENT_IDS.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(id);
            out.push_str("\":{\"x\":");
            push_coordinate(out, self.elements[index].x);
            out.push_str(",\"y\":");
            push_coordinate(out, self.elements[index].y);
            out.push_str(",\"scale\":");
            push_coordinate(out, self.elements[index].scale);
            out.push('}');
        }
        out.push('}');
    }

    /// The layout as JSON, for callers that need a value rather than bytes.
    #[must_use]
    pub fn to_json_value(&self) -> Value {
        let mut text = String::new();
        self.write_wire(&mut text);
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    /// Decodes a layout from JSON.
    ///
    /// Missing elements take their canonical default; unknown ids are ignored.
    /// A value that fails to parse falls back to that element's default, and
    /// every stored value is clamped — so a hand-edited profile cannot move an
    /// element off-screen.
    #[must_use]
    pub fn from_json(node: &Value) -> Self {
        let mut layout = Self::new();
        let Value::Object(object) = node else {
            return layout;
        };

        for (id, value) in object {
            let Some(index) = index_of(id) else {
                continue;
            };
            let Value::Object(element) = value else {
                continue;
            };

            let default = layout.elements[index];
            let x = read_double(element, "x", default.x);
            let y = read_double(element, "y", default.y);
            let scale = read_double(element, "scale", default.scale);

            layout.elements[index] = HudElementLayout {
                x: clamp_coord(x, default.x),
                y: clamp_coord(y, default.y),
                scale: clamp_scale(scale, default.scale),
            };
        }

        layout
    }

    /// Echo guard: whether every anchor and scale agrees within 1e-9.
    #[must_use]
    pub fn equals_layout(&self, other: &Self) -> bool {
        self.elements
            .iter()
            .zip(other.elements.iter())
            .all(|(a, b)| {
                (a.x - b.x).abs() <= EQUALITY_TOLERANCE
                    && (a.y - b.y).abs() <= EQUALITY_TOLERANCE
                    && (a.scale - b.scale).abs() <= EQUALITY_TOLERANCE
            })
    }
}

fn index_of(element_id: &str) -> Option<usize> {
    KNOWN_ELEMENT_IDS
        .iter()
        .position(|known| known.eq_ignore_ascii_case(element_id))
}

fn clamp_coord(value: f64, fallback: f64) -> f64 {
    if value.is_nan() || value.is_infinite() {
        return fallback;
    }
    value.clamp(0.0, 1.0)
}

fn clamp_scale(value: f64, fallback: f64) -> f64 {
    if value.is_nan() || value.is_infinite() || value == 0.0 {
        return fallback;
    }
    value.clamp(MIN_SCALE, MAX_SCALE)
}

/// Writes a coordinate the way `System.Text.Json` writes a `double`: shortest
/// round-trip form, and a whole number without a fractional part.
fn push_coordinate(out: &mut String, value: f64) {
    if !value.is_finite() {
        out.push('0');
        return;
    }
    out.push_str(&format!("{value}"));
}

fn read_double(element: &serde_json::Map<String, Value>, key: &str, fallback: f64) -> f64 {
    element.get(key).and_then(Value::as_f64).unwrap_or(fallback)
}
