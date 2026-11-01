//! Unit, structure and mission-object definitions (`assets/config/unit_defs.ron`).
//!
//! One record per role of `content/units.md`. Every record has a `kind`, its
//! **permanent number**: the simulation (`sf_sim::Kind`), scenario files, saves
//! and replays name the role by it. The number is chosen when the record is
//! written, never changes, and does not depend on the record's place in the
//! file. A removed role leaves a tombstone in `retired_units` so its number is
//! never handed out again (see `kinds.rs`). `docs/rules/units.md` describes
//! every field and its unit of measure.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::building::{self, BANNED_IDS, BuildingDef};
use super::classes::{ClassTables, is_valid_name};
use super::decimal::Decimal;
use super::error::{DefError, Source};
use super::kinds::{self, Live, Retired};
use super::weapon::{WeaponDef, WeaponMount};

/// Largest speed accepted, cells per second.
const MAX_SPEED_MILLI: i64 = 20_000;
/// Largest value accepted for hit points, cost and the like.
const MAX_BIG: u32 = 1_000_000;
/// Largest footprint side, in cells.
const MAX_FOOTPRINT: u8 = 16;
/// Largest radius or distance of an ability, in cells.
const MAX_CELLS_MILLI: i64 = 64_000;
/// Most weapons one unit may carry.
const MAX_WEAPONS: usize = 4;
/// Most unit definitions.
pub const MAX_UNITS: usize = 1024;
/// Largest unit `kind`.
pub const MAX_UNIT_KIND: u16 = 4095;

/// Which side can use a role. A role can be open to more than one side;
/// `Neutral` roles (civilians, mission objects) belong to no player.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Faction {
    /// The first campaign faction.
    Meridian,
    /// The second campaign faction.
    Veyran,
    /// Nobody: civilians and mission objects.
    Neutral,
}

impl Faction {
    /// The bit of this faction in the availability mask of the simulation
    /// definitions.
    pub const fn bit(self) -> u8 {
        match self {
            Faction::Meridian => 1,
            Faction::Veyran => 2,
            Faction::Neutral => 4,
        }
    }
}

/// What sort of thing a role is. Decides which fields are required.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Category {
    /// A person: moves on foot.
    Infantry,
    /// A land vehicle (or an amphibious one).
    Vehicle,
    /// A vessel on water.
    Vessel,
    /// A building. Stands on a footprint of cells and never moves.
    Structure,
    /// Anything else that stands in the world: a crate to pick up, an ore
    /// field. Never moves; may have no hit points (zero means it cannot be
    /// damaged).
    Object,
}

impl Category {
    /// Whether this category can have a movement class.
    pub const fn mobile(self) -> bool {
        matches!(
            self,
            Category::Infantry | Category::Vehicle | Category::Vessel
        )
    }
}

/// A placeholder 3D shape (the three primitives of `units.ron`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum ModelShape {
    /// A box.
    Cube,
    /// A cylinder.
    Cylinder,
    /// A cone.
    Cone,
}

/// The base shape of a role's strategic icon: what it is drawn as when it has
/// no icon of its own (or the icon is missing), one per kind of role
/// (PLAN.md section 2.2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum IconShape {
    /// Square (structures).
    Square,
    /// Circle (infantry).
    Circle,
    /// Triangle (vehicles).
    Triangle,
    /// Half-circle, a bowl (vessels).
    HalfCircle,
}

impl IconShape {
    /// The base shape the art brief gives a category: structures and objects a
    /// square, infantry a circle, vehicles a triangle, vessels a half-circle.
    pub fn of_category(category: Category) -> IconShape {
        match category {
            Category::Structure | Category::Object => IconShape::Square,
            Category::Infantry => IconShape::Circle,
            Category::Vehicle => IconShape::Triangle,
            Category::Vessel => IconShape::HalfCircle,
        }
    }
}

/// What a role looks like up close.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Model {
    /// A placeholder primitive from `units.ron`.
    Shape(ModelShape),
    /// A glTF binary, a path relative to `assets/` (for example
    /// `testpack_v1/models/lichte_tank.glb`); the file must exist.
    Glb(String),
}

/// How many cells a structure covers: `width` along x, `height` along y.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footprint {
    /// Cells along x, 1 to 16.
    pub width: u8,
    /// Cells along y, 1 to 16.
    pub height: u8,
}

