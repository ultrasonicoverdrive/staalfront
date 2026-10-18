//! What every role of the unit definitions looks like (WP3.2).
//!
//! The simulation knows a unit kind only as a permanent number. This module is
//! the data mapping from that number to a *shape* of the instancer, built from
//! the definitions in `assets/config/unit_defs.ron` (`sf_data::units`): the
//! `model` field says which placeholder primitive or which glTF file the role
//! is drawn with, `icon` which picture of the icon set it gets at strategic zoom
//! (WP3.6; the base shape `strategic_icon` when it has none or the set does not
//! have it), and the role's `size` and `footprint` how big it is. It replaces the stand-in
//! `kind % shapes` of the first demo (follow-up 12 of `docs/progress.md`).
//!
//! # Where the shapes come from
//!
//! The instancer draws a unit as one *shape* of its style: a placeholder cube,
//! cylinder or cone, or a glTF model. A role gets
//!
//! * for `Model::Glb(path)`: the glTF file, loaded with the axes and switch
//!   sizes of the model pack in `assets/config/models.ron` whose directory the
//!   file lies in (`sf_render::glb::load_pack_model`). Roles that draw the same
//!   file with different icons share the loaded model: one shape each, one chain
//!   and one set of meshes (`ModelSet::with_chains`). A structure's model is scaled, the same in every direction, until it
//!   just fits its footprint of 4 m cells. A role whose file cannot be read, and every role when models are
//!   turned off (`--models none`), takes the fallback below;
//! * for `Model::Shape(s)`: that placeholder primitive;
//! * as a fallback, a primitive chosen by what the role is: infantry a
//!   cylinder, a vehicle or vessel a cone, a structure or an object a cube.
//!
//! A placeholder primitive is drawn **at the size of the role**, not at the
//! one-size shapes of `units.ron`: a structure is `4 m x footprint` on each
//! side (B12: a cell is 4 m), a unit or object as wide as its `size` in cells
//! times 4 m. Each distinct size and icon is one more shape of the style
//! ([`Style::with_variants`]) that keeps the level-of-detail chain of the
//! primitive it comes from, so a cone far away still gets fewer facets. A
//! vehicle's glTF model keeps the size it was authored in.
//!
//! The shape numbers are: the style's own placeholder shapes first, then the
//! size variants in order of the role numbers, then the glTF models in the same
//! order. Roles are visited by ascending kind number, so the numbers do not
//! depend on the order of the file.
//!
//! Entities the simulation did not make from the definitions (the placeholder
//! crowd of `--units`) have no role; they keep the first demo's mapping
//! ([`crate::population`]).

use std::collections::BTreeMap;
use std::path::Path;

use sf_data::units::{Category, IconShape as DataIcon, Model, ModelShape, UnitDefs};
use sf_math::render_bridge::{METRES_PER_CELL, cells_to_metres};
use sf_render::glb::{LoadedPack, MODELS_CONFIG, load_pack_model};
use sf_render::scene::MeshKind;
use sf_render::style::Shape;
use sf_render::{IconAtlas, IconId, IconShape, ModelSet, Style};
use sf_sim::Kind;

/// Height of a placeholder structure in metres: this plus one metre per cell
/// of its shorter side (so a 1 x 1 tower is 4 m, a 4 x 4 centre 7 m).
const STRUCTURE_BASE_HEIGHT: f32 = 3.0;

/// A glTF model as the roles ask for it: the file and the footprint it is
/// fitted to (a structure's model), each combination loaded once. Roles with
/// different icons that draw the same model share its meshes: one more shape
/// of the style, the same chain (so no more draw calls and no more GPU memory).
type ModelKey = (String, Option<(u8, u8)>);

/// What a role is drawn with, before the shape numbers are fixed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    /// A placeholder primitive of this base shape, icon and size (in
    /// millimetres, so the key orders and compares exactly).
    Placeholder {
        base: u8,
        icon: IconId,
        size_mm: [i32; 3],
    },
    /// The glTF model with this icon: an index into the roster's model
    /// entries, each a loaded model and an icon.
    Model(usize),
}

