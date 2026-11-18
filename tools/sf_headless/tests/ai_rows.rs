//! The measuring harness of WP9.3a-1 (`docs/rules/ai.md`, "Escalation and the harvester threat (WP9.3a-1)"): it plays the games of
//! one row of the duel tables and writes one line per game. **Not a CI test**: it is `#[ignore]`d, takes its input from the
//! environment and is run by hand in release (`cargo test --release -p sf_headless --test ai_rows -- --ignored --nocapture`).
//!
//! * `ROWS_ASSETS`: an asset directory (the shipped `config` and `maps/m1`, an `ai/` directory with the profiles, a `matches/` directory).
//! * `ROWS_MATCH`: the match file inside it (two computer seats).
//! * `ROWS_SEED`: the base seed, `ROWS_GAMES`: how many games (a multiple of 8 for the 8-cycle), `ROWS_OUT`: the file the lines go to.
//!
//! The game loop is the one of `tournament::play_seeded_sited` (same order: thinks, then the step), with two additions that read
//! only what each AI itself sees (its own observation): every launch (`attacks_launched` going up: the wave number and the value
//! of the strike group), and, per seat, the tick of the first own harvester lost and of the first own structure hit or lost. One line per game:
//! `index swapped ending seat0_attacks seat1_attacks seat0_harv_lost seat0_struct_hit seat1_harv_lost seat1_struct_hit | launches0 | launches1`
//! with `-` for "never" and launches as `wave:value:tick` separated by commas.
//!
//! **Trace (WP9.3b-0).** With `ROWS_TRACE=<file>` the harness also writes one line per game about the *first clash* of the two
//! sides, read from the world itself (the harness is omniscient; nothing of it reaches an AI and no shipped code changes). The
//! first clash is the first shot of a member of one of its strike groups (as the AI last saw them) at an armed unit of the other side (no structure, no harvester). The wave of
//! a side is every armed mobile unit it owns within [`WAVE_RADIUS`] cells (Chebyshev) of the point of contact at that tick.
//! The line: `index swapped first_launcher launch0 launch1 clash_tick contact_x contact_y first_shooter |
//! n0 value0 spread0 gather_dist0 | n1 value1 spread1 gather_dist1 | left0 left1 winner | shots0 unarmed0 shots1 unarmed1`
//! (`spread` = the largest Chebyshev distance of a wave unit from the mean cell of the wave; `gather_dist` = Chebyshev
//! distance of the contact from the side's gather cell; `left` = value of the wave units still alive [`WINDOW`] ticks later,
//! `winner` the seat with more of it, `-` on a tie; the shots are those the wave units fired in the window, `unarmed` the ones
//! at a target with no weapon: a structure, a harvester); then the whole strike group of each side alive at the contact
//! (`units value spread`, seat 0 then seat 1). `-` as the clash tick: no clash in the game.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use sf_ai::{Lens, Observation};
use sf_headless::tournament::{
    Ending, STALEMATE_WINDOW, classify, factions_for, load_setup, prepare_sited, seeds_for,
    sites_swapped,
};
use sf_math::Fx;
use sf_sim::{EntityId, PlayerId, Tick, Verdict, World};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

/// Cells (Chebyshev) from the point of contact within which an armed unit counts as part of a side's wave.
const WAVE_RADIUS: i64 = 15;
/// Ticks after the first contact in which the clash is judged.
const WINDOW: u64 = 600;

fn cell_xy(world: &World, id: EntityId) -> Option<(i64, i64)> {
    let v = world.get(id)?;
    Some((v.pos.x.floor_int(), v.pos.y.floor_int()))
}

