//! The headless twin of the benchmark scene (WP5.6b): the world of `sf_demo --bench` and of
//! `sf_demo --bench --fight`, built by the functions the runner uses (`bench_scene`) and stepped with no window
//! and no GPU. The sim thread reports the checksum after every 100th tick (`SimHandle::checkpoints`, from
//! `StepReport::checksum`); so does this test, and the values were checked equal to a real `sf_demo --bench`
//! and `--bench --fight` run for the checkpoints it reached (see `docs/bench/README.md`).
//!
//! What is pinned, and why it is three lists and not one:
//!
//! * `M0` (`docs/bench/m0-2026-10-22.json`): what the laptop run of M0 recorded, 15 checkpoints. It is the
//!   world of that day, **without ore**. WP7.1 (ore on the map, 27-10-2026) put the ore in the world and in its
//!   checksum, so the plain benchmark of today no longer ends at `0x9e196a6ced2ef024`; this test builds the
//!   world without the ore and shows the old values come back, which is the measured cause. The simulation
//!   itself is what it was.
//! * `PLAIN`: the plain benchmark as `sf_demo --bench` builds it today (with ore).
//! * `FIGHT`: the fight benchmark as `sf_demo --bench --fight` builds it (with ore, the combat of the game and
//!   the skirmish of `skirmish`).
//!
//! A change to the scene, the skirmish or the simulation that moves one of them fails here, and the change that
//! moved it has to say why (checklist line 3).

use sf_data::map::generate::generate_cells;
use sf_data::units::UnitDefs;
use sf_game::bench_scene;
use sf_game::population::Population;
use sf_game::runner::load_pack;
use sf_sim::World;

const CELLS: u32 = 256;
const UNITS: u32 = 5000;

/// The checksum after every 100th tick up to 1500, as recorded by the laptop run of M0 on 22-10-2026.
const M0: [u64; 15] = [
    0x31dd7bed928a9fbd,
    0xc9e1a1952b5d0de7,
    0xb601b5b6774e4860,
    0x98b5f8f389f0a68a,
    0x17644906020fb586,
    0x41080ed619d78974,
    0xb8b97d9b066a7659,
    0xfd4f0bbcb2f41908,
    0xb8b137a4af4221f7,
    0xbc988b5226521da3,
    0xa8d32a4689268c39,
    0xc8b3259b8fc9eb9f,
    0x1d3fb045feca8dd9,
    0xd5dd8287278417c9,
    0x9e196a6ced2ef024,
];

/// The plain benchmark's world today, with the ore of WP7.1.
const PLAIN: [u64; 15] = [
    0xd98ce9a06ebccfa7,
    0xaf4c01fe1d253cc7,
    0x2074de0ee716e307,
    0x860e7cbaedd216c9,
    0x7c99263b196fee8a,
    0x9fcbbef0949827fe,
    0x0716b5a4f8ad10a0,
    0xa8c7acd34afe2878,
    0xb9f5df0a348bf1b9,
    0x65f48902c0a29170,
    0x5b3abf6e2d1023ed,
    0x9824c9aadd787d59,
    0xf5727d2580a38c51,
    0x1a6f7c8ca5c00a58,
    0x8e57352fc4bdec7b,
];

/// The fight benchmark's world.
const FIGHT: [u64; 15] = [
    0x1a05bef657f1e420,
    0x7ca7774f19e37632,
    0x25080df8e24caf75,
    0xb8efc3612e4f1fd6,
    0xd598f13f51556a32,
    0x7a2531f7bb74348f,
    0x8095ce5d07399cf8,
    0xe2d72750daf289e9,
    0x25cdece94206d875,
    0x507f72474aee9a32,
    0x32c3736dad00d1d6,
    0x634346427aeae431,
    0x6ce70653cce7c377,
    0xf8b808e3672a785a,
    0x568d257d092daa7b,
];

/// What the demo's scene is made of, as `Runner::new` makes it: 256 x 256 cells, the test pack, 5,000 units.
struct Scene {
    world: World,
    buildings: u32,
    armed: u32,
}

/// `fight`: the combat of the game and the skirmish. `ore`: the ore of the map (the game has it).
fn scene(fight: bool, ore: bool) -> Scene {
    let map = generate_cells(CELLS, CELLS, 1);
    let pack = load_pack("testpack").expect("the test pack loads");
    let population = Population::new(Some(&pack));
    let mut world = if ore {
        bench_scene::empty_world(&map).expect("the demo map starts a world")
    } else {
        // The world of 22-10-2026: the terrain layer, no ore.
        let terrain = sf_game::playfield::sim_terrain(&map)
            .expect("the layer is the shipped table's")
            .expect("the generated map has a layer");
        World::with_terrain(bench_scene::world_config((CELLS, CELLS)), terrain)
            .expect("the terrain starts a world")
    };
    let defs = UnitDefs::embedded();
    if fight {
        world = bench_scene::armed(world, &defs);
    }
    let (buildings, armed) = bench_scene::populate(
        &mut world,
        &population,
        UNITS,
        (CELLS, CELLS),
        fight.then_some(&defs),
    );
    Scene {
        world,
        buildings,
        armed,
    }
}

