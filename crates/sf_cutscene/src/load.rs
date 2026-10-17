//! Reading a scene from RON text or a file, with errors a writer can act on.
//!
//! * Syntax and shape errors (a missing field, an unknown field, a wrong
//!   variant) come from `ron` with their line and column.
//! * A scene that parses but is wrong (unknown character, overlapping lines,
//!   negative time...) comes back as a list of [`Issue`]s, each with the line of
//!   the offending value.
//! * Missing asset files are **not** an error; they are listed in
//!   [`LoadedScene::missing_assets`] when an asset root is given.
//!
//! Errors print like compiler messages (`file:line: message`), so editors can
//! jump to them.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::model::Scene;
use crate::span::SpanIndex;
use crate::validate::{self, Issue, MissingAsset};

/// A scene that loaded and validated, plus what to look at anyway.
#[derive(Clone, Debug)]
pub struct LoadedScene {
    pub scene: Scene,
    /// Things that are legal but probably not intended.
    pub warnings: Vec<Issue>,
    /// Referenced files not found under the asset root (empty when no root was
    /// given). Placeholders are allowed, so this is information, not failure.
    pub missing_assets: Vec<MissingAsset>,
}

/// Why a scene did not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneError {
    /// The file could not be read.
    Io { file: String, message: String },
    /// The text is not valid RON for a scene.
    Parse {
        file: Option<String>,
        line: usize,
        column: usize,
        message: String,
    },
    /// The text parsed but the scene is invalid.
    Invalid {
        file: Option<String>,
        issues: Vec<Issue>,
    },
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::Io { file, message } => write!(f, "{file}: {message}"),
            SceneError::Parse {
                file,
                line,
                column,
                message,
            } => {
                let file = file.as_deref().unwrap_or("<scene>");
                write!(f, "{file}:{line}:{column}: {message}")
            }
            SceneError::Invalid { file, issues } => {
                let file = file.as_deref().unwrap_or("<scene>");
                for (n, issue) in issues.iter().enumerate() {
                    if n > 0 {
                        writeln!(f)?;
                    }
                    match issue.line {
                        Some(l) => write!(f, "{file}:{l}: {}", issue.message)?,
                        None => write!(f, "{file}: {}", issue.message)?,
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SceneError {}

impl SceneError {
    /// Attach a file name to the messages.
    pub fn in_file(mut self, name: &str) -> SceneError {
        match &mut self {
            SceneError::Parse { file, .. } | SceneError::Invalid { file, .. } => {
                *file = Some(name.to_string());
            }
            SceneError::Io { .. } => {}
        }
        self
    }
}

/// Parse RON text into a [`Scene`] without validating it.
pub fn parse_str(text: &str) -> Result<Scene, SceneError> {
    ron::Options::default()
        .from_str::<Scene>(text)
        .map_err(|e| SceneError::Parse {
            file: None,
            line: e.span.start.line,
            column: e.span.start.col,
            message: e.code.to_string(),
        })
}

/// Parse and validate a scene from text. With `assets_root` the referenced
/// files are checked for existence too.
pub fn load_str(text: &str, assets_root: Option<&Path>) -> Result<LoadedScene, SceneError> {
    let scene = parse_str(text)?;
    let index = SpanIndex::new(text);
    let mut report = validate::validate(&scene);
    for issue in report.errors.iter_mut().chain(report.warnings.iter_mut()) {
        issue.line = index.line_of(&issue.path);
    }
    if !report.errors.is_empty() {
        return Err(SceneError::Invalid {
            file: None,
            issues: report.errors,
        });
    }
    let missing_assets = match assets_root {
        Some(root) => {
            let mut m = validate::missing_assets(&scene, root);
            for a in &mut m {
                a.line = index.line_of(&a.path);
            }
            m
        }
        None => Vec::new(),
    };
    Ok(LoadedScene {
        scene,
        warnings: report.warnings,
        missing_assets,
    })
}

/// Read, parse and validate a scene file. Error messages carry the file name.
pub fn load_file(path: &Path, assets_root: Option<&Path>) -> Result<LoadedScene, SceneError> {
    let name = path.display().to_string();
    let text = fs::read_to_string(path).map_err(|e| SceneError::Io {
        file: name.clone(),
        message: e.to_string(),
    })?;
    load_str(&text, assets_root).map_err(|e| e.in_file(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r##"Scene(
    name: "t",
    characters: [
        (id: "a", parts: [(name: "body", art: Rect(w: 10, h: 20, color: "#ff0000"))]),
    ],
    timeline: (tracks: [
        (name: "stage", events: [
            (at: 0, kind: Enter(character: "a", pos: (0, 0))),
        ]),
    ]),
)"##;

    #[test]
    fn minimal_scene_loads_with_defaults() {
        let l = load_str(MINIMAL, None).unwrap();
        assert_eq!(l.scene.format, 1);
        assert_eq!(l.scene.stage.width, 1920);
        assert_eq!(l.scene.characters[0].scale, 1.0);
        assert!(l.warnings.is_empty());
        assert!(l.missing_assets.is_empty());
    }

    #[test]
    fn syntax_error_has_line_and_column() {
        let text = MINIMAL.replace("pos: (0, 0)", "pos: (0, 0");
        let err = load_str(&text, None).unwrap_err();
        match err {
            SceneError::Parse { line, .. } => assert!(line >= 7, "line {line}"),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_field_is_a_parse_error_with_the_line() {
        let text = MINIMAL.replace("name: \"t\",", "name: \"t\",\n    colour: 3,");
        let err = load_str(&text, None).unwrap_err();
        let SceneError::Parse { line, message, .. } = &err else {
            panic!("expected a parse error, got {err:?}");
        };
        assert_eq!(*line, 3, "{err}");
        assert!(message.contains("colour"), "{message}");
    }

    #[test]
    fn unknown_character_is_reported_on_its_line() {
        let text = MINIMAL.replace("Enter(character: \"a\"", "Enter(character: \"zed\"");
        let err = load_str(&text, None).unwrap_err().in_file("s.ron");
        let SceneError::Invalid { issues, .. } = &err else {
            panic!("expected validation errors, got {err:?}");
        };
        assert_eq!(issues.len(), 1, "{err}");
        assert_eq!(issues[0].line, Some(8), "{err}");
        assert!(err.to_string().starts_with("s.ron:8: "), "{err}");
        assert!(
            err.to_string().contains("unknown character \"zed\""),
            "{err}"
        );
    }

    #[test]
    fn io_error_names_the_file() {
        let err = load_file(Path::new("/definitely/not/here.ron"), None).unwrap_err();
        assert!(matches!(err, SceneError::Io { .. }));
        assert!(err.to_string().contains("here.ron"));
    }
}
