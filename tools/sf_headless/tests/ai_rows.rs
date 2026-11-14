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

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use sf_ai::{Lens, Observation};
use sf_headless::tournament::{
    Ending, STALEMATE_WINDOW, classify, factions_for, load_setup, prepare_sited, seeds_for,
    sites_swapped,
};
use sf_sim::{EntityId, Tick, Verdict};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
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
    for index in 0..games {
        let factions = factions_for(setup.file_factions, index);
        let swapped = sites_swapped(index);
        let (world_seed, ai_seed) = seeds_for(seed, index);
        let p = prepare_sited(&setup, factions, swapped, world_seed, ai_seed).unwrap();
        let (mut world, mut ais) = (p.world, p.ais);
        let mut watch: Vec<Watch> = ais.iter().map(|_| Watch::default()).collect();
        let mut wanted = Vec::new();
        let mut last_loss = 0u64;
        let mut winner: Option<usize> = None;
        let mut decided = false;
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
                        let hit = w.structures.iter().any(|(id, hp)| match structures.get(id) {
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
            let report = world.step();
            if report.deaths > 0 {
                last_loss = world.tick().0;
            }
            if let Some(result) = world.match_result() {
                decided = true;
                winner = match result.verdict {
                    Verdict::Won(player) => setup.seats.iter().position(|s| *s == player),
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
        let _ = Tick(0);
    }
}