/// Steps to tick 1500 and returns the checksum after every 100th tick, with what the world reported of shots,
/// hits and deaths in each hundred.
fn run(world: &mut World) -> (Vec<u64>, Vec<[usize; 3]>) {
    let (mut sums, mut events) = (Vec::new(), Vec::new());
    let mut window = [0; 3];
    while world.tick().0 < 1500 {
        let report = world.step();
        window[0] += world.shots().len();
        window[1] += world.impacts().len();
        window[2] += world.deaths().len();
        if world.tick().0.is_multiple_of(100) {
            sums.push(report.checksum);
            events.push(std::mem::take(&mut window));
        }
    }
    (sums, events)
}

fn hex(v: &[u64]) -> Vec<String> {
    v.iter().map(|c| format!("0x{c:016x}")).collect()
}

/// The M0 record is what the constant says (the JSON file is the source), so the list below cannot drift from it.
#[test]
fn the_m0_list_is_the_one_in_the_recorded_json() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/bench/m0-2026-10-22.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let recorded: Vec<String> = json["sim"]["checksums"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["checksum"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(recorded, hex(&M0));
    assert_eq!(json["sim"]["final_checksum"], "0x9e196a6ced2ef024");
    assert_eq!(json["scene"]["units"], UNITS);
}

/// Without the ore the plain benchmark's world is the world M0 measured: all 15 checkpoints equal, the last
/// `0x9e196a6ced2ef024` at tick 1500. This is the measured cause of the move below.
#[test]
fn the_plain_benchmark_without_ore_is_the_world_m0_recorded() {
    let mut s = scene(false, false);
    assert_eq!((s.buildings, s.armed), (24, 0));
    let (sums, events) = run(&mut s.world);
    assert_eq!(hex(&sums), hex(&M0));
    assert_eq!(*sums.last().unwrap(), 0x9e196a6ced2ef024);
    assert!(events.iter().all(|e| *e == [0, 0, 0]), "nobody is armed");
}

/// The plain benchmark as the game builds it today: it ends at `0x8e57352fc4bdec7b`, not at M0's
/// `0x9e196a6ced2ef024`, because the world now holds the ore (WP7.1); the test above is the proof.
#[test]
fn the_plain_benchmark_ends_at_these_checksums() {
    let mut s = scene(false, true);
    assert_eq!((s.buildings, s.armed), (24, 0));
    let (sums, events) = run(&mut s.world);
    assert_eq!(hex(&sums), hex(&PLAIN));
    assert_ne!(sums, M0.to_vec(), "the ore moved it: see the test above");
    assert!(events.iter().all(|e| *e == [0, 0, 0]), "nobody is armed");
}

/// The fight benchmark: the pinned list, 720 armed units in waves, and a fight that lasts the whole flight
/// (every hundred ticks has shots, hits and deaths).
#[test]
fn the_fight_benchmark_ends_at_these_checksums_and_fights_all_the_way() {
    let mut s = scene(true, true);
    assert_eq!((s.buildings, s.armed), (24, 720));
    let (sums, events) = run(&mut s.world);
    assert_eq!(hex(&sums), hex(&FIGHT));
    for (i, e) in events.iter().enumerate() {
        assert!(
            e[0] >= 50 && e[1] >= 20 && e[2] >= 1,
            "ticks {}..{}: {} shots, {} hits, {} deaths",
            i * 100,
            i * 100 + 100,
            e[0],
            e[1],
            e[2]
        );
    }
}

/// The fight benchmark has the same crowd as the plain one: after tick 0 the first 5,000 entities have the
/// same kind, owner and place, and none of them is armed; the skirmish is the entities after them, at its sites.
#[test]
fn the_fight_benchmark_has_the_crowd_of_the_plain_one_and_the_skirmish_at_its_sites() {
    let plain = scene(false, true).world;
    let fight = scene(true, true).world;
    let a: Vec<_> = plain.snapshot().iter().collect();
    let b: Vec<_> = fight.snapshot().iter().collect();
    assert_eq!(a.len(), UNITS as usize);
    // Tick 0 also made the first wave: 3 sites, 2 sides, 8 units.
    assert_eq!(b.len(), UNITS as usize + 3 * 2 * 8);
    for (p, f) in a.iter().zip(&b) {
        assert_eq!(
            (p.id, p.kind, p.owner, p.pos),
            (f.id, f.kind, f.owner, f.pos)
        );
    }
    assert!(b[..UNITS as usize].iter().all(|v| !v.has_stats), "unarmed");
    let armed = &b[UNITS as usize..];
    assert!(
        armed.iter().all(|v| v.has_stats),
        "the skirmish is made from the definitions"
    );
    // Every skirmish unit stands within 6 cells of one of the three sites; each site gets 16.
    let mut per_site = [0; 3];
    for v in armed {
        let [x, y] = sf_math::render_bridge::vec2_to_f32(v.pos);
        let site = sf_game::skirmish::SITES
            .iter()
            .position(|s| (x - s.at.0 as f32).abs() < 6.0 && (y - s.at.1 as f32).abs() < 6.0)
            .unwrap_or_else(|| panic!("a unit at ({x}, {y}) is at no site"));
        per_site[site] += 1;
    }
    assert_eq!(per_site, [16, 16, 16]);
    // Both sides: the owners are Meridian (0) and Veyran (1).
    assert!(armed.iter().all(|v| v.owner.0 <= 1));
    assert_eq!(armed.iter().filter(|v| v.owner.0 == 0).count(), 24);
}