/// A structure that explodes when it is destroyed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeathBlast {
    /// Radius in cells.
    pub radius: Decimal,
    /// Damage to everything inside, before armor.
    pub damage: u32,
}

/// What a role can do besides move and shoot: generic data fields that the
/// work packages after WP3.1 read. All are optional and default to "no".
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Flags {
    /// Captures enemy structures by walking into them.
    pub can_capture: bool,
    /// Repairs friendly structures it touches.
    pub repairs_structures: bool,
    /// Places a demolition charge on a marked object.
    pub places_charges: bool,
    /// Is used up when it destroys a structure on contact.
    pub consumed_on_use: bool,
    /// Collects resources and drops them at a refinery.
    pub harvests: bool,
    /// Deploys to fire: immobile while it does.
    pub deploys: bool,
    /// Infantry it carries (0 means it carries none).
    pub transports: u8,
    /// Vehicles it carries instead of infantry (0 means none).
    pub transports_vehicles: u8,
    /// Infantry that can garrison it (a bunker).
    pub garrison: u8,
    /// Radius in cells in which it heals infantry (0 means it does not).
    pub heal_radius: Decimal,
    /// Radius in cells in which it repairs vehicles (0 means it does not).
    pub repair_radius: Decimal,
    /// Cannot be seen by the enemy while standing still in cover terrain.
    pub stealth_in_cover: bool,
    /// Structure ids it can build in the field.
    pub builds: Vec<String>,
    /// Looks like this other role to the enemy AI.
    pub disguise_as: Option<String>,
    /// Unit id that comes free with it when it is finished (a refinery's
    /// harvester).
    pub comes_with: Option<String>,
    /// A structure that can only be placed on the coast.
    pub needs_water: bool,
    /// Radius in cells around it in which the owner may build (a base anchor);
    /// 0 means none.
    pub build_radius: Decimal,
    /// Enables the minimap for its owner.
    pub minimap: bool,
    /// Explodes when destroyed.
    pub death_blast: Option<DeathBlast>,
    /// Is picked up by infantry walking onto it.
    pub pickup: bool,
    /// Fires at every faction, its own included.
    pub fires_at_all: bool,
}

/// The picture of a role in the sidebar's build menu (WP11.1): a coloured tile with a short name
/// until there is art, and where the role stands in its tab. Presentation only: the simulation never
/// sees it. Separate from [`UnitDef::icon`], which since WP3.6 is the name of the *strategic map*
/// icon. `docs/rules/sidebar.md` has the rules.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPicture {
    /// The colour of the tile, sRGB.
    pub color: (u8, u8, u8),
    /// The short name written on the tile: 1 to [`MAX_PICTURE_LABEL`] printable ASCII characters,
    /// no space at either end.
    pub label: String,
    /// Where the role stands among the roles of its category in the menu, ascending: 0 to
    /// [`MAX_PICTURE_ORDER`], unique within the category. A role without a picture follows
    /// after all that have one, by kind.
    pub order: u16,
}

/// Longest label of a build picture, in characters.
pub const MAX_PICTURE_LABEL: usize = 6;
/// Largest `order` of a build picture.
pub const MAX_PICTURE_ORDER: u16 = 999;

