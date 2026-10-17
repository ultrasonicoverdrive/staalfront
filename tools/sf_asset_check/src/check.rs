//! The rules: one `.glb` against the art brief.
//!
//! Every finding has a rule id (the word in the report), a severity and a
//! message. **Fail** = the file breaks a rule of the brief that can be
//! measured; **warn** = a tolerance or a heuristic that a person should look
//! at. The thresholds are the constants below.

use std::path::Path;

use glam::Vec3;
use sf_data::units::UnitDefs;
use sf_render::glb::{AxisConvention, LoadedLevel, load_glb};

use crate::class::{
    Class, Classified, Faction, ModelName, TWO_FIGURE_ROLES, budget, classify, parse_file_name,
};
use crate::doc::{DocInfo, LodRoot, NodeInfo, inspect};
use crate::geom::LevelGeom;
use crate::raster::top_down;

/// Levels of detail the brief asks for: `_LOD0`, `_LOD1`, `_LOD2`.
pub const LODS: usize = 3;
/// At most this many materials per model.
pub const MAX_MATERIALS: usize = 6;
/// The `team` material covers this share of the top-down visible area of LOD0.
pub const TEAM_SHARE: (f64, f64) = (0.20, 0.40);
/// The lowest point may be this far (metres) from `Y = 0` without a remark.
pub const GROUND_OK: f32 = 0.05;
/// Beyond [`GROUND_OK`] up to here is a warning, past it a failure.
pub const GROUND_WARN: f32 = 0.30;
/// A LOD whose bounds differ from LOD0 by more than this share of LOD0's
/// longest side is a warning (the brief says "same size" but a simpler LOD
/// cannot be identical, so this is a tolerance for a person to judge).
pub const SIZE_WARN: f32 = 0.10;
/// The middle of the footprint may be this share of the longer side away from
/// the origin before it is a warning.
pub const ORIGIN_WARN: f32 = 0.40;
/// One cell of the playfield (B12); an ore field must fit in one.
pub const CELL_METRES: f32 = 4.0;

/// The nodes of a figure (a second figure adds `_2` to each).
pub const FIGURE_NODES: [&str; 6] = ["torso", "head", "arm_l", "arm_r", "leg_l", "leg_r"];

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warn,
    Fail,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    /// Short id of the rule: `budget`, `team-share`, ...
    pub rule: &'static str,
    pub message: String,
}

/// Overall verdict of one file.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

/// What was measured, for the one-line summary of a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub class: Option<Class>,
    pub pair: bool,
    pub vessel: bool,
    /// Triangles per LOD, as the engine loader sees them.
    pub triangles: Vec<u32>,
    pub budget: Option<[u32; 3]>,
    pub materials: usize,
    /// Team share of the top-down view per LOD (only for models with a team material).
    pub team_share: Vec<Option<f64>>,
    /// Bounding box of LOD0 in engine axes.
    pub bounds: Option<(Vec3, Vec3)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileReport {
    pub file: String,
    pub facts: Facts,
    pub findings: Vec<Finding>,
}

impl FileReport {
    fn new(file: &str) -> Self {
        Self {
            file: file.to_owned(),
            facts: Facts::default(),
            findings: Vec::new(),
        }
    }

    fn add(&mut self, severity: Severity, rule: &'static str, message: String) {
        self.findings.push(Finding {
            severity,
            rule,
            message,
        });
    }

    fn fail(&mut self, rule: &'static str, message: String) {
        self.add(Severity::Fail, rule, message);
    }

    fn warn(&mut self, rule: &'static str, message: String) {
        self.add(Severity::Warn, rule, message);
    }

    pub fn status(&self) -> Status {
        match self.findings.iter().map(|f| f.severity).max() {
            None => Status::Pass,
            Some(Severity::Warn) => Status::Warn,
            Some(Severity::Fail) => Status::Fail,
        }
    }

    /// Whether any finding has this rule id.
    pub fn has(&self, rule: &str) -> bool {
        self.findings.iter().any(|f| f.rule == rule)
    }