fn cheb(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// An armed mobile unit: a weapon and no footprint.
fn armed_mobile(world: &World, id: EntityId) -> bool {
    world
        .stats_of(id)
        .is_some_and(|s| s.weapon_range > Fx::ZERO && s.footprint.is_none())
}

fn wave_value(world: &World, seat: usize, ids: &[EntityId]) -> u64 {
    let lens = Lens::new(world, PlayerId(seat as u8));
    ids.iter()
        .filter_map(|id| {
            let v = world.get(*id)?;
            lens.unit_cost(v.kind).map(u64::from)
        })
        .sum()
}

struct Clash {
    tick: u64,
    contact: (i64, i64),
    shooter: usize,
    waves: [Vec<EntityId>; 2],
    value: [u64; 2],
    spread: [i64; 2],
    gather_dist: [Option<i64>; 2],
    shots: [u32; 2],
    unarmed: [u32; 2],
    armed: BTreeMap<EntityId, bool>,
    /// The seat that was shot at first, its strike members and how many of them were on a `Move` at that tick.
    shot_at: (usize, usize, usize),
    left: Option<[u64; 2]>,
    /// The whole strike group of each side at the contact (alive members): units, value, spread.
    group: [(usize, u64, i64); 2],
}

/// The largest Chebyshev distance of a unit from the mean cell of the units.
fn spread_of(world: &World, ids: &[EntityId]) -> i64 {
    let cells: Vec<(i64, i64)> = ids.iter().filter_map(|id| cell_xy(world, *id)).collect();
    if cells.is_empty() {
        return 0;
    }
    let n = cells.len() as i64;
    let mean = (
        cells.iter().map(|c| c.0).sum::<i64>() / n,
        cells.iter().map(|c| c.1).sum::<i64>() / n,
    );
    cells.iter().map(|c| cheb(*c, mean)).max().unwrap_or(0)
}

fn find_clash(
    world: &World,
    gathers: [Option<(i64, i64)>; 2],
    strikers: &[BTreeSet<EntityId>; 2],
) -> Option<Clash> {
    let shot =
        world
            .shots()
            .iter()
            .find(|s| match (world.get(s.attacker), world.get(s.target)) {
                (Some(a), Some(t)) => {
                    a.owner != t.owner
                        && strikers
                            .get(usize::from(a.owner.0))
                            .is_some_and(|set| set.contains(&s.attacker))
                        && armed_mobile(world, s.attacker)
                        && armed_mobile(world, s.target)
                }
                _ => false,
            })?;
    let contact = cell_xy(world, shot.target)?;
    let shooter = usize::from(world.get(shot.attacker)?.owner.0);
    let mut waves: [Vec<EntityId>; 2] = [Vec::new(), Vec::new()];
    for v in world.snapshot().iter() {
        let seat = usize::from(v.owner.0);
        if seat < 2
            && armed_mobile(world, v.id)
            && cell_xy(world, v.id).is_some_and(|c| cheb(c, contact) <= WAVE_RADIUS)
        {
            waves[seat].push(v.id);
        }
    }
    let mut value = [0u64; 2];
    let mut spread = [0i64; 2];
    for seat in 0..2 {
        value[seat] = wave_value(world, seat, &waves[seat]);
        spread[seat] = spread_of(world, &waves[seat]);
    }
    Some(Clash {
        tick: world.tick().0,
        contact,
        shooter,
        waves,
        value,
        spread,
        gather_dist: [
            gathers[0].map(|g| cheb(g, contact)),
            gathers[1].map(|g| cheb(g, contact)),
        ],
        shots: [0; 2],
        unarmed: [0; 2],
        armed: BTreeMap::new(),
        shot_at: (0, 0, 0),
        left: None,
        group: [0, 1].map(|seat| {
            let ids: Vec<EntityId> = strikers[seat]
                .iter()
                .copied()
                .filter(|id| world.get(*id).is_some())
                .collect();
            (
                ids.len(),
                wave_value(world, seat, &ids),
                spread_of(world, &ids),
            )
        }),
    })
}

/// Before a step inside the window: remember which entities are armed (a target killed in the step is gone afterwards).
fn note_armed(world: &World, clash: &mut Clash) {
    for v in world.snapshot().iter() {
        clash.armed.insert(v.id, armed_mobile(world, v.id));
    }
}

/// After a step inside the window: count the shots of the wave units.
fn count_shots(world: &World, clash: &mut Clash) {
    for s in world.shots() {
        for seat in 0..2 {
            if clash.waves[seat].contains(&s.attacker) {
                clash.shots[seat] += 1;
                if !clash.armed.get(&s.target).copied().unwrap_or(false) {
                    clash.unarmed[seat] += 1;
                }
            }
        }
    }
}

fn trace_line(
    index: u32,
    swapped: bool,
    launches: [Option<u64>; 2],
    world: &World,
    clash: Option<&Clash>,
    recalled: [u32; 2],
) -> String {
    let first = match launches {
        [Some(a), Some(b)] if a < b => "0".to_string(),
        [Some(a), Some(b)] if b < a => "1".to_string(),
        [Some(_), None] => "0".to_string(),
        [None, Some(_)] => "1".to_string(),
        _ => "-".to_string(),
    };
    let head = format!(
        "{index} {} {first} {} {}",
        u8::from(swapped),
        tick_text(launches[0]),
        tick_text(launches[1])
    );
    let tail = format!("| {} {}", recalled[0], recalled[1]);
    let Some(c) = clash else {
        return format!("{head} - | - - - {tail}");
    };
    // The game ended inside the window: what was left at the end.
    let left = c.left.unwrap_or_else(|| {
        [
            wave_value(world, 0, &c.waves[0]),
            wave_value(world, 1, &c.waves[1]),
        ]
    });
    let winner = match left[0].cmp(&left[1]) {
        std::cmp::Ordering::Greater => "0",
        std::cmp::Ordering::Less => "1",
        std::cmp::Ordering::Equal => "-",
    };
    let dist = |d: Option<i64>| d.map_or("-".to_string(), |d| d.to_string());
    format!(
        "{head} {} {} {} {} | {} {} {} {} | {} {} {} {} | {} {} {} | {} {} {} {} | {} {} {} {} {} {} | {} {} {} {tail}",
        c.tick,
        c.contact.0,
        c.contact.1,
        c.shooter,
        c.waves[0].len(),
        c.value[0],
        c.spread[0],
        dist(c.gather_dist[0]),
        c.waves[1].len(),
        c.value[1],
        c.spread[1],
        dist(c.gather_dist[1]),
        left[0],
        left[1],
        winner,
        c.shots[0],
        c.unarmed[0],
        c.shots[1],
        c.unarmed[1],
        c.group[0].0,
        c.group[0].1,
        c.group[0].2,
        c.group[1].0,
        c.group[1].1,
        c.group[1].2,
        c.shot_at.0,
        c.shot_at.1,
        c.shot_at.2,
    )
}

fn tick_text(t: Option<u64>) -> String {
    t.map_or("-".to_string(), |t| t.to_string())
}

#[derive(Default)]
struct Watch {
    harvesters: BTreeSet<EntityId>,
    structures: BTreeMap<EntityId, u32>,
    harv_lost: Option<u64>,
    struct_hit: Option<u64>,
    launched: u32,
    launches: Vec<(u32, u64, u64)>,
    /// Strike groups seen so far, by id: whether the group has been `Engaged`, and whether it was called back.
    strikes: BTreeMap<u32, (bool, bool)>,
    /// Strikes called back before they were ever `Engaged`.
    recalled_early: u32,
}

/// Notes the strike groups of one think: a strike that turns `Returning` (the same group id) without ever having been `Engaged`
/// is a strike recalled early (WP9.3e-1).
fn note_strikes(w: &mut Watch, groups: &[sf_ai::state::Group]) {
    use sf_ai::state::GroupStage;
    for g in groups.iter().filter(|g| g.merge_into.is_none()) {
        match g.stage {
            GroupStage::Returning => {
                if let Some(entry) = w.strikes.get_mut(&g.id)
                    && !entry.1
                {
                    entry.1 = true;
                    if !entry.0 {
                        w.recalled_early += 1;
                    }
                }
            }
            stage => {
                let entry = w.strikes.entry(g.id).or_insert((false, false));
                entry.0 |= stage == GroupStage::Engaged;
            }
        }
    }
}

/// The seat that was shot at first: the members of its strike groups (as the AI last saw them) and how many of them stand on a
/// `Move` in the world now (their observed order).
fn shot_at(world: &World, ais: &[sf_ai::Ai], seat: usize) -> (usize, usize, usize) {
    let members: BTreeSet<EntityId> = ais[seat]
        .state()
        .groups
        .iter()
        .filter(|g| g.merge_into.is_none())
        .flat_map(|g| g.members.iter().copied())
        .collect();
    let obs = Observation::build(
        world,
        ais[seat].player(),
        &ais[seat].state().known_structures,
    );
    let on_move = obs
        .own
        .iter()
        .filter(|o| members.contains(&o.id) && matches!(o.order, sf_sim::Order::Move(_)))
        .count();
    (seat, members.len(), on_move)
}

#[test]
#[ignore]
fn play_one_row() {
    let assets = PathBuf::from(env("ROWS_ASSETS"));
    let match_path = PathBuf::from(env("ROWS_MATCH"));
    let seed: u64 = env("ROWS_SEED").parse().unwrap();
    let games: u32 = env("ROWS_GAMES").parse().unwrap();
    let out_path = env("ROWS_OUT");
    let cap = 60_000u64;
    let setup = load_setup(&assets, &match_path).unwrap();
    let mut out = std::fs::File::create(out_path).unwrap();
    let mut trace_out = std::env::var("ROWS_TRACE")
        .ok()
        .map(|p| std::fs::File::create(p).unwrap());
    for index in 0..games {
        let factions = factions_for(setup.file_factions(), index);
        let swapped = sites_swapped(index);
        let (world_seed, ai_seed) = seeds_for(seed, index);
        let p = prepare_sited(&setup, factions, swapped, world_seed, ai_seed).unwrap();
        let (mut world, mut ais) = (p.world, p.ais);
        let mut watch: Vec<Watch> = ais.iter().map(|_| Watch::default()).collect();
        let mut wanted = Vec::new();
        let mut last_loss = 0u64;
        let mut winner: Option<usize> = None;
        let mut decided = false;
        let mut clash: Option<Clash> = None;
        let mut first_launch: [Option<u64>; 2] = [None, None];
        while world.tick().0 < cap {
            for (seat, ai) in ais.iter_mut().enumerate() {
                if ai.due(world.tick()) {
                    let now = world.tick().0;
                    let obs = Observation::build(&world, ai.player(), &ai.state().known_structures);
                    let lens = Lens::new(&world, ai.player());
                    let kinds = lens.harvester_kinds();
                    let w = &mut watch[seat];
                    let harvesters: BTreeSet<EntityId> = obs
                        .own
                        .iter()
                        .filter(|o| o.footprint.is_none() && kinds.contains(&o.kind))
                        .map(|o| o.id)
                        .collect();
                    if w.harv_lost.is_none() && w.harvesters.iter().any(|h| !harvesters.contains(h))
                    {
                        w.harv_lost = Some(now);
                    }
                    w.harvesters = harvesters;
                    let structures: BTreeMap<EntityId, u32> = obs
                        .own
                        .iter()
                        .filter(|o| o.footprint.is_some() && !o.selling && o.building.is_none())
                        .map(|o| (o.id, o.hp))
                        .collect();
                    if w.struct_hit.is_none() {
                        let hit = w
                            .structures
                            .iter()
                            .any(|(id, hp)| match structures.get(id) {
                                Some(now_hp) => now_hp < hp,
                                None => true,
                            });
                        if hit {
                            w.struct_hit = Some(now);
                        }
                    }
                    w.structures = structures;
                    wanted.clear();
                    ai.think(&world, &mut wanted);
                    let st = ai.state();
                    note_strikes(w, &st.groups);
                    if st.attacks_launched > w.launched {
                        w.launched = st.attacks_launched;
                        let value = st.groups.last().map_or(0, |g| g.launch_value);
                        w.launches.push((st.attacks_launched, value, now));
                    }
                    for command in wanted.drain(..) {
                        world.schedule(world.tick(), ai.player(), command).unwrap();
                    }
                }
            }
            if let Some(c) = clash.as_mut()
                && world.tick().0 <= c.tick + WINDOW
            {
                note_armed(&world, c);
            }
            let report = world.step();
            if trace_out.is_some() {
                for (seat, w) in watch.iter().enumerate() {
                    if first_launch[seat].is_none() {
                        first_launch[seat] = w.launches.first().map(|l| l.2);
                    }
                }
                match clash.as_mut() {
                    None => {
                        let gathers = [0, 1].map(|s| {
                            ais[s]
                                .state()
                                .gather
                                .map(|g| (i64::from(g.x), i64::from(g.y)))
                        });
                        let strikers = [0, 1].map(|s| {
                            ais[s]
                                .state()
                                .groups
                                .iter()
                                .flat_map(|g| g.members.iter().copied())
                                .collect::<BTreeSet<EntityId>>()
                        });
                        clash = find_clash(&world, gathers, &strikers);
                        if let Some(c) = clash.as_mut() {
                            note_armed(&world, c);
                            count_shots(&world, c);
                            c.shot_at = shot_at(&world, &ais, 1 - c.shooter);
                        }
                    }
                    Some(c) if world.tick().0 <= c.tick + WINDOW => {
                        count_shots(&world, c);
                        if world.tick().0 == c.tick + WINDOW {
                            c.left = Some([
                                wave_value(&world, 0, &c.waves[0]),
                                wave_value(&world, 1, &c.waves[1]),
                            ]);
                        }
                    }
                    Some(_) => {}
                }
            }
            if report.deaths > 0 {
                last_loss = world.tick().0;
            }
            if let Some(result) = world.match_result() {
                decided = true;
                winner = match result.verdict {
                    // The seats are players 0 and 1 of the match file, ascending.
                    Verdict::Won(player) => Some(usize::from(player.0)),
                    Verdict::Draw => None,
                };
                break;
            }
        }
        let ending = classify(
            decided.then_some(winner),
            world.tick().0,
            last_loss,
            STALEMATE_WINDOW,
        );
        let ending_text = match ending {
            Ending::Won(seat) => format!("won{seat}"),
            Ending::MutualDefeat => "mutual".to_string(),
            Ending::Stalemate => "stalemate".to_string(),
            Ending::Capped => "capped".to_string(),
        };
        let launches = |w: &Watch| {
            w.launches
                .iter()
                .map(|(n, v, t)| format!("{n}:{v}:{t}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        writeln!(
            out,
            "{index} {} {ending_text} {} {} {} {} {} {} | {} | {}",
            u8::from(swapped),
            ais[0].state().attacks_launched,
            ais[1].state().attacks_launched,
            tick_text(watch[0].harv_lost),
            tick_text(watch[0].struct_hit),
            tick_text(watch[1].harv_lost),
            tick_text(watch[1].struct_hit),
            launches(&watch[0]),
            launches(&watch[1]),
        )
        .unwrap();
        out.flush().unwrap();
        if let Some(t) = trace_out.as_mut() {
            writeln!(
                t,
                "{}",
                trace_line(
                    index,
                    swapped,
                    first_launch,
                    &world,
                    clash.as_ref(),
                    [watch[0].recalled_early, watch[1].recalled_early]
                )
            )
            .unwrap();
            t.flush().unwrap();
        }
        let _ = Tick(0);
    }
}