/// One role, as written in the file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitDef {
    /// Permanent number (what the simulation uses as `Kind`), unique among
    /// units and tombstones; the position in the file does not matter.
    pub kind: u16,
    /// Unique snake_case role name, exactly as in `content/units.md`.
    pub id: String,
    /// Which sides can use it.
    pub factions: Vec<Faction>,
    /// What sort of thing it is.
    pub category: Category,
    /// Price in credits; absent means it cannot be bought or built.
    #[serde(default)]
    pub cost: Option<u32>,
    /// Time to build, in game seconds (whole ticks); required with a `cost`,
    /// absent without.
    #[serde(default)]
    pub build_seconds: Decimal,
    /// Hit points. Zero is only allowed for an `Object` and means it cannot be
    /// damaged.
    pub hp: u32,
    /// Name of an armor type in `unit_classes.ron`.
    pub armor: String,
    /// Power produced (positive) or used (negative); structures only.
    #[serde(default)]
    pub power: i32,
    /// Name of a movement class in `unit_classes.ron`; absent for things that
    /// never move.
    #[serde(default)]
    pub movement: Option<String>,
    /// Speed in cells per game second (4 m per cell). Stored as `sf_math`
    /// fixed point after loading.
    #[serde(default)]
    pub speed: Decimal,
    /// Speed on water in cells per second, for an amphibious unit that is
    /// slower (or faster) there than on land; absent means the same as `speed`.
    #[serde(default)]
    pub water_speed: Option<Decimal>,
    /// Facing steps (of 256 per full turn) it turns per tick.
    #[serde(default)]
    pub turn_rate: u8,
    /// Facing steps (of 256 per full turn) per tick its turret turns, for a unit
    /// with a turret that turns apart from the hull (WP4.5, the glTF `turret`
    /// part's yaw joint); zero or absent means no turret. Only with a movement
    /// class.
    #[serde(default)]
    pub turret_rate: u8,
    /// Diameter in cells (for collision and selection); not for structures.
    #[serde(default)]
    pub size: Decimal,
    /// Cells covered; structures only, and required for them.
    #[serde(default)]
    pub footprint: Option<Footprint>,
    /// How far it sees, in cells.
    #[serde(default)]
    pub sight: Decimal,
    /// Ids from `weapon_defs.ron`, at most four.
    #[serde(default)]
    pub weapons: Vec<String>,
    /// What it looks like up close: for every owner without an entry in
    /// [`UnitDef::models`], and the stand-in when a model cannot be used.
    pub model: Model,
    /// Looks per faction, where a faction's units of this role are not drawn
    /// with `model` (the infantry of set v3: `meridian_rifleman.glb` and
    /// `veyran_rifleman.glb` for one role). Keys are factions of the role.
    /// Presentation only: the simulation never sees it. Absent = none.
    #[serde(default, deserialize_with = "faction_models")]
    pub models: BTreeMap<Faction, Model>,
    /// The base shape at strategic zoom: what the role is drawn as when it
    /// has no `icon`, or when the icon is not in the icon set.
    pub strategic_icon: IconShape,
    /// The name of the role's strategic icon (WP3.6): a picture of the icon set
    /// in `assets/icons` (`assets/icons/icons.ron` names the files; without an
    /// entry the file is `<name>.png`). Only the spelling is checked here
    /// (`a-z 0-9 _`); the renderer loads the set and falls back to
    /// `strategic_icon` for a name it does not have.
    #[serde(default)]
    pub icon: Option<String>,
    /// The picture in the sidebar's build menu (WP11.1), separate from `icon`. Only for a role with a
    /// `cost` (only those are built). Optional: a role without one is shown with the default picture
    /// of its tab (`sidebar.ron`) and the first letters of its id, after the roles that have one;
    /// this is also what keeps the frozen copies of the definitions loading unchanged.
    #[serde(default)]
    pub build_picture: Option<BuildPicture>,
    /// Abilities.
    #[serde(default)]
    pub flags: Flags,
    /// What a Structure is for and what it needs (WP6.1, `building.rs`); absent
    /// means no function, no exit side and no prerequisites.
    #[serde(default)]
    pub building: Option<BuildingDef>,
    /// Ids of the structures its owner must have standing before this unit can be built (WP6.5,
    /// `docs/rules/production.md`), besides the factory that builds it
    /// ([`produced_by`](super::building::produced_by), derived from the category). Only for a
    /// unit with a cost; a Structure says it in `building.requires`. Default: none.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// The `models` map, refusing a faction that is listed twice (a plain map
/// would keep the last one without a word).
fn faction_models<'de, D>(deserializer: D) -> Result<BTreeMap<Faction, Model>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, MapAccess, Visitor};
    struct Entries;
    impl<'de> Visitor<'de> for Entries {
        type Value = BTreeMap<Faction, Model>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map from a faction to a model")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut models = BTreeMap::new();
            while let Some((faction, model)) = map.next_entry::<Faction, Model>()? {
                if models.insert(faction, model).is_some() {
                    return Err(A::Error::custom(format!(
                        "models: faction {faction:?} is listed twice"
                    )));
                }
            }
            Ok(models)
        }
    }
    deserializer.deserialize_map(Entries)
}

