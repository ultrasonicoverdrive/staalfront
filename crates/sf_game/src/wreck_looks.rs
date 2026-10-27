//! How what destruction leaves is drawn (WP5.5): the wreck colour, the wreck model of a vehicle by its
//! size and the rubble model of a structure by its footprint. The numbers live in
//! `assets/config/wrecks.ron`, which is compiled in like `units.ron` (sf_render's `EMBEDDED_STYLE`) and
//! checked when it is parsed. Presentation only: nothing here reaches the simulation.

use std::sync::OnceLock;

use serde::Deserialize;

/// The text of `assets/config/wrecks.ron`, compiled in.
pub const EMBEDDED_WRECK_LOOKS: &str = include_str!("../../../assets/config/wrecks.ron");

/// The most vehicle wreck classes: their looks take the kind numbers `0xFFF0..=0xFFFF`.
pub const MAX_VEHICLE_CLASSES: usize = 16;

/// The largest side of a rubble footprint: the kind number of a rubble look packs the two sides in
/// four bits each.
pub const MAX_RUBBLE_SIDE: u8 = 15;

/// One class of vehicle wreck.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VehicleWreck {
    /// What the F1 panel calls it (`wreck_light_vehicle`).
    pub id: String,
    /// The glTF file under `assets/`.
    pub model: String,
    /// The size, in tenths of a cell, of the vehicle the primitive stands in for when the model is missing.
    pub size_tenths: i64,
    /// A vehicle at most this many tenths of a cell across leaves this wreck; `None` for the last class.
    pub up_to_tenths: Option<i64>,
}

/// The rubble model of one footprint.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Rubble {
    /// Cells wide, cells deep.
    pub footprint: (u8, u8),
    /// The glTF file under `assets/`.
    pub model: String,
}

/// The whole of `wrecks.ron`, checked.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WreckLooks {
    /// The sRGB of the wreck colour.
    pub colour: (u8, u8, u8),
    /// The vehicle wreck classes, light to heavy.
    pub vehicles: Vec<VehicleWreck>,
    /// The rubble models by footprint.
    pub rubble: Vec<Rubble>,
}

impl WreckLooks {
    /// Parses and checks `text`: at least one and at most [`MAX_VEHICLE_CLASSES`] vehicle classes with
    /// distinct ids, positive sizes, bounds that rise and only the last class unbounded; at least one
    /// rubble model, footprints of 1 to [`MAX_RUBBLE_SIDE`] cells a side, each once; every model a `.glb`.
    pub fn from_ron(text: &str) -> Result<WreckLooks, String> {
        let looks: WreckLooks = ron::from_str(text).map_err(|e| e.to_string())?;
        looks.check()?;
        Ok(looks)
    }

    /// The compiled-in `wrecks.ron`. A test keeps it valid, so this cannot fail in a build that passed.
    pub fn shipped() -> &'static WreckLooks {
        static SHIPPED: OnceLock<WreckLooks> = OnceLock::new();
        SHIPPED.get_or_init(|| {
            WreckLooks::from_ron(EMBEDDED_WRECK_LOOKS)
                .unwrap_or_else(|e| panic!("assets/config/wrecks.ron: {e}"))
        })
    }

    fn check(&self) -> Result<(), String> {
        let n = self.vehicles.len();
        if n == 0 || n > MAX_VEHICLE_CLASSES {
            return Err(format!(
                "vehicles: {n} classes, want 1 to {MAX_VEHICLE_CLASSES}"
            ));
        }
        let mut last_bound: Option<i64> = None;
        for (i, v) in self.vehicles.iter().enumerate() {
            if v.id.is_empty() || self.vehicles[..i].iter().any(|o| o.id == v.id) {
                return Err(format!("vehicles[{i}]: id \"{}\" empty or twice", v.id));
            }
            check_model(&v.model).map_err(|e| format!("vehicles[{i}] ({}): {e}", v.id))?;
            if v.size_tenths <= 0 {
                return Err(format!("{}: size_tenths must be above 0", v.id));
            }
            match (v.up_to_tenths, i + 1 == n) {
                (None, true) => {}
                (None, false) => {
                    return Err(format!("{}: only the last class may have no bound", v.id));
                }
                (Some(_), true) => {
                    return Err(format!("{}: the last class must have no bound", v.id));
                }
                (Some(b), false) => {
                    if b <= 0 || last_bound.is_some_and(|l| b <= l) {
                        return Err(format!(
                            "{}: up_to_tenths {b} must be above 0 and above the class before",
                            v.id
                        ));
                    }
                    last_bound = Some(b);
                }
            }
        }
        if self.rubble.is_empty() {
            return Err("rubble: no models".to_owned());
        }
        for (i, r) in self.rubble.iter().enumerate() {
            let (w, h) = r.footprint;
            if !(1..=MAX_RUBBLE_SIDE).contains(&w) || !(1..=MAX_RUBBLE_SIDE).contains(&h) {
                return Err(format!(
                    "rubble[{i}]: footprint {w}x{h}, each side must be 1 to {MAX_RUBBLE_SIDE}"
                ));
            }
            if self.rubble[..i].iter().any(|o| o.footprint == r.footprint) {
                return Err(format!("rubble[{i}]: footprint {w}x{h} twice"));
            }
            check_model(&r.model).map_err(|e| format!("rubble[{i}]: {e}"))?;
        }
        Ok(())
    }

    /// The index of the vehicle class a vehicle `tenths` tenths of a cell across leaves.
    pub fn vehicle_class(&self, tenths: i64) -> usize {
        self.vehicles
            .iter()
            .position(|v| v.up_to_tenths.is_none_or(|b| tenths <= b))
            .unwrap_or(self.vehicles.len() - 1)
    }

    /// The rubble file whose area is nearest to `w` x `h` (the smaller one on a tie).
    pub fn rubble_file(&self, w: u8, h: u8) -> &str {
        let area = |(a, b): (u8, u8)| i32::from(a) * i32::from(b);
        let want = area((w, h));
        self.rubble
            .iter()
            .min_by_key(|r| ((area(r.footprint) - want).abs(), area(r.footprint)))
            .map_or("", |r| r.model.as_str())
    }
}

