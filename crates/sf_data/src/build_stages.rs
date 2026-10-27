//! How a structure under construction looks (WP6.3), in `assets/config/build_stages.ron`.
//!
//! The simulation knows only how far along a construction is (`sf_sim::Construction`, thousandths).
//! This table says which *picture* goes with a progress: the stages in order, from which thousandth
//! each begins, the suffix that turns the file name of the finished model into the stage's model
//! (`set_v4/meridian_barracks.glb` and the suffix `_foundation` give
//! `set_v4/meridian_barracks_foundation.glb`), and what a structure that has no such model is drawn
//! as instead. The finished model is drawn from the tick the simulation says the structure is
//! complete, not from a progress of 1000. Presentation only: no simulation number lives here, and
//! no checksum depends on it (game data in RON, PLAN.md section 5 rule 6).
//!
//! ```
//! use sf_data::build_stages::BuildStages;
//!
//! let stages = BuildStages::embedded();
//! assert_eq!(stages.stage_at(0), 0);
//! assert_eq!(
//!     stages.model_of("set_v4/meridian_barracks.glb", 0).as_deref(),
//!     Some("set_v4/meridian_barracks_foundation.glb")
//! );
//! ```

use serde::Deserialize;

/// The shipped file, compiled in, like the other tables.
pub const EMBEDDED_BUILD_STAGES: &str = include_str!("../../../assets/config/build_stages.ron");

/// One stage of a construction.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    /// What the stage is called (screenshots, the log).
    pub id: String,
    /// The progress, in thousandths, from which the stage is shown (until the next one begins).
    pub from_permille: u16,
    /// Put before `.glb` in the finished model's file name to get this stage's model.
    pub suffix: String,
    /// When the stage has no model: the placeholder box is drawn this many thousandths of the
    /// finished one's height (the footprint stays).
    pub fallback_height_permille: u16,
}

/// The colours of the progress bar over a structure under construction (sRGB, alpha last).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BarLook {
    /// The part that is done.
    pub fill: (u8, u8, u8, u8),
    /// The rest of the bar, and its border.
    pub back: (u8, u8, u8, u8),
    /// Height of the bar in logical pixels.
    pub height_px: u16,
}

/// The table.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildStages {
    /// The stages in order of progress.
    pub stages: Vec<Stage>,
    /// The progress bar.
    pub bar: BarLook,
}

impl BuildStages {
    /// Reads and checks a table. The error says what is wrong.
    pub fn parse(text: &str) -> Result<BuildStages, String> {
        let table: BuildStages = ron::from_str(text).map_err(|e| e.to_string())?;
        table.check()?;
        Ok(table)
    }

    /// The shipped table. Panics if the shipped file is wrong (a test reads it).
    pub fn embedded() -> BuildStages {
        BuildStages::parse(EMBEDDED_BUILD_STAGES)
            .unwrap_or_else(|e| panic!("assets/config/build_stages.ron: {e}"))
    }

    fn check(&self) -> Result<(), String> {
        let first = self.stages.first().ok_or("no stages")?;
        if first.from_permille != 0 {
            return Err(format!(
                "stage \"{}\" is the first and must begin at 0, not {}",
                first.id, first.from_permille
            ));
        }
        for (i, s) in self.stages.iter().enumerate() {
            if s.id.is_empty() || self.stages[..i].iter().any(|o| o.id == s.id) {
                return Err(format!(
                    "stage {i}: the id \"{}\" is empty or used twice",
                    s.id
                ));
            }
            if s.suffix.is_empty() || s.suffix.contains(['/', '\\', '.']) {
                return Err(format!(
                    "stage \"{}\": suffix \"{}\" must be a name part without / or .",
                    s.id, s.suffix
                ));
            }
            if s.from_permille >= 1000 {
                return Err(format!(
                    "stage \"{}\": begins at {}, which is the finished structure",
                    s.id, s.from_permille
                ));
            }
            if !(1..=1000).contains(&s.fallback_height_permille) {
                return Err(format!(
                    "stage \"{}\": fallback_height_permille {} must be 1 to 1000",
                    s.id, s.fallback_height_permille
                ));
            }
            if i > 0 && s.from_permille <= self.stages[i - 1].from_permille {
                return Err(format!(
                    "stage \"{}\" begins at {}, not after the stage before it",
                    s.id, s.from_permille
                ));
            }
        }
        if self.bar.height_px == 0 {
            return Err("bar.height_px must be above 0".into());
        }
        Ok(())
    }

    /// The index of the stage shown at `permille` thousandths: the last one that has begun.
    pub fn stage_at(&self, permille: u16) -> usize {
        self.stages
            .iter()
            .rposition(|s| s.from_permille <= permille)
            .unwrap_or(0)
    }

    /// The model file of stage `stage` for a structure whose finished model is `finished`
    /// (`dir/name.glb`): the suffix goes before the extension. `None` for a file without `.glb` or
    /// a stage that does not exist.
    pub fn model_of(&self, finished: &str, stage: usize) -> Option<String> {
        let base = finished.strip_suffix(".glb")?;
        let s = self.stages.get(stage)?;
        Some(format!("{base}{}.glb", s.suffix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"(
        stages: [
            (id: "a", from_permille: 0, suffix: "_a", fallback_height_permille: 100),
            (id: "b", from_permille: 400, suffix: "_b", fallback_height_permille: 600),
        ],
        bar: (fill: (1, 2, 3, 4), back: (5, 6, 7, 8), height_px: 5),
    )"#;

    fn err(text: &str) -> String {
        BuildStages::parse(text).unwrap_err()
    }

    #[test]
    fn the_shipped_table_loads() {
        let t = BuildStages::embedded();
        assert_eq!(t.stages.len(), 2);
        assert_eq!(t.stages[0].id, "foundation");
        assert_eq!(t.stages[1].id, "scaffolding");
    }

    #[test]
    fn the_stage_is_the_last_one_that_has_begun() {
        let t = BuildStages::parse(GOOD).unwrap();
        assert_eq!(t.stage_at(0), 0);
        assert_eq!(t.stage_at(399), 0);
        assert_eq!(t.stage_at(400), 1);
        assert_eq!(t.stage_at(999), 1);
    }

    #[test]
    fn the_model_name_comes_from_the_data() {
        let t = BuildStages::parse(GOOD).unwrap();
        assert_eq!(t.model_of("x/y.glb", 1).as_deref(), Some("x/y_b.glb"));
        assert_eq!(t.model_of("x/y.glb", 2), None);
        assert_eq!(t.model_of("x/y.png", 0), None);
    }

    #[test]
    fn a_bad_table_is_refused_with_the_reason() {
        assert!(err(&GOOD.replace("from_permille: 0", "from_permille: 10")).contains("begin at 0"));
        assert!(err(&GOOD.replace("from_permille: 400", "from_permille: 0")).contains("not after"));
        assert!(err(&GOOD.replace("id: \"b\"", "id: \"a\"")).contains("used twice"));
        assert!(err(&GOOD.replace("\"_b\"", "\"/b\"")).contains("suffix"));
        assert!(err(&GOOD.replace("400", "1000")).contains("finished"));
        assert!(err(&GOOD.replace("600", "0")).contains("fallback_height_permille"));
        assert!(err(&GOOD.replace("height_px: 5", "height_px: 0")).contains("height_px"));
        assert!(
            err("(stages: [], bar: (fill: (0,0,0,0), back: (0,0,0,0), height_px: 1))")
                .contains("no stages")
        );
    }
}
