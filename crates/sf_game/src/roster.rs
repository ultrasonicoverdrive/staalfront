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
//! # One role, a model per faction
//!
//! A role has one generic `model` and may have a `models` entry per faction
//! (set v4: `meridian_rifleman.glb` and `veyran_rifleman.glb` are one role).
//! Which one a unit is drawn with follows its **owner**: [`faction_of_owner`]
//! maps the owner number to a faction (the sandbox's seats), and
//! [`Roster::look_for`] picks the faction's entry if the role has one that
//! loaded, else the generic model, else (the generic model is a glTF file that
//! does not load, or models are off) the primitive of what the role is. This
//! is presentation only: the simulation never sees a model, so no checksum
//! depends on it.
//!
//! The shape numbers are: the style's own placeholder shapes first, then the
//! size variants in order of the role numbers (and, within a role, the generic
//! look before the factions'), then the glTF models in the same order. Roles
//! are visited by ascending kind number, so the numbers do not depend on the
//! order of the file.
//!
//! Entities the simulation did not make from the definitions (the placeholder
//! crowd of `--units`) have no role; they keep the first demo's mapping
//! ([`crate::population`]).

use std::collections::BTreeMap;
use std::path::Path;

use sf_data::units::{
    Category, Faction, IconShape as DataIcon, Model, ModelShape, UnitDef, UnitDefs,
};
use sf_math::render_bridge::{METRES_PER_CELL, cells_to_metres};
use sf_render::glb::{LoadedPack, MODELS_CONFIG, load_pack_model};
use sf_render::scene::MeshKind;
use sf_render::style::Shape;
use sf_render::{IconAtlas, IconId, IconShape, ModelSet, Style};
use sf_sim::Kind;

use crate::sandbox;

/// Height of a placeholder structure in metres: this plus one metre per cell
/// of its shorter side (so a 1 x 1 tower is 4 m, a 4 x 4 centre 7 m).
const STRUCTURE_BASE_HEIGHT: f32 = 3.0;

/// A glTF model as the roles ask for it: the file and the footprint it is
/// fitted to (a structure's model), each combination loaded once. Roles with
/// different icons that draw the same model share its meshes: one more shape
/// of the style, the same chain (so no more draw calls and no more GPU memory).
type ModelKey = (String, Option<(u8, u8)>);

/// Which look: a role, and the faction whose own model it is (`None` = the
/// role's generic `model`).
type LookKey = (u16, Option<Faction>);

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
    /// The generic look of every role: its `model`.
    looks: BTreeMap<u16, RoleLook>,
    /// The looks that a faction's `models` entry gives a role, where it has one
    /// that could be used.
    faction_looks: BTreeMap<(u16, Faction), RoleLook>,
    /// The size variants appended after the style's own shapes, with the shape
    /// whose level-of-detail chain each keeps.
    variants: Vec<(u8, Shape)>,
    /// The glTF models of the roles, after the variants.
    pack: Option<LoadedPack>,
    /// Roles whose model could not be used and fell back to a primitive.
    pub warnings: Vec<String>,
}

/// The faction an owner number stands for: the seats of the sandbox
/// ([`sandbox::MERIDIAN`], [`sandbox::VEYRAN`], [`sandbox::NEUTRAL`]); `None`
/// for any other number, which is drawn with the generic model of its role.
pub fn faction_of_owner(owner: u8) -> Option<Faction> {
    sandbox::faction_of_owner(owner)
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

/// What `Roster::build` knows about the role whose models it is resolving.
struct RoleCtx<'a> {
    def: &'a UnitDef,
    /// The role's `size`, for a primitive.
    size: sf_math::Fx,
    footprint: Option<(u8, u8)>,
    /// The base shape of the role's strategic icon.
    shape: IconShape,
    /// The role's strategic icon.
    icon: IconId,
}

/// The glTF files of a build: each loaded once, with the model entries (a
/// loaded model and an icon) that the roles' shapes will be.
struct Loader<'a> {
    assets: &'a Path,
    models_ron: &'a str,
    use_models: bool,
    /// glTF models in the order they are first needed, each file (at a
    /// footprint) once.
    models: Vec<LoadedPack>,
    model_index: BTreeMap<ModelKey, Option<usize>>,
    /// The (model, icon) pairs the roles draw: the model *entries*, which
    /// become shapes.
    entries: Vec<(usize, IconId)>,
    warnings: Vec<String>,
}