    /// Whether a failure (not a warning) has this rule id.
    pub fn failed(&self, rule: &str) -> bool {
        self.findings
            .iter()
            .any(|f| f.rule == rule && f.severity == Severity::Fail)
    }
}

/// Checks files; holds the unit definitions that give roles their class.
pub struct Checker {
    defs: UnitDefs,
}

impl Checker {
    pub fn new(defs: UnitDefs) -> Self {
        Self { defs }
    }

    /// With the `unit_defs.ron` compiled into `sf_data`.
    pub fn embedded() -> Self {
        Self::new(UnitDefs::embedded())
    }

    /// Read and check a file; an unreadable file is a failure of rule `io`.
    pub fn check_path(&self, path: &Path) -> FileReport {
        let file = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        match std::fs::read(path) {
            Ok(bytes) => self.check_bytes(&file, &bytes),
            Err(e) => {
                let mut report = FileReport::new(&file);
                report.fail("io", format!("cannot read {}: {e}", path.display()));
                report
            }
        }
    }

    /// Check the bytes of a file called `file_name`.
    pub fn check_bytes(&self, file_name: &str, bytes: &[u8]) -> FileReport {
        let mut run = Run {
            report: FileReport::new(file_name),
            name: None,
            classified: None,
        };
        run.go(&self.defs, file_name, bytes);
        run.report
    }
}

struct Run {
    report: FileReport,
    name: Option<ModelName>,
    classified: Option<Classified>,
}

impl Run {
    fn go(&mut self, defs: &UnitDefs, file_name: &str, bytes: &[u8]) {
        self.name_and_class(defs, file_name);
        let doc = match inspect(bytes) {
            Ok(doc) => doc,
            Err(e) => {
                self.report.fail("glb", e);
                return;
            }
        };
        let roots = self.lod_roots(&doc);
        if roots.iter().all(Option::is_some) {
            self.lod_structure(&roots);
        }
        self.materials(&doc);
        let model = match load_glb(bytes, &AxisConvention::ENGINE) {
            Ok(model) => Some(model),
            Err(e) => {
                self.report
                    .fail("engine-load", format!("the engine loader refuses it: {e}"));
                None
            }
        };
        let geoms: Vec<LevelGeom> = model
            .iter()
            .flat_map(|m| m.levels.iter().map(LevelGeom::from_level))
            .collect();
        if let Some(root0) = roots[0] {
            self.nodes(root0);
            if let Some(level0) = model.as_ref().and_then(|m| m.levels.first()) {
                self.engine_parts(root0, level0);
            }
        }
        if geoms.is_empty() {
            return;
        }
        self.report.facts.triangles = geoms.iter().map(|g| g.tris.len() as u32).collect();
        self.budgets(roots[0]);
        self.mesh_quality(&geoms);
        self.team(&doc, &geoms);
        self.bounds(&geoms);
    }

    fn name_and_class(&mut self, defs: &UnitDefs, file_name: &str) {
        match parse_file_name(file_name) {
            Ok(name) => self.name = Some(name),
            Err(e) => self.report.fail("file-name", e),
        }
        let Some(name) = &self.name else { return };
        match classify(&name.role, defs) {
            Ok(c) => {
                if c.faction_listed(name.faction) == Some(false) {
                    self.report.warn(
                        "faction",
                        format!(
                            "unit_defs.ron does not give the role {:?} to {}",
                            name.role,
                            name.faction.prefix()
                        ),
                    );
                }
                if matches!(c.class, Class::Decor | Class::Wreck)
                    && name.faction != Faction::Neutral
                    && c.unit_factions.is_none()
                {
                    self.report.warn(
                        "faction",
                        format!(
                            "{} is decor or a wreck; it belongs under neutral_",
                            name.stem
                        ),
                    );
                }
                self.report.facts.class = Some(c.class);
                self.report.facts.vessel = c.vessel;
                self.classified = Some(c);
            }
            Err(e) => self.report.fail("class", e),
        }
    }