/// One role: what the game calls it and how it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleLook {
    /// The role's id in the definitions (`light_tank`).
    pub id: String,
    /// Index into the style's shapes.
    pub shape: u8,
    /// True if the role is drawn with a glTF model.
    pub model: bool,
}

/// The look of every role of a set of definitions, ready to be put on a
/// [`Style`] and a model set. See the module documentation.
pub struct Roster {
    looks: BTreeMap<u16, RoleLook>,
    /// The size variants appended after the style's own shapes, with the shape
    /// whose level-of-detail chain each keeps.
    variants: Vec<(u8, Shape)>,
    /// The glTF models of the roles, after the variants.
    pack: Option<LoadedPack>,
    /// Roles whose model could not be used and fell back to a primitive.
    pub warnings: Vec<String>,
}

fn icon_of(icon: DataIcon) -> IconShape {
    match icon {
        DataIcon::Square => IconShape::Square,
        DataIcon::Circle => IconShape::Circle,
        DataIcon::Triangle => IconShape::Triangle,
        DataIcon::HalfCircle => IconShape::HalfCircle,
    }
}

fn mesh_of(shape: ModelShape) -> MeshKind {
    match shape {
        ModelShape::Cube => MeshKind::Cube,
        ModelShape::Cylinder => MeshKind::Cylinder,
        ModelShape::Cone => MeshKind::Cone,
    }
}

/// The primitive a role gets when it has no model to draw (yet): chosen by
/// what the role is.
pub fn fallback_shape(category: Category) -> ModelShape {
    match category {
        Category::Infantry => ModelShape::Cylinder,
        Category::Vehicle | Category::Vessel => ModelShape::Cone,
        Category::Structure | Category::Object => ModelShape::Cube,
    }
}

/// Millimetres of a size in metres, for exact keys.
fn mm(metres: f32) -> i32 {
    (metres * 1000.0).round() as i32
}

