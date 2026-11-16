//! The announcements (WP11.2c) in a real match: the first match (`assets/matches/m1.ron`) is played in a
//! `World` with every rule a match has, a `SimSnapshot` is captured after every tick, and the production
//! functions of the game read it (`Facts::read`, `Announcer::observe`, `announce::deaths`,
//! `announce::refusal`, `Announcer::speak`). The test says what the player does and which announcements it
//! hears, tick by tick. Only the runner's own call (a window) is left out, see `docs/rules/announcements.md`.

use std::path::{Path, PathBuf};

use sf_data::map::Map;
use sf_data::match_def::MatchDef;
use sf_data::units::{Faction, UnitDefs};
use sf_game::announce::{self, AnnounceOut, Announcement, Announcer, Facts};
use sf_game::fx_events;
use sf_game::sidebar::Catalog;
use sf_game::sim_snapshot::SimSnapshot;
use sf_game::{matchplay, playfield};
use sf_math::Vec2;
use sf_sim::{Cell, Command, EntityId, Kind, PlayerId, World, WorldConfig};

const ME: PlayerId = PlayerId(0);
const FOE: PlayerId = PlayerId(1);

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// Records what is played; never busy, every event has a sound.
#[derive(Default)]
struct Speaker {
    heard: Vec<(u64, &'static str)>,
    tick: u64,
    names: Vec<String>,
}

impl AnnounceOut for Speaker {
    fn has_sound(&self, _event: &str) -> bool {
        true
    }
    fn busy(&self) -> bool {
        false
    }
    fn play(&mut self, event: &str) {
        let name = Announcement::ALL
            .iter()
            .find(|a| a.event() == event)
            .map(|a| a.event())
            .expect("an announcement");
        self.heard.push((self.tick, name));
        self.names.push(event.to_string());
    }
}

struct Game {
    world: World,
    defs: UnitDefs,
    catalog: Catalog,
    announcer: Announcer,
    speaker: Speaker,
}

impl Game {
    fn new() -> Game {
        Game::started(false)
    }

    /// The match, started. `shipped`: with the brake of the shipped `audio.ron`; otherwise with no gaps at all.
    fn started(shipped: bool) -> Game {
        let def = MatchDef::embedded();
        let defs = UnitDefs::embedded();
        let map = Map::load(&matchplay::map_dir(&def, &assets())).unwrap();
        let cells = map.size_in_cells();
        let world = playfield::make_world(
            WorldConfig::new(def.seed).with_map_size(cells.0, cells.1),
            &map,
        )
        .unwrap();
        let mut world = matchplay::configure(world, &def, &defs).unwrap();
        matchplay::schedule(&mut world, &def, &defs).unwrap();
        let catalog = Catalog::build(
            &defs,
            Faction::Meridian,
            sf_game::sidebar_view::shipped_look(),
            playfield::shipped_build_speed(),
        );
        // The shipped numbers, with no gaps: every raised announcement is heard in the tick it is raised, so
        // the test shows the moments (the brake has its own tests).
        let mut config = sf_audio::AudioConfig::embedded().announce;
        if !shipped {
            config.min_gap_ms = 0;
            config.fallback.repeat_gap_ms = 0;
            config.events.values_mut().for_each(|r| r.repeat_gap_ms = 0);
        }
        let mut g = Game {
            world,
            defs,
            catalog,
            announcer: Announcer::new(config),
            speaker: Speaker::default(),
        };
        g.step();
        g
    }

    fn kind(&self, role: &str) -> Kind {
        Kind(self.defs.kind_of(role).unwrap())
    }

    /// One tick, then what the runner does with it: the snapshot's facts, the own deaths of the tick, speak.
    fn step(&mut self) {
        self.world.step();
        let snap = SimSnapshot::capture(&self.world);
        let tick = snap.tick().0;
        let now = tick * 50;
        self.speaker.tick = tick;
        self.announcer
            .observe(Facts::read(&snap, ME, Some(&self.catalog)), now);
        let mut events = Vec::new();
        fx_events::record(&self.world, &mut events);
        for a in announce::deaths(&events, ME) {
            self.announcer.raise(a, now);
        }
        // The refusals of the tick, as the runner reads them (it keeps the last ones of the sim thread).
        if let Some(a) = announce::refusal(self.world.placement_rejections(), ME) {
            self.announcer.raise(a, now);
        }
        while self.announcer.speak(now, &mut self.speaker).is_some() {}
    }