    /// The three LOD roots, by level; reports the missing and the odd ones.
    fn lod_roots<'d>(&mut self, doc: &'d DocInfo) -> [Option<&'d LodRoot>; LODS] {
        let mut found: [Option<&LodRoot>; LODS] = [None; LODS];
        if doc.roots.is_empty() {
            self.report
                .fail("lod-roots", "the default scene has no root nodes".into());
        }
        for root in &doc.roots {
            match root.lod {
                None => self.report.fail(
                    "lod-roots",
                    format!("scene root {:?} has no _LOD<n> suffix", root.raw_name),
                ),
                Some(n) if n as usize >= LODS => self.report.warn(
                    "lod-roots",
                    format!("{:?}: the brief has LOD0 to LOD2 only", root.raw_name),
                ),
                Some(n) => {
                    if found[n as usize].is_some() {
                        self.report.fail(
                            "lod-roots",
                            format!("two scene roots are LOD{n} ({:?})", root.raw_name),
                        );
                    } else {
                        found[n as usize] = Some(root);
                    }
                }
            }
            if let Some(name) = &self.name
                && root.base != name.stem
            {
                self.report.fail(
                    "lod-roots",
                    format!(
                        "root {:?} should be named {}_LOD<n> after the file",
                        root.raw_name, name.stem
                    ),
                );
            }
        }
        let stem = self.name.as_ref().map_or("<id>", |n| n.stem.as_str());
        for (n, root) in found.iter().enumerate() {
            if root.is_none() && !doc.roots.is_empty() {
                self.report
                    .fail("lod-roots", format!("{stem}_LOD{n} is missing"));
            }
        }
        found
    }

    /// Identical node structure and root transform in every LOD.
    fn lod_structure(&mut self, roots: &[Option<&LodRoot>; LODS]) {
        let (Some(first), Some(_), Some(_)) = (roots[0], roots[1], roots[2]) else {
            return;
        };
        let base = first.structure();
        for (n, root) in roots.iter().enumerate().skip(1) {
            let Some(root) = root else { continue };
            let other = root.structure();
            if other != base {
                let missing: Vec<&String> = base.iter().filter(|p| !other.contains(p)).collect();
                let extra: Vec<&String> = other.iter().filter(|p| !base.contains(p)).collect();
                self.report.fail(
                    "lod-structure",
                    format!("LOD{n} nodes differ from LOD0: missing {missing:?}, extra {extra:?}"),
                );
            }
            if !same_transform(&first.transform, &root.transform) {
                self.report.fail(
                    "lod-origin",
                    format!(
                        "the root of LOD{n} has another origin (position, rotation or scale) than LOD0"
                    ),
                );
            }
        }
    }

    fn materials(&mut self, doc: &DocInfo) {
        let count = doc.materials.len() + usize::from(doc.unmaterialed_primitive);
        self.report.facts.materials = count;
        if count > MAX_MATERIALS {
            self.report.fail(
                "materials",
                format!("{count} materials, at most {MAX_MATERIALS}"),
            );
        }
        if doc.unmaterialed_primitive {
            self.report.warn(
                "materials",
                "a primitive has no material; the default material counts as one".into(),
            );
        }
        if doc.textures > 0 || doc.images > 0 {
            self.report.fail(
                "textures",
                format!(
                    "{} textures and {} images; materials are base colour only",
                    doc.textures, doc.images
                ),
            );
        }
        for m in doc.materials.iter().filter(|m| m.emissive) {
            self.report.warn(
                "materials",
                format!(
                    "material {:?} has an emissive factor; the engine takes base colour only",
                    m.name.as_deref().unwrap_or("?")
                ),
            );
        }
    }

    fn pair(&self, root0: Option<&LodRoot>) -> bool {
        let two_figure_role = self
            .name
            .as_ref()
            .is_some_and(|n| TWO_FIGURE_ROLES.contains(&n.role.as_str()));
        let has_second = root0.is_some_and(|r| {
            FIGURE_NODES
                .iter()
                .any(|n| r.tree.find(&format!("{n}_2")).is_some())
        });
        let infantry = self
            .classified
            .as_ref()
            .is_some_and(|c| c.class == Class::Infantry);
        infantry && (two_figure_role || has_second)
    }

    fn budgets(&mut self, root0: Option<&LodRoot>) {
        let tris = self.report.facts.triangles.clone();
        let Some(class) = self.classified.as_ref().map(|c| c.class) else {
            return;
        };
        let pair = self.pair(root0);
        let limits = budget(class, pair);
        self.report.facts.pair = pair;
        self.report.facts.budget = Some(limits);
        let what = if pair {
            "two-figure infantry".to_owned()
        } else {
            class.name().to_owned()
        };
        for (n, (&t, &limit)) in tris.iter().zip(&limits).enumerate() {
            if t > limit {
                self.report.fail(
                    "budget",
                    format!("LOD{n} has {t} triangles, the {what} budget is {limit}"),
                );
            }
        }
        for n in 1..tris.len() {
            if tris[n] > tris[n - 1] {
                self.report.warn(
                    "lod-triangles",
                    format!(
                        "LOD{n} has more triangles ({}) than LOD{} ({})",
                        tris[n],
                        n - 1,
                        tris[n - 1]
                    ),
                );
            }
        }
    }

    /// Triangles without area and vertices without a normal.
    fn mesh_quality(&mut self, geoms: &[LevelGeom]) {
        let list = |counts: Vec<usize>| {
            counts
                .iter()
                .enumerate()
                .filter(|&(_, &c)| c > 0)
                .map(|(n, c)| format!("LOD{n} {c}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let degenerate = list(geoms.iter().map(|g| g.degenerate).collect());
        if !degenerate.is_empty() {
            self.report.warn(
                "degenerate",
                format!("zero-area triangles ({degenerate}); they use up the budget"),
            );
        }
        let zero = list(geoms.iter().map(|g| g.zero_normals).collect());
        if !zero.is_empty() {
            self.report.warn(
                "normals",
                format!(
                    "vertices with a zero-length normal ({zero}); the lighting is undefined there"
                ),
            );
        }
    }

    /// The engine loader turns a node into a rigged part only if it has no
    /// mesh or has children; a `turret` or `barrel` that is a plain mesh node
    /// is folded into its parent and cannot turn or tilt on its own.
    fn engine_parts(&mut self, root0: &LodRoot, level0: &LoadedLevel) {
        let Some(class) = self.classified.as_ref().map(|c| c.class) else {
            return;
        };
        if !matches!(class, Class::Vehicle | Class::Wreck) {
            return;
        }
        for (name, motion) in [("turret", "turn"), ("barrel", "tilt")] {
            let Some((node, _)) = root0.tree.find(name) else {
                continue;
            };
            if node.has_mesh && !level0.parts.iter().any(|p| p.name == name) {
                let message = format!(
                    "{name} is a mesh node without children, so the engine loader merges it into \
                     its parent and it cannot {motion} on its own (give it an empty node with the \
                     mesh as a child)"
                );
                if class == Class::Vehicle {
                    self.report.fail("engine-parts", message);
                } else {
                    self.report.warn("engine-parts", message);
                }
            }
        }
    }

    fn team(&mut self, doc: &DocInfo, geoms: &[LevelGeom]) {
        let Some(name) = &self.name else { return };
        let has_team = doc.has_team_material();
        let needs = name.faction.needs_team();
        if !needs {
            if has_team {
                self.report
                    .fail("team", "a neutral object has a material named team".into());
            }
            return;
        }
        if !has_team {
            self.report.fail(
                "team",
                format!(
                    "a {} model needs a material named team",
                    name.faction.prefix()
                ),
            );
            return;
        }
        let shares: Vec<Option<f64>> = geoms
            .iter()
            .map(|g| top_down(&g.tris).team_share())
            .collect();
        self.report.facts.team_share.clone_from(&shares);
        let (lo, hi) = TEAM_SHARE;
        for (n, share) in shares.iter().enumerate() {
            let severity = if n == 0 {
                Severity::Fail
            } else {
                Severity::Warn
            };
            let message = match share {
                None => format!("LOD{n} shows nothing from above"),
                Some(s) if geoms[n].team_triangles() == 0 => {
                    format!("LOD{n} has no team triangles ({:.0} %)", s * 100.0)
                }
                Some(s) if *s < lo || *s > hi => format!(
                    "team is {:.1} % of the top-down view of LOD{n}, the brief asks {:.0} to {:.0} %",
                    s * 100.0,
                    lo * 100.0,
                    hi * 100.0
                ),
                Some(_) => continue,
            };
            let rule = if geoms[n].team_triangles() == 0 {
                "team"
            } else {
                "team-share"
            };
            self.report.add(severity, rule, message);
        }
    }

    fn bounds(&mut self, geoms: &[LevelGeom]) {
        let g0 = &geoms[0];
        self.report.facts.bounds = Some((g0.min, g0.max));
        let vessel = self.report.facts.vessel;
        for (n, g) in geoms.iter().enumerate() {
            if vessel {
                if !(g.min.y < 0.0 && g.max.y > 0.0) {
                    self.report.warn(
                        "ground",
                        format!(
                            "vessel LOD{n}: the waterline Y = 0 is not inside the hull (Y {:.2} to {:.2} m)",
                            g.min.y, g.max.y
                        ),
                    );
                }
                continue;
            }
            let off = g.min.y.abs();
            if off > GROUND_OK {
                let message = format!(
                    "LOD{n}: the lowest point is at Y = {:.2} m, the ground is Y = 0",
                    g.min.y
                );
                if off > GROUND_WARN {
                    self.report.fail("ground", message);
                } else {
                    self.report.warn("ground", message);
                }
            }
        }
        let ext = g0.extent();
        let class = self.classified.as_ref().map(|c| c.class);
        match class {
            Some(Class::Vehicle | Class::Wreck) if ext.x <= ext.z => self.report.fail(
                "orientation",
                format!(
                    "the front must be +X: LOD0 is {:.2} m along X but {:.2} m along Z",
                    ext.x, ext.z
                ),
            ),
            Some(Class::Infantry) if ext.y <= ext.x.max(ext.z) => self.report.fail(
                "orientation",
                format!(
                    "infantry must stand along +Y: LOD0 is {:.2} m high but {:.2} m wide",
                    ext.y,
                    ext.x.max(ext.z)
                ),
            ),
            _ => {}
        }
        let larger = ext.x.max(ext.z);
        let centre = (g0.min + g0.max) * 0.5;
        let offset = centre.x.hypot(centre.z);
        if larger > 0.0 && offset > ORIGIN_WARN * larger {
            self.report.warn(
                "origin",
                format!(
                    "the middle of LOD0 is {offset:.2} m from the origin (footprint {larger:.2} m); \
                     the origin belongs under the middle"
                ),
            );
        }
        let ore = self
            .name
            .as_ref()
            .is_some_and(|n| n.role.starts_with("ore_field"));
        if ore && larger > CELL_METRES + GROUND_OK {
            self.report.warn(
                "footprint",
                format!("an ore field is one {CELL_METRES} m cell; LOD0 is {larger:.2} m wide"),
            );
        }
        let longest = ext.max_element();
        if longest > 0.0 {
            for (n, g) in geoms.iter().enumerate().skip(1) {
                let drift = (g.min - g0.min)
                    .abs()
                    .max((g.max - g0.max).abs())
                    .max_element();
                let share = drift / longest;
                let message = format!(
                    "LOD{n} bounds differ from LOD0 by {drift:.2} m ({:.0} % of the longest side)",
                    share * 100.0
                );
                if share > SIZE_WARN {
                    self.report.warn("lod-size", message);
                }
            }
        }
    }

    /// Required node names for the class, on LOD0.
    fn nodes(&mut self, root0: &LodRoot) {
        let Some(class) = self.classified.as_ref().map(|c| c.class) else {
            return;
        };
        let tree = &root0.tree;
        match class {
            Class::Infantry => {
                let missing: Vec<&str> = FIGURE_NODES
                    .iter()
                    .copied()
                    .filter(|n| tree.find(n).is_none())
                    .collect();
                if !missing.is_empty() {
                    self.report.fail(
                        "nodes",
                        format!("infantry nodes missing: {}", missing.join(", ")),
                    );
                }
                if self.pair(Some(root0)) {
                    let missing: Vec<String> = FIGURE_NODES
                        .iter()
                        .map(|n| format!("{n}_2"))
                        .filter(|n| tree.find(n).is_none())
                        .collect();
                    if !missing.is_empty() {
                        self.report.fail(
                            "nodes",
                            format!(
                                "two-figure unit: nodes of the second figure missing: {}",
                                missing.join(", ")
                            ),
                        );
                    }
                }
            }
            Class::Vehicle | Class::Wreck => {
                self.vehicle_nodes(tree, class);
                if class == Class::Wreck {
                    self.fx_nodes(tree);
                }
            }
            Class::Building | Class::Decor => {}
        }
    }

    fn vehicle_nodes(&mut self, tree: &NodeInfo, class: Class) {
        let hull = tree.find("hull");
        if hull.is_none() {
            self.report.fail("nodes", "no node called hull".into());
        }
        let turret = tree.find("turret");
        let barrel = tree.find("barrel");
        if let Some((_, parent)) = turret
            && parent != Some("hull")
        {
            self.report.fail(
                "nodes",
                format!(
                    "turret hangs under {}, the brief has hull -> turret -> barrel",
                    parent.map_or("the root".to_owned(), |p| p.to_owned())
                ),
            );
        }
        if let Some((_, parent)) = barrel
            && parent != Some("turret")
        {
            self.report.fail(
                "nodes",
                format!(
                    "barrel hangs under {}, the brief has hull -> turret -> barrel",
                    parent.map_or("the root".to_owned(), |p| p.to_owned())
                ),
            );
        }
        // A wreck may have lost its turret; a vehicle keeps (empty) nodes for it.
        if class == Class::Vehicle && (turret.is_none() || barrel.is_none()) {
            self.report.warn(
                "nodes",
                "a vehicle keeps turret and barrel nodes, empty if it has none".into(),
            );
        }
    }

    fn fx_nodes(&mut self, tree: &NodeInfo) {
        let all = tree.descendants();
        for prefix in ["fx_smoke_", "fx_fire_"] {
            let nodes: Vec<&&NodeInfo> =
                all.iter().filter(|n| n.name.starts_with(prefix)).collect();
            if nodes.is_empty() {
                self.report
                    .fail("fx-nodes", format!("a wreck needs an empty {prefix}* node"));
            }
            for n in nodes.iter().filter(|n| n.has_mesh) {
                self.report.fail(
                    "fx-nodes",
                    format!("{} must be an empty node but has a mesh", n.name),
                );
            }
        }
    }
}

fn same_transform(a: &([f32; 3], [f32; 4], [f32; 3]), b: &([f32; 3], [f32; 4], [f32; 3])) -> bool {
    const EPS: f32 = 1e-4;
    let close = |x: &[f32], y: &[f32]| x.iter().zip(y).all(|(p, q)| (p - q).abs() <= EPS);
    // q and -q are the same rotation.
    let dot: f32 = a.1.iter().zip(&b.1).map(|(p, q)| p * q).sum();
    close(&a.0, &b.0) && close(&a.2, &b.2) && dot.abs() >= 1.0 - EPS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_transforms_match_and_others_do_not() {
        let t = ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        assert!(same_transform(&t, &t));
        let flipped = (t.0, [0.0, 0.0, 0.0, -1.0], t.2);
        assert!(same_transform(&t, &flipped), "q and -q are one rotation");
        let moved = ([0.0, 0.1, 0.0], t.1, t.2);
        assert!(!same_transform(&t, &moved));
        let scaled = (t.0, t.1, [2.0, 1.0, 1.0]);
        assert!(!same_transform(&t, &scaled));
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let turned = (t.0, [0.0, h, 0.0, h], t.2);
        assert!(!same_transform(&t, &turned));
    }

    #[test]
    fn status_is_the_worst_finding() {
        let mut r = FileReport::new("x.glb");
        assert_eq!(r.status(), Status::Pass);
        r.warn("origin", "w".into());
        assert_eq!(r.status(), Status::Warn);
        r.fail("budget", "f".into());
        assert_eq!(r.status(), Status::Fail);
        assert!(r.has("origin") && r.failed("budget") && !r.failed("origin"));
    }
}