impl Roster {
    /// The looks of all roles of `defs` on `base`, the style of the
    /// placeholder shapes (cube, cylinder and cone at vehicle size).
    /// `assets` is the assets directory the model files lie under;
    /// `models_ron` the text of `assets/config/models.ron`. With `use_models`
    /// false no file is read and every role is a primitive. `icons` is the
    /// loaded icon set: a role's `icon` is looked up in it, and a role without
    /// one (or whose name the set does not have, which is a warning) gets the
    /// plain base shape of its `strategic_icon`.
    ///
    /// Fails only if the roles need more than 255 shapes in all.
    pub fn build(
        defs: &UnitDefs,
        base: &Style,
        icons: &IconAtlas,
        assets: &Path,
        models_ron: &str,
        use_models: bool,
    ) -> Result<Roster, String> {
        let mut warnings = Vec::new();
        // glTF models in the order they are first needed, each file (at a
        // footprint) once; and the (model, icon) pairs the roles draw, the
        // model *entries*, which become shapes.
        let mut models: Vec<LoadedPack> = Vec::new();
        let mut model_index: BTreeMap<ModelKey, Option<usize>> = BTreeMap::new();
        let mut entries: Vec<(usize, IconId)> = Vec::new();
        let mut sources: BTreeMap<u16, Source> = BTreeMap::new();

        for def in defs.units() {
            let sim = defs
                .sim_unit(def.kind)
                .ok_or_else(|| format!("{}: no converted role", def.id))?;
            let shape = icon_of(def.strategic_icon);
            let icon = icons.resolve(def.icon.as_deref(), shape);
            if let Some(name) = &def.icon
                && icons.lookup(name).is_none()
            {
                warnings.push(format!(
                    "{}: icon \"{name}\" is not in the icon set; drawn as its base shape",
                    def.id
                ));
            }
            let mut mesh = None;
            match &def.model {
                Model::Shape(shape) => mesh = Some(mesh_of(*shape)),
                Model::Glb(path) if use_models => {
                    // A structure's model is fitted to its footprint, so the
                    // same file under two footprints is two models.
                    let fit = sim.footprint.map(|(w, h)| {
                        (
                            f32::from(w) * METRES_PER_CELL,
                            f32::from(h) * METRES_PER_CELL,
                        )
                    });
                    let key = (path.clone(), sim.footprint);
                    let found = *model_index.entry(key).or_insert_with(|| {
                        match load_pack_model(models_ron, path, shape.into(), assets, fit) {
                            Ok(one) => {
                                models.push(one);
                                Some(models.len() - 1)
                            }
                            Err(e) => {
                                warnings.push(format!("{}: {e}; drawn as a primitive", def.id));
                                None
                            }
                        }
                    });
                    match found {
                        Some(i) => {
                            let entry = entries
                                .iter()
                                .position(|e| *e == (i, icon))
                                .unwrap_or_else(|| {
                                    entries.push((i, icon));
                                    entries.len() - 1
                                });
                            sources.insert(def.kind, Source::Model(entry));
                        }
                        None => mesh = Some(mesh_of(fallback_shape(def.category))),
                    }
                }
                Model::Glb(_) => mesh = Some(mesh_of(fallback_shape(def.category))),
            }
            if let Some(mesh) = mesh {
                sources.insert(
                    def.kind,
                    placeholder(sim.size, sim.footprint, mesh, icon, base),
                );
            }
        }

        // Fix the numbers: the style's shapes, the variants, the models.
        let mut variants: Vec<(u8, Shape)> = Vec::new();
        let mut variant_at: BTreeMap<Source, usize> = BTreeMap::new();
        let mut kinds_by_source: Vec<(u16, Source)> = sources.into_iter().collect();
        kinds_by_source.sort_by_key(|(kind, _)| *kind);
        for (_, source) in &kinds_by_source {
            if let Source::Placeholder {
                base: chain,
                icon,
                size_mm,
            } = source
                && !variant_at.contains_key(source)
            {
                variant_at.insert(source.clone(), variants.len());
                let mesh = base.shape(*chain).mesh;
                variants.push((
                    *chain,
                    Shape {
                        mesh,
                        size: size_mm.map(|v| v as f32 / 1000.0),
                        icon: *icon,
                    },
                ));
            }
        }
        let base_count = base.shape_count();
        let model_base = base_count + variants.len();
        if model_base + entries.len() > 255 {
            return Err(format!(
                "the roles need {} shapes, the instancer takes 255",
                model_base + entries.len()
            ));
        }
        let mut looks = BTreeMap::new();
        for (kind, source) in kinds_by_source {
            let def = defs
                .unit_by_kind(kind)
                .ok_or_else(|| format!("kind {kind}: no definition"))?;
            let (shape, model) = match &source {
                Source::Placeholder { .. } => (base_count + variant_at[&source], false),
                Source::Model(i) => (model_base + i, true),
            };
            looks.insert(
                kind,
                RoleLook {
                    id: def.id.clone(),
                    shape: shape as u8,
                    model,
                },
            );
        }

        // The models as one pack: the loaded models in order, then one shape per
        // entry: its model's chain (the meshes are shared) and its icon.
        let pack = models
            .into_iter()
            .reduce(|mut all, one| {
                all.set = all
                    .set
                    .concat(&one.set)
                    .expect("a few models fit in a mesh table");
                all.models.extend(one.models);
                all
            })
            .map(|mut all| {
                let chains: Vec<usize> = entries.iter().map(|e| e.0).collect();
                all.set = all
                    .set
                    .with_chains(&chains)
                    .expect("every entry is a loaded model");
                all.icons = entries.iter().map(|e| e.1).collect();
                all
            });
        Ok(Roster {
            looks,
            variants,
            pack,
            warnings,
        })
    }

    /// The look of the role with this kind number.
    pub fn look(&self, kind: Kind) -> Option<&RoleLook> {
        self.looks.get(&kind.0)
    }

    /// The shape an entity of this kind is drawn as; `None` for a number that
    /// is no role.
    pub fn shape_of(&self, kind: Kind) -> Option<u8> {
        self.look(kind).map(|l| l.shape)
    }