    fn send(&mut self, player: PlayerId, command: Command) {
        self.world
            .schedule(self.world.tick(), player, command)
            .unwrap();
    }

    fn run(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.step();
        }
    }

    /// What was heard since `from`, as event names.
    fn heard_since(&self, from: usize) -> Vec<&'static str> {
        self.speaker.heard[from..].iter().map(|(_, n)| *n).collect()
    }

    fn mark(&self) -> usize {
        self.speaker.heard.len()
    }

    /// The first cell where `kind` may be placed for `player`, scanning the neighbourhood of the base.
    fn site(&self, kind: Kind, player: PlayerId) -> Cell {
        for y in 54..86u32 {
            for x in 0..40u32 {
                let cell = Cell::new(x, y);
                if self
                    .world
                    .preview_placement(kind, cell, player)
                    .verdict
                    .is_ok()
                {
                    return cell;
                }
            }
        }
        panic!("no site")
    }

    fn build(&mut self, role: &str) {
        let kind = self.kind(role);
        let cell = self.site(kind, ME);
        self.send(ME, Command::BuildStructure { kind, cell });
    }

    fn run_until(&mut self, what: &str, max: u64, mut done: impl FnMut(&Game) -> bool) {
        for _ in 0..max {
            if done(self) {
                return;
            }
            self.step();
        }
        panic!(
            "{what}: not within {max} ticks; heard {:?}",
            self.speaker.heard
        );
    }

    fn mine(&self, role: &str) -> Vec<EntityId> {
        let kind = self.kind(role);
        self.world
            .snapshot()
            .iter()
            .filter(|v| v.owner == ME && v.kind == kind)
            .map(|v| v.id)
            .collect()
    }
}

/// The starting objects are not news: after 100 quiet ticks nothing has been said.
#[test]
fn the_starting_objects_of_a_match_are_not_announced() {
    let mut g = Game::new();
    g.run(160);
    assert_eq!(g.speaker.heard, [], "nothing happened yet");
    assert!(g.announcer.pending().is_empty());
}

#[test]
fn building_a_base_and_making_units_is_announced_at_each_moment() {
    let mut g = Game::new();
    g.run(110);
    let start = g.mark();

    // A power plant is placed: "Building" in the tick the world takes it.
    g.build("power_plant");
    g.step();
    g.step();
    assert_eq!(
        g.heard_since(start),
        ["announce.building"],
        "{:?}",
        g.speaker.heard
    );

    // It goes up: "Structure ready" (and what it unlocks).
    g.run_until("the plant goes up", 6000, |g| {
        g.world.under_construction() == 0
    });
    g.step();
    let heard = g.heard_since(start);
    assert!(heard.contains(&"announce.structure_ready"), "{heard:?}");

    // A barracks needs a plant: now it can go up, and it opens the units.
    let before = g.mark();
    g.build("barracks");
    g.run_until("the barracks goes up", 6000, |g| {
        g.mine("barracks").len() == 1 && g.world.under_construction() == 0
    });
    g.step();
    let heard = g.heard_since(before);
    assert_eq!(heard[0], "announce.building", "{heard:?}");
    assert!(heard.contains(&"announce.structure_ready"), "{heard:?}");
    assert!(
        heard.contains(&"announce.new_options"),
        "units are new: {heard:?}"
    );
    println!("heard so far: {:?}", g.speaker.heard);

    // A unit is queued: "Training"; it comes out: "Unit ready".
    let barracks = g.mine("barracks")[0];
    let rifleman = g.kind("rifleman");
    let before = g.mark();
    g.send(
        ME,
        Command::QueueUnit {
            factory: barracks,
            kind: rifleman,
        },
    );
    g.step();
    g.step();
    assert_eq!(g.heard_since(before), ["announce.training"]);
    let before = g.mark();
    g.run_until("a rifleman", 3000, |g| !g.mine("rifleman").is_empty());
    g.step();
    assert_eq!(g.heard_since(before), ["announce.unit_ready"]);

    // Two more, then the last one is taken out again: "Cancelled", and no unit came out.
    g.send(
        ME,
        Command::QueueUnit {
            factory: barracks,
            kind: rifleman,
        },
    );
    g.send(
        ME,
        Command::QueueUnit {
            factory: barracks,
            kind: rifleman,
        },
    );
    g.step();
    g.step();
    let before = g.mark();
    g.send(
        ME,
        Command::CancelUnit {
            factory: barracks,
            index: 1,
        },
    );
    g.step();
    g.step();
    assert_eq!(g.heard_since(before), ["announce.cancelled"]);
    println!("heard: {:?}", g.speaker.heard);
}