fn check_model(model: &str) -> Result<(), String> {
    if model.len() > 4 && model.ends_with(".glb") && !model.starts_with('/') {
        Ok(())
    } else {
        Err(format!("model \"{model}\" must be a relative .glb path"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The numbers the game ships with (WP5.5), pinned: a change to wrecks.ron is a deliberate one.
    #[test]
    fn the_shipped_wreck_looks_are_valid_and_pinned() {
        let w = WreckLooks::shipped();
        assert_eq!(w.colour, (70, 66, 62));
        let classes: Vec<_> = w
            .vehicles
            .iter()
            .map(|v| {
                (
                    v.id.as_str(),
                    v.model.as_str(),
                    v.size_tenths,
                    v.up_to_tenths,
                )
            })
            .collect();
        assert_eq!(
            classes,
            [
                (
                    "wreck_light_vehicle",
                    "set_v3/neutral_wreck_light_vehicle.glb",
                    12,
                    Some(12)
                ),
                (
                    "wreck_medium_vehicle",
                    "set_v3/neutral_wreck_medium_vehicle.glb",
                    13,
                    Some(13)
                ),
                (
                    "wreck_heavy_vehicle",
                    "set_v3/neutral_wreck_heavy_vehicle.glb",
                    15,
                    None
                ),
            ]
        );
        let rubble: Vec<_> = w
            .rubble
            .iter()
            .map(|r| (r.footprint, r.model.as_str()))
            .collect();
        assert_eq!(
            rubble,
            [
                ((1, 1), "set_v4/neutral_rubble_1x1.glb"),
                ((2, 2), "set_v4/neutral_rubble_2x2.glb"),
                ((2, 3), "set_v4/neutral_rubble_2x3.glb"),
                ((3, 4), "set_v4/neutral_rubble_3x4.glb"),
                ((4, 4), "set_v4/neutral_rubble_4x4.glb"),
                ((4, 5), "set_v4/neutral_rubble_4x5.glb"),
            ]
        );
        // Every model the file names is on disk.
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for file in w
            .vehicles
            .iter()
            .map(|v| &v.model)
            .chain(w.rubble.iter().map(|r| &r.model))
        {
            assert!(models.join(file).is_file(), "{file} missing");
        }
    }

    #[test]
    fn a_vehicle_takes_the_first_class_whose_bound_it_fits_and_rubble_the_nearest_area() {
        let w = WreckLooks::shipped();
        assert_eq!(
            [10, 12, 13, 14, 40].map(|t| w.vehicle_class(t)),
            [0, 0, 1, 2, 2]
        );
        assert_eq!(w.rubble_file(1, 1), "set_v4/neutral_rubble_1x1.glb");
        assert_eq!(w.rubble_file(3, 3), "set_v4/neutral_rubble_2x3.glb"); // 9: 6 and 12 tie, smaller wins
        assert_eq!(w.rubble_file(6, 6), "set_v4/neutral_rubble_4x5.glb");
    }

    /// Each broken edit of the shipped file is refused when it is parsed.
    #[test]
    fn a_broken_wrecks_file_is_refused() {
        let ok = EMBEDDED_WRECK_LOOKS;
        let broken = [
            // Bounds that do not rise.
            ok.replacen("up_to_tenths: Some(13)", "up_to_tenths: Some(12)", 1),
            // An unbounded class before the last.
            ok.replacen("up_to_tenths: Some(12)", "up_to_tenths: None", 1),
            // A bounded last class.
            ok.replacen("up_to_tenths: None", "up_to_tenths: Some(20)", 1),
            // A zero size.
            ok.replacen("size_tenths: 15", "size_tenths: 0", 1),
            // A footprint twice, and one out of range.
            ok.replacen("footprint: (2, 2)", "footprint: (1, 1)", 1),
            ok.replacen("footprint: (4, 5)", "footprint: (4, 16)", 1),
            // A model that is no glb.
            ok.replacen("neutral_rubble_1x1.glb", "neutral_rubble_1x1.obj", 1),
            // An id twice.
            ok.replacen("wreck_medium_vehicle\"", "wreck_light_vehicle\"", 1),
            // An unknown field.
            ok.replacen("colour:", "color2: (0, 0, 0),\n    colour:", 1),
        ];
        for (i, text) in broken.iter().enumerate() {
            assert_ne!(text, ok, "edit {i} did not apply");
            assert!(WreckLooks::from_ron(text).is_err(), "edit {i} was accepted");
        }
        assert!(WreckLooks::from_ron(ok).is_ok());
    }
}