impl Loader<'_> {
    /// What `model` makes of the role: a primitive of the role's size, or its
    /// glTF model. `None` for a glTF model that cannot be used (models are
    /// off, or the file does not load: a warning in that case) and the caller
    /// picks the next fallback. `faction` is only for the warning.
    fn source(
        &mut self,
        role: &RoleCtx<'_>,
        faction: Option<Faction>,
        model: &Model,
        base: &Style,
    ) -> Option<Source> {
        let path = match model {
            Model::Shape(shape) => {
                return Some(placeholder(
                    role.size,
                    role.footprint,
                    mesh_of(*shape),
                    role.icon,
                    base,
                ));
            }
            Model::Glb(path) if self.use_models => path,
            Model::Glb(_) => return None,
        };
        // A structure's model is fitted to its footprint, so the same file
        // under two footprints is two models.
        let fit = role.footprint.map(|(w, h)| {
            (
                f32::from(w) * METRES_PER_CELL,
                f32::from(h) * METRES_PER_CELL,
            )
        });
        let key = (path.clone(), role.footprint);
        let found = match self.model_index.get(&key) {
            Some(found) => *found,
            None => {
                let loaded =
                    load_pack_model(self.models_ron, path, role.shape.into(), self.assets, fit);
                let found = match loaded {
                    Ok(one) => {
                        self.models.push(one);
                        Some(self.models.len() - 1)
                    }
                    Err(e) => {
                        let who = match faction {
                            Some(f) => format!("{} ({f:?})", role.def.id),
                            None => role.def.id.clone(),
                        };
                        let then = if faction.is_some() {
                            "drawn with its generic model"
                        } else {
                            "drawn as a primitive"
                        };
                        self.warnings.push(format!("{who}: {e}; {then}"));
                        None
                    }
                };
                self.model_index.insert(key, found);
                found
            }
        }?;
        let entry = self
            .entries
            .iter()
            .position(|e| *e == (found, role.icon))
            .unwrap_or_else(|| {
                self.entries.push((found, role.icon));
                self.entries.len() - 1
            });
        Some(Source::Model(entry))
    }
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
        let mut loader = Loader {
            assets,
            models_ron,
            use_models,
            models: Vec::new(),
            model_index: BTreeMap::new(),
            entries: Vec::new(),
            warnings: Vec::new(),
        };
        // What every role (and every faction's entry of a role) is drawn with.
        let mut sources: BTreeMap<LookKey, Source> = BTreeMap::new();

        for def in defs.units() {
            let sim = defs
                .sim_unit(def.kind)
                .ok_or_else(|| format!("{}: no converted role", def.id))?;
            let shape = icon_of(def.strategic_icon);
            let icon = icons.resolve(def.icon.as_deref(), shape);
            if let Some(name) = &def.icon
                && icons.lookup(name).is_none()
            {
                loader.warnings.push(format!(
                    "{}: icon \"{name}\" is not in the icon set; drawn as its base shape",
                    def.id
                ));
            }
            let role = RoleCtx {
                def,
                size: sim.size,
                footprint: sim.footprint,
                shape,
                icon,
            };
            // The generic model: always a look. What cannot be drawn as a
            // model is the primitive of what the role is.
            let generic = loader
                .source(&role, None, &def.model, base)
                .unwrap_or_else(|| {
                    let mesh = mesh_of(fallback_shape(def.category));
                    placeholder(sim.size, sim.footprint, mesh, icon, base)
                });
            sources.insert((def.kind, None), generic);
            // A faction's own model: a look only if it can be used; otherwise
            // that faction's units are drawn with the generic one.
            for (faction, model) in &def.models {
                if let Some(source) = loader.source(&role, Some(*faction), model, base) {
                    sources.insert((def.kind, Some(*faction)), source);
                }
            }
        }
        let Loader {
            models,
            entries,
            warnings,
            ..
        } = loader;

        // Fix the numbers: the style's shapes, the variants, the models.
        let mut variants: Vec<(u8, Shape)> = Vec::new();
        let mut variant_at: BTreeMap<Source, usize> = BTreeMap::new();
        // Ascending by kind and then faction, so the numbers do not depend on
        // the order of the file.
        let kinds_by_source: Vec<(LookKey, Source)> = sources.into_iter().collect();
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
        let mut faction_looks = BTreeMap::new();
        for ((kind, faction), source) in kinds_by_source {
            let def = defs
                .unit_by_kind(kind)
                .ok_or_else(|| format!("kind {kind}: no definition"))?;
            let (shape, model) = match &source {
                Source::Placeholder { .. } => (base_count + variant_at[&source], false),
                Source::Model(i) => (model_base + i, true),
            };
            let look = RoleLook {
                id: def.id.clone(),
                shape: shape as u8,
                model,
            };
            match faction {
                None => {
                    looks.insert(kind, look);
                }
                Some(faction) => {
                    faction_looks.insert((kind, faction), look);
                }
            }
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
            faction_looks,
            variants,
            pack,
            warnings,
        })
    }

    /// The generic look of the role with this kind number (its `model`).
    pub fn look(&self, kind: Kind) -> Option<&RoleLook> {
        self.looks.get(&kind.0)
    }

    /// The look of a unit of this role owned by `faction`: the faction's own
    /// model if the role has one that could be used, else the generic look.
    /// `None` (an owner that is no known side) gets the generic look.
    pub fn look_for(&self, kind: Kind, faction: Option<Faction>) -> Option<&RoleLook> {
        faction
            .and_then(|f| self.faction_looks.get(&(kind.0, f)))
            .or_else(|| self.look(kind))
    }

    /// The shape an entity of this kind is drawn as, whoever owns it; `None`
    /// for a number that is no role.
    pub fn shape_of(&self, kind: Kind) -> Option<u8> {
        self.look(kind).map(|l| l.shape)
    }

    /// The shape of a unit of this kind owned by `faction`
    /// ([`Roster::look_for`]).
    pub fn shape_for(&self, kind: Kind, faction: Option<Faction>) -> Option<u8> {
        self.look_for(kind, faction).map(|l| l.shape)
    }

    /// The looks that factions' own models give roles, ascending by kind and
    /// then faction.
    pub fn faction_looks(&self) -> impl Iterator<Item = (Kind, Faction, &RoleLook)> {
        self.faction_looks
            .iter()
            .map(|(&(k, f), l)| (Kind(k), f, l))
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

    /// The shipped definitions built into a roster after `edit` changed the
    /// text of `unit_defs.ron` (for what the shipped roles no longer do, such
    /// as two roles drawn with one file).
    fn built_with(edit: impl Fn(&str) -> String) -> (UnitDefs, Roster) {
        use sf_data::units::{
            DefFile, DefSources, EMBEDDED_CLASSES, EMBEDDED_UNITS, EMBEDDED_WEAPONS,
        };
        let units = edit(&EMBEDDED_UNITS.replace("\r\n", "\n"));
        let (classes, weapons) = (
            EMBEDDED_CLASSES.replace("\r\n", "\n"),
            EMBEDDED_WEAPONS.replace("\r\n", "\n"),
        );
        let sources = DefSources {
            classes: DefFile {
                name: "classes.ron",
                text: &classes,
            },
            weapons: DefFile {
                name: "weapons.ron",
                text: &weapons,
            },
            units: DefFile {
                name: "units.ron",
                text: &units,
            },
        };
        let defs = UnitDefs::from_sources(&sources, Some(&assets())).unwrap();
        let roster =
            Roster::build(&defs, &base(), &icons(), &assets(), &models_ron(), true).unwrap();
        (defs, roster)
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
        // The shipped roles name a file of their own each. To see how roles
        // that name one file are drawn, give the decoy the tower's file and the
        // transport truck the repair truck's (the testpack ones they had).
        let (defs, roster) = built_with(|text| {
            text.replace(
                "set_v4/meridian_decoy_emplacement.glb",
                "testpack_v1/models/verdedigingstoren.glb",
            )
            .replace(
                "set_v4/veyran_transport_truck.glb",
                "testpack_v1/models/pantserwagen.glb",
            )
        });
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
            mesh("sniper"),
            MeshKind::Cylinder,
            "infantry with a glb file"
        );
        assert_eq!(mesh("fuel_depot"), MeshKind::Cube, "an object's glb file");
        assert_eq!(mesh("gunboat"), MeshKind::Cone, "a vessel's glb file");
        // A role that names a primitive keeps it (the infantry's own is a cone,
        // not the cylinder its sort of thing gets).
        assert_eq!(mesh("rifleman"), MeshKind::Cone);
        assert_eq!(mesh("heavy_gun_team"), MeshKind::Cone);
        assert_eq!(mesh("barracks"), MeshKind::Cube);
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
            .flat_map(|d| {
                d.all_models()
                    .filter(|m| matches!(m, Model::Glb(_)))
                    .map(|m| (format!("{m:?}"), d.footprint.map(|f| (f.width, f.height))))
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
                .all(|w| w.contains("drawn as a primitive")
                    || w.contains("drawn with its generic model"))
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
        // The generic look of the structures of both factions is still a
        // primitive (what an owner without a faction model gets): five of them.
        assert!(seen >= 5, "structures with a primitive: {seen}");
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
        // The generic looks of the infantry of both factions, and the one
        // role set v4 has no usable model for.
        for id in [
            "rifleman",
            "rocket_trooper",
            "engineer",
            "scout",
            "heavy_gun_team",
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

    fn shape_in(roster: &Roster, defs: &UnitDefs, id: &str, f: Option<Faction>) -> u8 {
        roster
            .shape_for(Kind(defs.kind_of(id).unwrap()), f)
            .unwrap_or_else(|| panic!("{id}"))
    }

    /// The meshes a shape is drawn with (what makes two models different).
    fn meshes_of(roster: &Roster, shape: u8) -> Vec<usize> {
        let set = roster.model_set(&roster.style(&base())).unwrap();
        set.model(shape).meshes().map(|m| m.index()).collect()
    }

    /// Art set v4 is wired in: a role with a model per faction is drawn with
    /// the model of its owner's faction, and the others with the one they have.
    #[test]
    fn a_unit_is_drawn_with_the_model_of_its_owners_faction() {
        let (defs, roster) = built(true);
        assert!(roster.warnings.is_empty(), "{:?}", roster.warnings);
        let (m, v, n) = (
            Some(Faction::Meridian),
            Some(Faction::Veyran),
            Some(Faction::Neutral),
        );
        // The roles of both factions: infantry, vehicles, structures.
        let dual: Vec<&str> = defs
            .units()
            .iter()
            .filter(|d| !d.models.is_empty())
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(dual.len(), 18, "{dual:?}");
        for id in ["rifleman", "light_tank", "power_plant", "harvester"] {
            assert!(dual.contains(&id), "{id}");
        }
        for &id in &dual {
            let (sm, sv) = (
                shape_in(&roster, &defs, id, m),
                shape_in(&roster, &defs, id, v),
            );
            assert_ne!(sm, sv, "{id}: one model per faction");
            let kind = Kind(defs.kind_of(id).unwrap());
            assert!(roster.look_for(kind, m).unwrap().model, "{id}");
            assert!(roster.look_for(kind, v).unwrap().model, "{id}");
            assert_ne!(meshes_of(&roster, sm), meshes_of(&roster, sv), "{id}");
            // Neutral and an owner that is no side: the generic model (a
            // primitive or an older stand-in), which is neither faction's.
            let generic = roster.shape_of(kind).unwrap();
            assert_eq!(shape_in(&roster, &defs, id, n), generic, "{id}");
            assert_eq!(shape_in(&roster, &defs, id, None), generic, "{id}");
            assert_ne!(sm, generic, "{id}");
            assert_ne!(sv, generic, "{id}");
        }
        // A primitive generic look stays a primitive (the infantry's cone).
        for id in ["rifleman", "rocket_trooper", "engineer", "scout"] {
            let kind = Kind(defs.kind_of(id).unwrap());
            assert!(!roster.look(kind).unwrap().model, "{id}");
        }
        // A role of one faction has one model, whoever asks. Set v3's
        // demolition team is the one that is not from set v4.
        for id in [
            "sniper",
            "medic",
            "combat_engineer",
            "demolition_team",
            "civilian",
            "technician",
            "transport_truck",
            "light_transport",
            "amphibious_transport",
            "gunboat",
            "decoy_emplacement",
            "fortified_bunker",
            "ferry",
            "ore_field",
        ] {
            let kind = Kind(defs.kind_of(id).unwrap());
            let own = roster.shape_of(kind).unwrap();
            assert!(roster.look(kind).unwrap().model, "{id}: a glTF model");
            for f in [m, v, n, None] {
                assert_eq!(shape_in(&roster, &defs, id, f), own, "{id}");
            }
        }
        // The roles that stay as they were are a primitive (heavy_gun_team,
        // which has no usable v4 model) or an older stand-in (supply_truck).
        let kind = |id: &str| Kind(defs.kind_of(id).unwrap());
        assert!(!roster.look(kind("heavy_gun_team")).unwrap().model);
        assert!(roster.look(kind("supply_truck")).unwrap().model);
        // Every faction look is a role of the definitions, in the style, with
        // the icon of the role.
        let style = full_style(&roster);
        let atlas = icons();
        let mut seen = 0;
        for (kind, faction, look) in roster.faction_looks() {
            let def = defs.unit_by_kind(kind.0).unwrap();
            assert!(def.factions.contains(&faction), "{}", def.id);
            assert!(usize::from(look.shape) < style.shape_count());
            let want = atlas.resolve(def.icon.as_deref(), icon_of(def.strategic_icon));
            assert_eq!(style.shape(look.shape).icon, want, "{}", def.id);
            seen += 1;
        }
        assert_eq!(seen, 36, "eighteen roles, two factions");
    }

    /// With models off, or a faction's file missing, its units get the generic
    /// look; a file that loads is still used by the other faction.
    #[test]
    fn a_faction_model_that_is_missing_falls_back_to_the_generic_one() {
        let (defs, off) = built(false);
        assert_eq!(off.faction_looks().count(), 0);
        let rifleman = Kind(defs.kind_of("rifleman").unwrap());
        for f in [Faction::Meridian, Faction::Veyran] {
            assert_eq!(
                off.shape_for(rifleman, Some(f)),
                off.shape_of(rifleman),
                "models off"
            );
        }

        let dir = std::env::temp_dir().join(format!("sf_roster_faction_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("set_v4")).unwrap();
        std::fs::copy(
            assets().join("set_v4/veyran_rifleman.glb"),
            dir.join("set_v4/veyran_rifleman.glb"),
        )
        .unwrap();
        let roster = Roster::build(&defs, &base(), &icons(), &dir, &models_ron(), true);
        std::fs::remove_dir_all(&dir).ok();
        let roster = roster.unwrap();
        let warning = roster
            .warnings
            .iter()
            .find(|w| w.starts_with("rifleman (Meridian)"))
            .unwrap_or_else(|| panic!("{:?}", roster.warnings));
        assert!(
            warning.contains("drawn with its generic model"),
            "{warning}"
        );
        let generic = roster.shape_of(rifleman).unwrap();
        assert_eq!(
            roster.shape_for(rifleman, Some(Faction::Meridian)),
            Some(generic)
        );
        let veyran = roster.look_for(rifleman, Some(Faction::Veyran)).unwrap();
        assert!(veyran.model);
        assert_ne!(veyran.shape, generic);
        // A role whose only model is missing is a primitive of what it is.
        let sniper = roster.look(Kind(defs.kind_of("sniper").unwrap())).unwrap();
        assert!(!sniper.model);
    }

    /// The owner numbers of the sandbox are the factions it gives them, so
    /// every unit it places is drawn with a model of a faction its role has.
    #[test]
    fn the_owners_of_the_sandbox_map_to_the_factions_it_gave_them() {
        use sf_data::map::generate::generate_cells;
        let defs = UnitDefs::embedded();
        let map = generate_cells(256, 256, 1);
        let config = sf_sim::WorldConfig::new(1).with_map_size(256, 256);
        let mut world = crate::playfield::make_world(config, &map)
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        crate::sandbox::schedule(&mut world, &defs);
        world.step();
        let mut owners = std::collections::BTreeSet::new();
        for v in world.snapshot().iter() {
            let faction = faction_of_owner(v.owner.0).expect("a sandbox seat");
            let def = defs.unit_by_kind(v.kind.0).unwrap();
            assert!(def.factions.contains(&faction), "{} of {faction:?}", def.id);
            owners.insert(v.owner.0);
        }
        assert_eq!(owners.len(), 3);
        assert_eq!(faction_of_owner(3), None);
        assert_eq!(faction_of_owner(255), None);
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