impl Game {
    /// A power plant, a barracks and a refinery standing (the refinery brings a free harvester).
    fn with_base() -> Game {
        Game::base(false)
    }

    fn base(shipped: bool) -> Game {
        let mut g = Game::started(shipped);
        g.run(110);
        for role in ["power_plant", "barracks", "refinery"] {
            g.build(role);
            g.step();
            g.run_until(role, 8000, |g| g.world.under_construction() == 0);
        }
        g.run_until("the harvester", 3000, |g| !g.mine("harvester").is_empty());
        g.run(5);
        g
    }

    fn spawn(&mut self, role: &str, owner: PlayerId, near: EntityId, dx: i64) -> EntityId {
        let at = self.world.get(near).unwrap().pos;
        let pos = Vec2::from_ints((at.x.floor_int() + dx) as i32, at.y.floor_int() as i32);
        let kind = self.kind(role);
        let before: Vec<EntityId> = self.world.snapshot().iter().map(|v| v.id).collect();
        self.send(owner, Command::SpawnUnit { kind, pos, owner });
        self.step();
        self.world
            .snapshot()
            .iter()
            .map(|v| v.id)
            .find(|id| !before.contains(id))
            .expect("spawned")
    }
}

/// An enemy tank beside the player's harvester: the harvester is hit (and the base after it), a damaged structure
/// is repaired, a plant is sold, and what is destroyed is announced. The opponent's own unit appearing says
/// nothing.
#[test]
fn an_attack_a_repair_a_sale_and_the_losses_are_announced_for_the_local_player_only() {
    let mut g = Game::with_base();
    let harvester = g.mine("harvester")[0];
    let mark = g.mark();
    // The opponent's tank appears: not a "unit ready", and nothing else yet.
    let tank = g.spawn("light_tank", FOE, harvester, 4);
    assert!(g.world.get(tank).is_some_and(|v| v.owner == FOE));
    assert!(
        g.heard_since(mark).is_empty(),
        "the opponent's unit is not ours"
    );
    g.run_until("the harvester is hit", 400, |g| {
        g.heard_since(mark)
            .contains(&"announce.harvester_under_attack")
    });
    // A structure is hit as well: the base is under attack.
    g.run_until("a structure is hit", 800, |g| {
        g.heard_since(mark).contains(&"announce.base_under_attack")
    });
    assert!(
        !g.heard_since(mark).contains(&"announce.unit_ready"),
        "{:?}",
        g.heard_since(mark)
    );

    // A damaged structure is repaired: "Repairing", once.
    let hurt = g
        .world
        .snapshot()
        .iter()
        .filter(|v| v.owner == ME)
        .filter(|v| {
            g.world
                .stats_of(v.id)
                .is_some_and(|s| s.footprint.is_some() && v.hp < s.hp)
        })
        .map(|v| v.id)
        .next()
        .expect("a damaged structure");
    let before = g.mark();
    g.send(
        ME,
        Command::Repair {
            structure: hurt,
            on: true,
        },
    );
    g.step();
    g.step();
    assert_eq!(g.heard_since(before), ["announce.repairing"]);

    // The power plant is sold: "Structure sold", and the base is short of power.
    let plant = g.mine("power_plant")[0];
    let before = g.mark();
    g.send(ME, Command::Sell { structure: plant });
    g.step();
    g.step();
    assert_eq!(
        g.heard_since(before),
        ["announce.structure_sold", "announce.low_power"]
    );

    // The tank goes on: a unit or a structure of ours is destroyed and announced.
    g.run_until("a loss is announced", 6000, |g| {
        let h = g.heard_since(mark);
        h.contains(&"announce.unit_lost") || h.contains(&"announce.structure_lost")
    });
    println!("heard: {:?}", g.speaker.heard);
}