impl UnitDef {
    /// What a unit of this role owned by `faction` looks like: the model of
    /// that faction if the role has one, otherwise the generic `model`.
    /// `None` (an owner that is no known side) gets the generic one. The last
    /// fallback, a primitive by category, is the renderer's.
    pub fn model_for(&self, faction: Option<Faction>) -> &Model {
        faction
            .and_then(|f| self.models.get(&f))
            .unwrap_or(&self.model)
    }

    /// Every model the role can be drawn with: the generic one, then one per
    /// faction in faction order.
    pub fn all_models(&self) -> impl Iterator<Item = &Model> {
        std::iter::once(&self.model).chain(self.models.values())
    }
}

/// The RON file.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnitsFile {
    units: Vec<UnitDef>,
    /// Tombstones of removed roles: their numbers are never reused.
    #[serde(default)]
    retired_units: Vec<Retired>,
}

/// An icon name: 1 to 48 of `a-z 0-9 _` (the same rule as the icon atlas in
/// `sf_render`, which cannot be named from here).
fn valid_icon_name(name: &str) -> bool {
    (1..=48).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Where a file reference is checked: with `Some(dir)` the file must exist
/// under `dir`; with `None` only the shape of the path is checked.
#[derive(Copy, Clone)]
pub(crate) struct Assets<'a> {
    pub dir: Option<&'a Path>,
    /// The directories (`dir`) of the model packs of `config/models.ron`,
    /// when that file is there: a glTF model must lie in one of them, or the
    /// renderer has no axes for it. `None` = not checked.
    pub packs: Option<&'a [String]>,
}

impl Assets<'_> {
    /// `Ok` if `rel` is a plain relative path with the right extension, and
    /// (when a directory is known) names an existing file.
    fn check(&self, rel: &str, extension: &str) -> Result<(), String> {
        let bad_shape = rel.is_empty()
            || rel.starts_with('/')
            || rel.starts_with('\\')
            || rel.contains('\\')
            || rel.contains(':')
            || rel.split('/').any(|part| part.is_empty() || part == "..");
        if bad_shape {
            return Err(format!(
                "\"{rel}\" is not a plain relative path (no \"..\", no leading slash)"
            ));
        }
        if !rel.ends_with(extension) {
            return Err(format!("\"{rel}\" should end with {extension}"));
        }
        if let Some(dir) = self.dir
            && !dir.join(rel).is_file()
        {
            return Err(format!("\"{rel}\" does not exist under {}", dir.display()));
        }
        Ok(())
    }

    /// `Ok` if `rel` lies in the directory of a model pack (when they are known).
    fn check_pack(&self, rel: &str) -> Result<(), String> {
        let Some(packs) = self.packs else {
            return Ok(());
        };
        let inside = |dir: &String| {
            rel.strip_prefix(dir.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        };
        if packs.iter().any(inside) {
            return Ok(());
        }
        Err(format!(
            "\"{rel}\" is in no model pack of models.ron (pack directories: {})",
            packs.join(", ")
        ))
    }
}

/// Parses `unit_defs.ron` and checks every record against the tables and the
/// weapons.
pub(crate) fn parse(
    src: Source<'_>,
    classes: &ClassTables,
    weapons: &[WeaponDef],
    assets: Assets<'_>,
) -> Result<(Vec<UnitDef>, Vec<Retired>), DefError> {
    src.check_numbers()?;
    let file: UnitsFile = ron::from_str(src.text).map_err(|e| src.ron_error(&e))?;
    let mut units = file.units;
    if units.is_empty() {
        return Err(src.error("the list of units is empty"));
    }
    if units.len() > MAX_UNITS {
        return Err(src.error(format!("more than {MAX_UNITS} units")));
    }
    for (i, u) in units.iter().enumerate() {
        let id = u.id.as_str();
        if !is_valid_name(id) {
            return Err(src.error_at_record(
                "id",
                id,
                0,
                format!("unit id \"{id}\" is not snake_case (a-z, 0-9, _)"),
            ));
        }
        if BANNED_IDS.contains(&id) {
            return Err(src.error_at_record(
                "id",
                id,
                0,
                format!(
                    "unit id \"{id}\" is a banned name (this game says construction_center and shipyard)"
                ),
            ));
        }
        if units[..i].iter().any(|o| o.id == u.id) {
            let nth = units[..i].iter().filter(|o| o.id == u.id).count();
            return Err(src.error_at_record(
                "id",
                id,
                nth,
                format!("unit id \"{id}\" appears twice"),
            ));
        }
        let ctx = Ctx {
            src,
            classes,
            weapons,
            units: &units,
            assets,
            u,
        };
        ctx.check()?;
    }
    check_picture_orders(src, &units)?;
    for u in &units {
        building::check_record(src, &units, u)?;
        building::check_unit_production(src, &units, u)?;
        if u.build_picture.is_some() && u.cost.is_none() {
            return Err(src.error_in_record(
                "id",
                &u.id,
                "build_picture",
                format!(
                    "unit \"{}\": build_picture on a role that cannot be built (no cost)",
                    u.id
                ),
            ));
        }
    }
    building::check_cycles(src, &units)?;
    let live: Vec<Live<'_>> = units
        .iter()
        .map(|u| Live {
            kind: u.kind,
            name: &u.id,
        })
        .collect();
    kinds::check(src, "unit", "id", MAX_UNIT_KIND, &live, &file.retired_units)?;
    units.sort_by_key(|u| u.kind);
    Ok((units, file.retired_units))
}

