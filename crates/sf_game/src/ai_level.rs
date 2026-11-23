//! The difficulty levels of the computer opponent (WP9.4c): which shipped AI profiles count as levels, read
//! from `assets/config/ai_levels.ron` (compiled in, checked when it is parsed). One place: `--ai <level>`
//! ([`crate::args`]), the start menu ([`crate::menu`]) and the match loader ([`crate::matchplay`]) all ask
//! here. Presentation and wiring only: the numbers of a level stay in its profile file (`assets/ai/`), and
//! nothing in this module reaches the simulation. The rules are in `docs/rules/ai.md`.

use std::sync::OnceLock;

use serde::Deserialize;

/// The text of `assets/config/ai_levels.ron`, compiled in.
pub const EMBEDDED_AI_LEVELS: &str = include_str!("../../../assets/config/ai_levels.ron");

/// The match file the start menu's "against the computer" entry plays (a bare name under
/// `assets/matches`). Its AI seat gets the level the player chose.
pub const AI_MATCH: &str = "m1_ai";

/// The levels and the default.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AiLevels {
    /// The level of a player who never chose one.
    pub default: String,
    /// The profile names that count as levels, in the order the menu cycles them.
    pub levels: Vec<String>,
}

impl AiLevels {
    /// Parses and checks a level list.
    pub fn from_ron(text: &str) -> Result<AiLevels, String> {
        let levels: AiLevels = ron::from_str(text).map_err(|e| format!("ai_levels.ron: {e}"))?;
        levels.check()?;
        Ok(levels)
    }

    fn check(&self) -> Result<(), String> {
        if self.levels.is_empty() {
            return Err("ai_levels.ron: no levels".into());
        }
        for (i, name) in self.levels.iter().enumerate() {
            let plain = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !plain {
                return Err(format!(
                    "ai_levels.ron: level \"{name}\" must be a profile name of lower-case letters, digits and '_'"
                ));
            }
            if self.levels[..i].contains(name) {
                return Err(format!("ai_levels.ron: level \"{name}\" is listed twice"));
            }
        }
        if !self.levels.contains(&self.default) {
            return Err(format!(
                "ai_levels.ron: the default \"{}\" is not one of the levels",
                self.default
            ));
        }
        Ok(())
    }

    /// The levels compiled into the binary.
    pub fn embedded() -> &'static AiLevels {
        static LEVELS: OnceLock<AiLevels> = OnceLock::new();
        LEVELS.get_or_init(|| {
            AiLevels::from_ron(EMBEDDED_AI_LEVELS).expect("the shipped ai_levels.ron is valid")
        })
    }

    /// The shipped level called `name`, `None` if it is not one.
    pub fn find(&self, name: &str) -> Option<&str> {
        self.levels.iter().find(|l| *l == name).map(String::as_str)
    }

    /// The level after `name` in the list, wrapping round; the default when `name` is not a level.
    pub fn next_after(&self, name: &str) -> &str {
        match self.levels.iter().position(|l| l == name) {
            Some(i) => &self.levels[(i + 1) % self.levels.len()],
            None => &self.default,
        }
    }

    /// `name` if it is a level, else the default (a settings file from another version).
    pub fn or_default<'a>(&'a self, name: Option<&str>) -> &'a str {
        name.and_then(|n| self.find(n)).unwrap_or(&self.default)
    }

    /// The levels as a list for a message: `easy, normal, hard`.
    pub fn list_text(&self) -> String {
        self.levels.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped list, pinned: a renamed, reordered or added level fails here.
    #[test]
    fn the_shipped_levels_are_easy_normal_hard_with_normal_as_the_default() {
        let l = AiLevels::embedded();
        assert_eq!(l.levels, ["easy", "normal", "hard"]);
        assert_eq!(l.default, "normal");
        assert_eq!(l.list_text(), "easy, normal, hard");
    }

    #[test]
    fn every_level_is_a_shipped_profile_file() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for name in &AiLevels::embedded().levels {
            let file = assets.join("ai").join(format!("{name}.ron"));
            assert!(file.is_file(), "{} is not a file", file.display());
        }
    }

    #[test]
    fn the_menu_cycles_the_levels_and_falls_back_to_the_default() {
        let l = AiLevels::embedded();
        assert_eq!(l.next_after("easy"), "normal");
        assert_eq!(l.next_after("normal"), "hard");
        assert_eq!(l.next_after("hard"), "easy");
        assert_eq!(l.next_after("brutal"), "normal");
        assert_eq!(l.or_default(None), "normal");
        assert_eq!(l.or_default(Some("hard")), "hard");
        assert_eq!(l.or_default(Some("brutal")), "normal");
        assert_eq!(l.find("easy"), Some("easy"));
        assert_eq!(l.find("Easy"), None);
    }

    #[test]
    fn a_list_that_cannot_work_is_refused() {
        assert!(AiLevels::from_ron("(default: \"a\", levels: [])").is_err());
        assert!(AiLevels::from_ron("(default: \"b\", levels: [\"a\"])").is_err());
        assert!(AiLevels::from_ron("(default: \"a\", levels: [\"a\", \"a\"])").is_err());
        assert!(AiLevels::from_ron("(default: \"A\", levels: [\"A\"])").is_err());
        assert!(AiLevels::from_ron("(default: \"a\", levels: [\"a\"], extra: 1)").is_err());
        assert!(AiLevels::from_ron("not ron").is_err());
        assert!(AiLevels::from_ron("(default: \"a\", levels: [\"a\"])").is_ok());
    }
}