/// A match decided by the world: in the tick the last construction centre falls, the player hears the result.
#[test]
fn victory_and_defeat_are_announced_once_in_the_tick_the_match_is_decided() {
    // The opponent's construction centre is slot 11, the player's slot 0 (`matchplay` tests).
    for (loser, centre, event) in [(FOE, 11, "announce.victory"), (ME, 0, "announce.defeat")] {
        let mut g = Game::new();
        g.run(110);
        g.send(
            ME,
            Command::Despawn {
                id: EntityId::new(centre, 0),
            },
        );
        g.step();
        let decided = g.world.match_result().expect("decided");
        assert_eq!(
            g.speaker.heard,
            [(decided.tick.0 + 1, event)],
            "{event} ({loser:?} fell)"
        );
        // Once: more ticks say nothing more.
        g.run(100);
        assert_eq!(g.speaker.heard.len(), 1);
    }
}

/// The simulation refuses a placement of the local player: "Cannot build there". The same refusal of the
/// opponent is not ours to hear.
#[test]
fn a_refused_placement_of_the_local_player_says_cannot_deploy_and_the_opponents_does_not() {
    let mut g = Game::new();
    g.run(110);
    let far = Cell::new(1, 1);
    let kind = g.kind("power_plant");
    // The opponent's refusal.
    g.send(FOE, Command::BuildStructure { kind, cell: far });
    g.step();
    assert!(
        g.world
            .placement_rejections()
            .iter()
            .any(|r| r.player == FOE),
        "the opponent's placement is refused"
    );
    assert_eq!(g.speaker.heard, []);
    // Ours: far from the base.
    g.send(ME, Command::BuildStructure { kind, cell: far });
    g.step();
    assert!(
        g.world
            .placement_rejections()
            .iter()
            .any(|r| r.player == ME)
    );
    assert_eq!(g.heard_since(0), ["announce.cannot_deploy"]);
}

/// Four refineries at once drain the credits: the building waits for money, "Not enough credits", once.
#[test]
fn running_out_of_credits_while_building_is_announced_once() {
    let mut g = Game::new();
    g.run(110);
    for role in ["power_plant", "barracks"] {
        g.build(role);
        g.step();
        g.run_until(role, 8000, |g| g.world.under_construction() == 0);
    }
    let mark = g.mark();
    for _ in 0..4 {
        g.build("refinery");
        g.step();
        g.step();
    }
    g.run(1500);
    assert_eq!(g.world.credits(ME), 0);
    let heard = g.heard_since(mark);
    assert_eq!(
        heard
            .iter()
            .filter(|n| **n == "announce.insufficient_funds")
            .count(),
        1,
        "{heard:?}"
    );
    assert_eq!(
        heard.iter().filter(|n| **n == "announce.building").count(),
        4
    );
}

/// The same long attack with the shipped brake (`audio.ron`): the opponent's tank shoots at the base for two
/// and a half minutes of game time, and the player hears it about every 20 s, never two announcements within
/// 2 s, and every event no more often than its own gap.
#[test]
fn the_shipped_brake_thins_a_long_attack_to_one_announcement_every_20_seconds() {
    let mut g = Game::base(true);
    let harvester = g.mine("harvester")[0];
    let mark = g.mark();
    g.spawn("light_tank", FOE, harvester, 4);
    g.run(3000);
    let heard: Vec<(u64, &str)> = g.speaker.heard[mark..].to_vec();
    println!("heard: {heard:?}");
    // 3000 ticks are 150 s: the unthinned run says "base under attack" every 40 ticks.
    let base: Vec<u64> = heard
        .iter()
        .filter(|(_, n)| *n == "announce.base_under_attack")
        .map(|(t, _)| *t)
        .collect();
    assert!(
        (3..=8).contains(&base.len()),
        "{} times: {base:?}",
        base.len()
    );
    assert!(
        base.windows(2).all(|w| w[1] - w[0] >= 400),
        "20 s = 400 ticks: {base:?}"
    );
    // Never two within 2 s (40 ticks), whatever they are.
    let times: Vec<u64> = g.speaker.heard.iter().map(|(t, _)| *t).collect();
    assert!(times.windows(2).all(|w| w[1] - w[0] >= 40), "{times:?}");
    // Every event keeps its own repeat gap (ticks of 50 ms).
    let config = sf_audio::AudioConfig::embedded().announce;
    for (event, rule) in &config.events {
        let at: Vec<u64> = g
            .speaker
            .heard
            .iter()
            .filter(|(_, n)| n == event)
            .map(|(t, _)| *t)
            .collect();
        let gap = u64::from(rule.repeat_gap_ms) / 50;
        assert!(at.windows(2).all(|w| w[1] - w[0] >= gap), "{event}: {at:?}");
    }
}