/// The `order` of a build picture is unique among the roles of one category: two roles at the same
/// place in a tab would be shown in whatever order the file happens to have.
fn check_picture_orders(src: Source<'_>, units: &[UnitDef]) -> Result<(), DefError> {
    for (i, u) in units.iter().enumerate() {
        let Some(p) = &u.build_picture else { continue };
        if let Some(other) = units[..i].iter().find(|o| {
            o.category == u.category && o.build_picture.as_ref().is_some_and(|q| q.order == p.order)
        }) {
            return Err(src.error_in_record(
                "id",
                &u.id,
                "build_picture",
                format!(
                    "unit \"{}\": build_picture order {} is also used by \"{}\" ({:?})",
                    u.id, p.order, other.id, u.category
                ),
            ));
        }
    }
    Ok(())
}

/// Everything the checks of one record need.
struct Ctx<'a> {
    src: Source<'a>,
    classes: &'a ClassTables,
    weapons: &'a [WeaponDef],
    units: &'a [UnitDef],
    assets: Assets<'a>,
    u: &'a UnitDef,
}

impl Ctx<'_> {
    /// An error inside this record at the quoted `needle`.
    fn at(&self, needle: &str, msg: impl std::fmt::Display) -> DefError {
        self.src.error_in_record(
            "id",
            &self.u.id,
            needle,
            format!("unit \"{}\": {msg}", self.u.id),
        )
    }

    /// An error on the record's id line.
    fn here(&self, msg: impl std::fmt::Display) -> DefError {
        self.at(&self.u.id, msg)
    }

    fn check(&self) -> Result<(), DefError> {
        self.check_factions()?;
        self.check_numbers()?;
        self.check_movement()?;
        self.check_references()?;
        self.check_flags()
    }

    fn check_factions(&self) -> Result<(), DefError> {
        let f = &self.u.factions;
        if f.is_empty() {
            return Err(self.here("factions is empty (use [Neutral] for mission objects)"));
        }
        for (i, a) in f.iter().enumerate() {
            if f[..i].contains(a) {
                return Err(self.here(format!("faction {a:?} is listed twice")));
            }
        }
        if f.contains(&Faction::Neutral) && f.len() > 1 {
            return Err(self.here("Neutral cannot be combined with another faction"));
        }
        Ok(())
    }

    fn check_numbers(&self) -> Result<(), DefError> {
        let u = self.u;
        if u.hp > MAX_BIG || (u.hp == 0 && u.category != Category::Object) {
            return Err(self.here(format!(
                "hp {} must be 1 to {MAX_BIG} (0 is only allowed for an Object)",
                u.hp
            )));
        }
        if u.build_seconds.is_negative() {
            return Err(self.here(format!("build_seconds {} is negative", u.build_seconds)));
        }
        match u.cost {
            Some(c) => {
                if c == 0 || c > MAX_BIG {
                    return Err(self.here(format!("cost {c} must be 1 to {MAX_BIG}")));
                }
                match u.build_seconds.to_ticks() {
                    None => {
                        return Err(self.here(format!(
                            "build_seconds {} is not a whole number of ticks (a multiple of 0.05)",
                            u.build_seconds
                        )));
                    }
                    Some(0) => return Err(self.here("a unit with a cost needs build_seconds")),
                    Some(_) => {}
                }
            }
            None => {
                if !u.build_seconds.is_zero() {
                    return Err(self.here("build_seconds without a cost"));
                }
            }
        }
        if u.power != 0 && u.category != Category::Structure {
            return Err(self.here("only a Structure can have power"));
        }
        if !(-100_000..=100_000).contains(&u.power) {
            return Err(self.here(format!("power {} is out of range", u.power)));
        }
        if u.sight.is_negative() || u.sight.milli() > MAX_CELLS_MILLI {
            return Err(self.here(format!("sight {} must be 0 to 64 cells", u.sight)));
        }
        if u.sight.is_zero() && u.category != Category::Object {
            return Err(self.here("sight must be above 0"));
        }
        if u.category == Category::Structure {
            let Some(fp) = u.footprint else {
                return Err(self.here("a Structure needs a footprint"));
            };
            for (side, name) in [(fp.width, "width"), (fp.height, "height")] {
                if side == 0 || side > MAX_FOOTPRINT {
                    return Err(self.at(
                        &u.id,
                        format!("footprint {name} {side} must be 1 to {MAX_FOOTPRINT}"),
                    ));
                }
            }
            if !u.size.is_zero() {
                return Err(self.here("a Structure has a footprint, not a size"));
            }
        } else {
            if u.footprint.is_some() {
                return Err(self.here("only a Structure has a footprint"));
            }
            if u.size.milli() <= 0 || u.size.milli() > 16_000 {
                return Err(self.here(format!("size {} must be above 0 and at most 16", u.size)));
            }
        }
        Ok(())
    }

    fn check_movement(&self) -> Result<(), DefError> {
        let u = self.u;
        match &u.movement {
            Some(m) => {
                if !u.category.mobile() {
                    return Err(self.at(
                        m,
                        "only Infantry, Vehicle and Vessel can have a movement class",
                    ));
                }
                if self.classes.movement_kind(m).is_none() {
                    return Err(self.at(
                        m,
                        format!("unknown movement class \"{m}\" (not in unit_classes.ron)"),
                    ));
                }
                if u.speed.milli() <= 0 || u.speed.milli() > MAX_SPEED_MILLI {
                    return Err(self.here(format!(
                        "speed {} must be above 0 and at most 20 cells per second",
                        u.speed
                    )));
                }
                if let Some(w) = u.water_speed
                    && (w.milli() <= 0 || w.milli() > MAX_SPEED_MILLI)
                {
                    return Err(self.here(format!(
                        "water_speed {w} must be above 0 and at most 20 cells per second"
                    )));
                }
                if u.turn_rate == 0 {
                    return Err(self.here("turn_rate must be at least 1 for a unit that moves"));
                }
            }
            None => {
                if u.category.mobile() {
                    return Err(self.here("an Infantry, Vehicle or Vessel needs a movement class"));
                }
                if !u.speed.is_zero()
                    || u.water_speed.is_some()
                    || u.turn_rate != 0
                    || u.turret_rate != 0
                {
                    return Err(self.here(
                        "speed, water_speed, turn_rate and turret_rate need a movement class",
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_references(&self) -> Result<(), DefError> {
        let u = self.u;
        if self.classes.armor_kind(&u.armor).is_none() {
            return Err(self.at(
                &u.armor,
                format!(
                    "unknown armor type \"{}\" (not in unit_classes.ron)",
                    u.armor
                ),
            ));
        }
        if u.weapons.len() > MAX_WEAPONS {
            return Err(self.here(format!("more than {MAX_WEAPONS} weapons")));
        }
        for (i, w) in u.weapons.iter().enumerate() {
            let Some(def) = self.weapons.iter().find(|d| d.id == *w) else {
                return Err(self.at(
                    w,
                    format!("unknown weapon \"{w}\" (not in weapon_defs.ron)"),
                ));
            };
            if def.mount == WeaponMount::Turret && u.turret_rate == 0 {
                return Err(self.at(
                    w,
                    format!(
                        "weapon \"{w}\" is mounted on a turret but the unit has no turret_rate"
                    ),
                ));
            }
            if u.weapons[..i].contains(w) {
                return Err(self.at(w, format!("weapon \"{w}\" is listed twice")));
            }
        }
        let per_faction = u
            .models
            .iter()
            .map(|(faction, model)| (format!("models {faction:?}"), model));
        for (who, model) in std::iter::once(("model".to_owned(), &u.model)).chain(per_faction) {
            if let Model::Glb(path) = model {
                self.assets
                    .check(path, ".glb")
                    .and_then(|()| self.assets.check_pack(path))
                    .map_err(|e| self.at(path, format!("{who}: {e}")))?;
            }
        }
        for faction in u.models.keys() {
            if !u.factions.contains(faction) {
                return Err(self.here(format!(
                    "models: {faction:?} is not one of the factions of this role"
                )));
            }
        }
        if let Some(name) = &u.icon
            && !valid_icon_name(name)
        {
            return Err(self.at(
                name,
                format!("icon \"{name}\": a name is 1 to 48 of a-z 0-9 _ (not a file path)"),
            ));
        }
        if let Some(p) = &u.build_picture {
            let bad_label = p.label.is_empty()
                || p.label.chars().count() > MAX_PICTURE_LABEL
                || !p.label.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                || p.label.starts_with(' ')
                || p.label.ends_with(' ');
            if bad_label {
                return Err(self.at(
                    &p.label,
                    format!(
                        "build_picture label \"{}\": 1 to {MAX_PICTURE_LABEL} printable ASCII characters, no space at either end",
                        p.label
                    ),
                ));
            }
            if p.order > MAX_PICTURE_ORDER {
                return Err(self.at(
                    "build_picture",
                    format!(
                        "build_picture order {} is above {MAX_PICTURE_ORDER}",
                        p.order
                    ),
                ));
            }
        }
        Ok(())
    }

    fn find(&self, id: &str) -> Option<&UnitDef> {
        self.units.iter().find(|d| d.id == id)
    }

    fn check_flags(&self) -> Result<(), DefError> {
        let u = self.u;
        let f = &u.flags;
        for (name, radius) in [
            ("heal_radius", f.heal_radius),
            ("repair_radius", f.repair_radius),
            ("build_radius", f.build_radius),
        ] {
            if radius.is_negative() || radius.milli() > MAX_CELLS_MILLI {
                return Err(self.here(format!("{name} {radius} must be 0 to 64 cells")));
            }
        }
        let moves = u.movement.is_some();
        for (name, on) in [
            ("harvests", f.harvests),
            ("deploys", f.deploys),
            ("can_capture", f.can_capture),
            ("repairs_structures", f.repairs_structures),
            ("places_charges", f.places_charges),
            ("consumed_on_use", f.consumed_on_use),
            ("stealth_in_cover", f.stealth_in_cover),
        ] {
            if on && !moves {
                return Err(self.here(format!("{name} needs a unit that moves")));
            }
        }
        if (f.transports > 0 || f.transports_vehicles > 0) && !moves {
            return Err(
                self.here("transports needs a unit that moves (use garrison for a structure)")
            );
        }
        if f.garrison > 0 && u.category != Category::Structure {
            return Err(self.here("garrison is for a Structure"));
        }
        if f.needs_water && u.category != Category::Structure {
            return Err(self.here("needs_water is for a Structure"));
        }
        if f.minimap && u.category != Category::Structure {
            return Err(self.here("minimap is for a Structure"));
        }
        if (!f.build_radius.is_zero()) && u.category != Category::Structure {
            return Err(self.here("build_radius is for a Structure"));
        }
        if let Some(b) = f.death_blast
            && (b.radius.milli() <= 0 || b.radius.milli() > MAX_CELLS_MILLI || b.damage == 0)
        {
            return Err(self.here("death_blast needs a radius above 0 and a damage above 0"));
        }
        for b in &f.builds {
            match self.find(b) {
                None => {
                    return Err(self.at(b, format!("builds: unknown unit \"{b}\"")));
                }
                Some(d) if d.category != Category::Structure => {
                    return Err(self.at(b, format!("builds: \"{b}\" is not a Structure")));
                }
                Some(_) => {}
            }
        }
        for (name, target) in [
            ("disguise_as", &f.disguise_as),
            ("comes_with", &f.comes_with),
        ] {
            if let Some(t) = target
                && self.find(t).is_none()
            {
                return Err(self.at(t, format!("{name}: unknown unit \"{t}\"")));
            }
        }
        Ok(())
    }
}