    /// The id of the role (`light_tank`).
    pub fn name_of(&self, kind: Kind) -> Option<&str> {
        self.look(kind).map(|l| l.id.as_str())
    }

    /// Every role with its look, ascending by kind.
    pub fn looks(&self) -> impl Iterator<Item = (Kind, &RoleLook)> {
        self.looks.iter().map(|(&k, l)| (Kind(k), l))
    }

    /// `base` followed by the size variants: the style to put the glTF models
    /// after (`crate::runner::apply_pack`).
    pub fn style(&self, base: &Style) -> Style {
        base.with_variants(&self.variants)
            .expect("the shape count was checked when the roster was built")
    }

    /// The glTF models of the roles, drawn after the variants. `None` if no
    /// role uses a model.
    pub fn pack(&self) -> Option<&LoadedPack> {
        self.pack.as_ref()
    }

    /// The model set for [`Roster::style`] and the models: the placeholder
    /// chains of the style, then the glTF models.
    pub fn model_set(&self, style: &Style) -> Option<ModelSet> {
        let set = ModelSet::from_style(style);
        match &self.pack {
            Some(pack) => set.concat(&pack.set),
            None => Some(set),
        }
    }
}

/// A role drawn as a primitive of `mesh`, at the size of the role.
fn placeholder(
    size: sf_math::Fx,
    footprint: Option<(u8, u8)>,
    mesh: MeshKind,
    icon: IconId,
    base: &Style,
) -> Source {
    let chain = base
        .shapes()
        .iter()
        .position(|s| s.mesh == mesh)
        .unwrap_or(0);
    let own = base.shape(chain as u8).size;
    let size = match footprint {
        // A structure: 4 m per cell on each side.
        Some((w, h)) => [
            f32::from(w) * METRES_PER_CELL,
            STRUCTURE_BASE_HEIGHT + f32::from(w.min(h)),
            f32::from(h) * METRES_PER_CELL,
        ],
        // A unit or an object: as wide as it is, in the proportions of the
        // primitive. A role of size zero keeps the primitive's own size.
        None => {
            let wide = cells_to_metres(size);
            if wide > 0.0 && own[0] > 0.0 {
                own.map(|v| v * wide / own[0])
            } else {
                own
            }
        }
    };
    Source::Placeholder {
        base: chain as u8,
        icon,
        size_mm: size.map(mm),
    }
}

/// The path of `models.ron` under the assets root, for callers.
pub const MODELS_FILE: &str = MODELS_CONFIG;

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    fn base() -> Style {
        Style::embedded().scaled(3.0)
    }

    fn models_ron() -> String {
        std::fs::read_to_string(assets().join(MODELS_CONFIG)).unwrap()
    }

    /// The whole style the instancer gets: the placeholders, the size
    /// variants and the models.
    fn full_style(roster: &Roster) -> Style {
        let style = roster.style(&base());
        match roster.pack() {
            Some(pack) => pack.style_after(&style).unwrap(),
            None => style,
        }
    }

    fn icons() -> IconAtlas {
        let atlas = IconAtlas::load(&assets().join("icons"));
        assert!(atlas.warnings().is_empty(), "{:?}", atlas.warnings());
        atlas
    }

    fn built(use_models: bool) -> (UnitDefs, Roster) {
        let defs = UnitDefs::embedded();
        let roster = Roster::build(
            &defs,
            &base(),
            &icons(),
            &assets(),
            &models_ron(),
            use_models,
        )
        .unwrap();
        (defs, roster)
    }

    #[test]
    fn every_role_has_a_look_inside_the_style() {
        let (defs, roster) = built(true);
        let style = full_style(&roster);
        assert_eq!(roster.looks().count(), defs.units().len());
        assert!(roster.warnings.is_empty(), "{:?}", roster.warnings);
        for def in defs.units() {
            let look = roster.look(Kind(def.kind)).expect(&def.id);
            assert_eq!(look.id, def.id);
            assert!(usize::from(look.shape) < style.shape_count(), "{}", def.id);
        }
        assert_eq!(roster.shape_of(Kind(999)), None);
        assert_eq!(roster.name_of(Kind(999)), None);
        assert_eq!(roster.name_of(Kind(19)), Some("light_tank"));
        // The model set has a chain for every shape of the style.
        let set = roster.model_set(&roster.style(&base())).unwrap();
        assert_eq!(set.models().len(), style.shape_count());
    }

    #[test]
    fn the_roles_with_a_glb_use_it_and_the_rest_are_primitives() {
        let (defs, roster) = built(true);
        for def in defs.units() {
            let model = matches!(def.model, Model::Glb(_));
            assert_eq!(
                roster.look(Kind(def.kind)).unwrap().model,
                model,
                "{}",
                def.id
            );
        }
        // Two roles with the same file and icon are one shape (the decoy has
        // the tower's icon on purpose); with different icons they are two
        // shapes that share one model: the same chain, so the same meshes.
        let shape = |id: &str| roster.look(Kind(defs.kind_of(id).unwrap())).unwrap().shape;
        assert_eq!(shape("defence_tower"), shape("decoy_emplacement"));
        assert_ne!(shape("repair_truck"), shape("transport_truck"));
        let set = roster.model_set(&roster.style(&base())).unwrap();
        let meshes = |id: &str| {
            set.model(shape(id))
                .meshes()
                .map(|m| m.index())
                .collect::<Vec<_>>()
        };
        assert_eq!(meshes("repair_truck"), meshes("transport_truck"));
        assert!(!meshes("repair_truck").is_empty());
        // More shapes than loaded models: the icons make shapes, the files
        // are loaded (and their meshes uploaded) once.
        let pack = roster.pack().unwrap();
        assert!(pack.models.len() < pack.set.models().len());
        assert_eq!(pack.icons.len(), pack.set.models().len());
        // Primitives of one size and icon are one shape; with another icon
        // they are another shape on the same primitive.
        let style = full_style(&roster);
        assert_ne!(shape("rifleman"), shape("engineer"));
        assert_eq!(
            style.shape(shape("rifleman")).mesh,
            style.shape(shape("engineer")).mesh
        );
        // The models come after the variants.
        let first_model = roster
            .looks()
            .filter(|(_, l)| l.model)
            .map(|(_, l)| l.shape)
            .min()
            .unwrap();
        let last_primitive = roster
            .looks()
            .filter(|(_, l)| !l.model)
            .map(|(_, l)| l.shape)
            .max()
            .unwrap();
        assert!(last_primitive < first_model);
    }

    #[test]
    fn without_models_every_role_is_a_primitive() {
        let (defs, roster) = built(false);
        assert!(roster.pack().is_none());
        assert!(roster.looks().all(|(_, l)| !l.model));
        let style = full_style(&roster);
        assert_eq!(
            style.shape_count(),
            base().shape_count() + roster.variants.len()
        );
        // A glb role takes the primitive its sort of thing gets: a vehicle a
        // cone, a structure a cube, infantry a cylinder.
        let mesh = |id: &str| {
            let kind = defs.kind_of(id).unwrap();
            style.shape(roster.shape_of(Kind(kind)).unwrap()).mesh
        };
        assert_eq!(mesh("light_tank"), MeshKind::Cone);
        assert_eq!(mesh("power_plant"), MeshKind::Cube);
        assert_eq!(
            mesh("scout"),
            MeshKind::Cylinder,
            "infantry with a glb file"
        );
        // A role that names a primitive keeps it.
        assert_eq!(mesh("rifleman"), MeshKind::Cone);
        assert_eq!(mesh("fuel_depot"), MeshKind::Cylinder);
        assert_eq!(mesh("gunboat"), MeshKind::Cylinder);
    }

    #[test]
    fn a_missing_model_file_falls_back_to_a_primitive_and_says_so() {
        let empty = std::env::temp_dir().join(format!("sf_roster_empty_{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let defs = UnitDefs::embedded();
        let roster = Roster::build(&defs, &base(), &icons(), &empty, &models_ron(), true);
        std::fs::remove_dir_all(&empty).ok();
        let roster = roster.unwrap();
        assert!(roster.pack().is_none());
        assert!(roster.looks().all(|(_, l)| !l.model));
        // One warning per file and footprint, naming the first role that wanted it.
        let glb_files = defs
            .units()
            .iter()
            .filter(|d| matches!(d.model, Model::Glb(_)))
            .map(|d| {
                (
                    format!("{:?}", d.model),
                    d.footprint.map(|f| (f.width, f.height)),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            roster.warnings.len(),
            glb_files.len(),
            "{:?}",
            roster.warnings
        );
        assert!(
            roster
                .warnings
                .iter()
                .all(|w| w.contains("drawn as a primitive"))
        );
    }

    #[test]
    fn a_primitive_structure_is_drawn_at_its_footprint() {
        let (defs, roster) = built(true);
        let style = full_style(&roster);
        let mut seen = 0;
        for def in defs.units() {
            let (Some(fp), Model::Shape(_)) = (def.footprint, &def.model) else {
                continue;
            };
            let shape = style.shape(roster.shape_of(Kind(def.kind)).unwrap());
            let cell = METRES_PER_CELL;
            assert_eq!(shape.size[0], f32::from(fp.width) * cell, "{}", def.id);
            assert_eq!(shape.size[2], f32::from(fp.height) * cell, "{}", def.id);
            seen += 1;
        }
        assert!(seen >= 10, "most structures are still primitives: {seen}");
    }

    #[test]
    fn a_glb_structure_is_scaled_to_fit_its_footprint() {
        let (defs, roster) = built(true);
        let style = full_style(&roster);
        let mut seen = 0;
        for def in defs.units() {
            let (Some(fp), Model::Glb(_)) = (def.footprint, &def.model) else {
                continue;
            };
            let shape = style.shape(roster.shape_of(Kind(def.kind)).unwrap());
            let (w, h) = (
                f32::from(fp.width) * METRES_PER_CELL,
                f32::from(fp.height) * METRES_PER_CELL,
            );
            assert!(
                shape.size[0] <= w + 0.01 && shape.size[2] <= h + 0.01,
                "{}",
                def.id
            );
            // It fills one side of the footprint (the uniform scale is set by
            // the tighter one).
            assert!(
                (shape.size[0] - w).abs() < 0.01 || (shape.size[2] - h).abs() < 0.01,
                "{}: {:?} in {w} x {h}",
                def.id,
                shape.size
            );
            seen += 1;
        }
        assert!(seen >= 4, "{seen} glb structures");
    }

    #[test]
    fn a_primitive_unit_is_as_wide_as_its_size() {
        let (defs, roster) = built(true);
        let style = full_style(&roster);
        for id in [
            "rifleman",
            "heavy_gun_team",
            "gunboat",
            "evidence_crate",
            "ferry",
        ] {
            let def = defs.unit(id).unwrap();
            let sim = defs.sim_unit(def.kind).unwrap();
            let shape = style.shape(roster.shape_of(Kind(def.kind)).unwrap());
            let wide = cells_to_metres(sim.size);
            assert!(
                (shape.size[0] - wide).abs() < 0.002,
                "{id}: {} vs {wide}",
                shape.size[0]
            );
            assert!((shape.size[2] - wide).abs() < 0.002, "{id}");
        }
    }

    #[test]
    fn every_role_gets_the_icon_of_its_definition() {
        let atlas = icons();
        for use_models in [true, false] {
            let (defs, roster) = built(use_models);
            let style = full_style(&roster);
            for def in defs.units() {
                let shape = style.shape(roster.shape_of(Kind(def.kind)).unwrap());
                // The picture the name gives, else the base shape.
                let want = atlas.resolve(def.icon.as_deref(), icon_of(def.strategic_icon));
                assert_eq!(shape.icon, want, "{}", def.id);
                match &def.icon {
                    // A role with an icon of its own is not a plain shape...
                    Some(name) => {
                        assert!(atlas.lookup(name).is_some(), "{}: {name}", def.id);
                        if name != "civilian" {
                            assert!(shape.icon.0 >= 4, "{}: {name}", def.id);
                        }
                    }
                    // ...and one without gets the plain shape of its category.
                    None => assert_eq!(
                        shape.icon,
                        IconId::from(icon_of(DataIcon::of_category(def.category))),
                        "{}",
                        def.id
                    ),
                }
            }
        }
    }

    /// Every role of the art brief has a picture of its own in the shipped
    /// set, and the four roles that are not in it are drawn as base shapes.
    #[test]
    fn the_shipped_set_covers_every_role_of_the_art_brief() {
        let defs = UnitDefs::embedded();
        let atlas = icons();
        let without: Vec<&str> = defs
            .units()
            .iter()
            .filter(|d| d.icon.is_none())
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(
            without,
            ["evidence_crate", "bridge_segment", "rail_spur", "ore_field"]
        );
        assert_eq!(defs.units().len() - without.len(), 39);
        for d in defs.units() {
            if let Some(name) = &d.icon {
                assert!(atlas.lookup(name).is_some(), "{}: no icon {name}", d.id);
            }
        }
    }

    /// An icon name the set does not have is a warning, and the role is drawn
    /// as the base shape of its category, whatever else is wrong.
    #[test]
    fn an_unknown_icon_falls_back_to_the_base_shape_and_says_so() {
        let defs = UnitDefs::embedded();
        let atlas = IconAtlas::base();
        // The plain base atlas has no role pictures at all: every named icon
        // is missing, so each role is its base shape and each has a warning.
        let roster = Roster::build(&defs, &base(), &atlas, &assets(), "", false).unwrap();
        let style = full_style(&roster);
        let named = defs.units().iter().filter(|d| d.icon.is_some()).count();
        assert_eq!(roster.warnings.len(), named, "{:?}", roster.warnings);
        assert!(roster.warnings[0].contains("is not in the icon set"));
        for def in defs.units() {
            let shape = style.shape(roster.shape_of(Kind(def.kind)).unwrap());
            assert_eq!(
                shape.icon,
                IconId::from(icon_of(def.strategic_icon)),
                "{}",
                def.id
            );
            assert!(shape.icon.0 < 4, "{}", def.id);
        }
        // The vessel falls back to the half-circle.
        let gunboat = defs.kind_of("gunboat").unwrap();
        assert_eq!(
            style.shape(roster.shape_of(Kind(gunboat)).unwrap()).icon,
            IconId::from(IconShape::HalfCircle)
        );
    }

    #[test]
    fn a_variant_keeps_the_level_of_detail_chain_of_its_primitive() {
        let (_, roster) = built(false);
        let style = full_style(&roster);
        let set = ModelSet::from_style(&style);
        let cone = base()
            .shapes()
            .iter()
            .position(|s| s.mesh == MeshKind::Cone)
            .unwrap();
        let levels = |i: usize| set.models()[i].levels().len();
        assert!(levels(cone) > 1, "the cone has a chain");
        for (i, shape) in style.shapes().iter().enumerate() {
            if shape.mesh == MeshKind::Cone {
                assert_eq!(levels(i), levels(cone), "shape {i}");
            }
        }
    }

    #[test]
    fn the_numbers_do_not_depend_on_the_order_of_the_file() {
        // Built twice from the same definitions: the same numbers.
        let (_, a) = built(true);
        let (_, b) = built(true);
        let numbers = |r: &Roster| r.looks().map(|(k, l)| (k.0, l.shape)).collect::<Vec<_>>();
        assert_eq!(numbers(&a), numbers(&b));
    }

    #[test]
    fn what_a_role_falls_back_to_follows_its_kind_of_thing() {
        assert_eq!(fallback_shape(Category::Infantry), ModelShape::Cylinder);
        assert_eq!(fallback_shape(Category::Vehicle), ModelShape::Cone);
        assert_eq!(fallback_shape(Category::Vessel), ModelShape::Cone);
        assert_eq!(fallback_shape(Category::Structure), ModelShape::Cube);
        assert_eq!(fallback_shape(Category::Object), ModelShape::Cube);
    }
}
