//! The AI tournament (WP9.5a, `docs/rules/ai.md` "AI against AI"): `sf_headless tournament <match.ron>
//! [--games N] [--cap TICKS] [--seed S] [--json PATH] [--assets DIR] [--stalemate TICKS]`.
//!
//! It plays one match file, in which every seat is a computer opponent (`assets/matches/m1_duel.ron`), many
//! times and says who wins how often. The world is the one the game makes (`scenario::game`, the shipped
//! assets under `--assets`, which is the directory two levels above the match file when left out), the AIs are the
//! ones the game makes (`sf_ai::match_ais`).
//!
//! # What varies from game to game, on purpose
//!
//! * **The seats** ([`factions_for`]): game `i` puts the factions of the file on the start sites in one of
//!   four ways, cycling: as written, swapped, the first faction on both sites, the second on both sites
//!   (the mirrors). Game 0 is the file as written.
//! * **The world seed** and **the AI seed** ([`seeds_for`]): both come from `--seed` (the seed of the match
//!   file when left out) and the game number through a fixed integer mixer, so a game number always has the
//!   same seeds for the same `--seed`, and no two games share them. The AI seed feeds the random stream of
//!   each AI (`AI_STREAM` plus the player number); no rule draws from it yet, so today it only shows in the
//!   digest. How much the world seed alone separates the games is what the count of distinct final
//!   checksums measures.
//!
//! # When a game ends
//!
//! At the first tick after which the match has a result (`World::match_result`), or when `--cap` ticks
//! (default [`DEFAULT_CAP`]) have run. A game that reached the cap has no winner; it is a draw and is
//! labelled ([`Ending`]): **stalemate** when no unit or structure was lost in its last `--stalemate` ticks
//! (default [`STALEMATE_WINDOW`]; a game shorter than the window cannot be one), otherwise **capped**. A game
//! that both players lose in the same tick is a **mutual defeat**, also a draw.
//!
//! # The numbers
//!
//! Everything that is printed and written apart from the `host` block of the JSON is a pure function of the
//! arguments and the assets: whole numbers only (win rates in permille, printed with one decimal), no clock.
//! The wall time of a game goes to stderr and into the `host` block, never into the table. A faction's win
//! rate counts only the games in which the two sides are different factions (a mirror says nothing about
//! the factions); the win rate of a seat counts every game; durations are those of decided games.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sf_data::match_def::Controller;
use sf_data::units::Faction;
use sf_math::Fx;
use sf_sim::{PlayerId, Verdict};

use crate::cli::{EXIT_INVALID, EXIT_OK};
use crate::log::{self, Host};
use crate::scenario::game::{self, LoadedGame};

/// Games played when `--games` is not given.
pub const DEFAULT_GAMES: u32 = 8;
/// The most games one run may ask for.
pub const MAX_GAMES: u32 = 100_000;
/// Ticks after which a game that is still going is stopped and called a draw (`--cap`).
pub const DEFAULT_CAP: u64 = 60_000;
/// Ticks without a loss that make a capped game a stalemate (`--stalemate`).
pub const STALEMATE_WINDOW: u64 = 10_000;
/// The world's walking speed as raw fixed point: the game's own (`sf_game::bench_scene::DEMO_SPEED`, 1.5, the
/// number a recorded game carries as `speed_raw`). An engine constant: it is part of the checksum, and a
/// tournament that plays another speed than the game plays another game.
pub const GAME_SPEED_RAW: i64 = 98_304;
/// The `format` of the JSON.
pub const FORMAT: u32 = 1;

/// The help text of the subcommand.
pub const USAGE: &str = "\
Usage: sf_headless tournament <match.ron> [options]

Plays a match file whose seats are all computer opponents many times, varying the seats, the world seed
and the AI seed from game to game, and prints who wins how often. The match file is read with the
shipped assets (the directory above it, or --assets).

Options:
  --games N        number of games (default 8)
  --cap TICKS      stop a game after TICKS ticks and call it a draw (default 60000)
  --seed S         base seed, decimal or 0x hex (default: the seed of the match file)
  --json PATH      write the games and the summary as JSON to PATH
  --assets DIR     the asset directory (default: two levels above the match file)
  --stalemate N    a capped game with no unit lost in its last N ticks is a stalemate (default 10000)
  -h, --help       show this text

Exit status: 0 success, 2 usage error, unreadable or unusable match, or unwritable JSON.
";

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// The match file.
    pub match_file: PathBuf,
    /// `--games`.
    pub games: u32,
    /// `--cap`.
    pub cap: u64,
    /// `--seed`; the match file's own seed when `None`.
    pub seed: Option<u64>,
    /// `--json`.
    pub json: Option<PathBuf>,
    /// `--assets`; derived from the match file's path when `None`.
    pub assets: Option<PathBuf>,
    /// `--stalemate`.
    pub stalemate: u64,
}

/// The result of parsing the arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Run the tournament.
    Run(Args),
    /// Show the help text.
    Help,
}

fn parse_seed(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let cleaned: String = t.chars().filter(|&c| c != '_').collect();
    let parsed = match cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => cleaned.parse::<u64>(),
    };
    parsed.map_err(|_| format!("--seed needs a whole number (decimal or 0x hex), got `{t}`"))
}

fn parse_count(flag: &str, value: &str, max: u64) -> Result<u64, String> {
    match value.parse::<u64>() {
        Ok(n) if (1..=max).contains(&n) => Ok(n),
        Ok(_) => Err(format!("{flag} must be between 1 and {max}")),
        Err(_) => Err(format!("{flag} needs a whole number, got `{value}`")),
    }
}

/// Parses the arguments after `tournament`. Flags take their value as the next argument or after `=`; a
/// flag given twice, an unknown flag, a missing match file or a second one is an error.
pub fn parse_args<I>(args: I) -> Result<Parsed, String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let mut match_file: Option<PathBuf> = None;
    let mut games = None;
    let mut cap = None;
    let mut seed = None;
    let mut json = None;
    let mut assets = None;
    let mut stalemate = None;
    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Ok(Parsed::Help);
        }
        if let Some(flag_text) = arg.strip_prefix("--") {
            let (flag, inline) = match flag_text.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (flag_text, None),
            };
            if !matches!(
                flag,
                "games" | "cap" | "seed" | "json" | "assets" | "stalemate"
            ) {
                return Err(format!("unknown option `--{flag}`"));
            }
            let value = match inline {
                Some(v) => v,
                None => iter
                    .next()
                    .ok_or_else(|| format!("--{flag} needs a value"))?,
            };
            let name = format!("--{flag}");
            let twice = || format!("{name} given twice");
            let fresh = match flag {
                "games" => games
                    .replace(parse_count(&name, &value, u64::from(MAX_GAMES))?)
                    .is_none(),
                "cap" => cap
                    .replace(parse_count(&name, &value, u64::from(u32::MAX))?)
                    .is_none(),
                "stalemate" => stalemate
                    .replace(parse_count(&name, &value, u64::from(u32::MAX))?)
                    .is_none(),
                "seed" => seed.replace(parse_seed(&value)?).is_none(),
                "json" => json.replace(PathBuf::from(value)).is_none(),
                _ => assets.replace(PathBuf::from(value)).is_none(),
            };
            if !fresh {
                return Err(twice());
            }
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option `{arg}`"));
        } else if match_file.replace(PathBuf::from(arg)).is_some() {
            return Err("more than one match file given".to_string());
        }
    }
    let match_file = match_file.ok_or_else(|| "no match file given".to_string())?;
    Ok(Parsed::Run(Args {
        match_file,
        // The bounds were checked above, so the conversions cannot fail.
        games: games.map_or(DEFAULT_GAMES, |g| u32::try_from(g).unwrap_or(DEFAULT_GAMES)),
        cap: cap.unwrap_or(DEFAULT_CAP),
        seed,
        json,
        assets,
        stalemate: stalemate.unwrap_or(STALEMATE_WINDOW),
    }))
}

// ---------------------------------------------------------------------------------------------------
// The schedule: what a game number means.
// ---------------------------------------------------------------------------------------------------

/// The factions on the start sites in game `index`, for a file whose two seats have `file` (seat 0, seat 1):
/// the games cycle through as written, swapped, the first faction on both sites, the second on both.
pub fn factions_for(file: [Faction; 2], index: u32) -> [Faction; 2] {
    match index % 4 {
        0 => [file[0], file[1]],
        1 => [file[1], file[0]],
        2 => [file[0], file[0]],
        _ => [file[1], file[1]],
    }
}

/// One step of the SplitMix64 generator: a fixed integer mixer with no state of its own.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The world seed and the AI seed of game `index` under the base seed `base`. Different for every game
/// number and for the two roles; the same on every machine.
pub fn seeds_for(base: u64, index: u32) -> (u64, u64) {
    let game = mix(base ^ mix(u64::from(index)));
    (mix(game ^ 1), mix(game ^ 2))
}

// ---------------------------------------------------------------------------------------------------
// One game.
// ---------------------------------------------------------------------------------------------------

/// How a game ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The seat (0 or 1, the order of the match file) won.
    Won(usize),
    /// Both players lost their last construction centre in the same tick: nobody won.
    MutualDefeat,
    /// The cap was reached and nothing was lost in the last window of ticks.
    Stalemate,
    /// The cap was reached while units were still being lost (or the game was shorter than the window).
    Capped,
}

impl Ending {
    /// The word in the table and the JSON.
    pub fn label(self) -> &'static str {
        match self {
            Ending::Won(_) => "won",
            Ending::MutualDefeat => "mutual defeat",
            Ending::Stalemate => "stalemate",
            Ending::Capped => "capped",
        }
    }

    /// Whether the match was decided (a winner, or both defeated at once), not stopped by the cap.
    pub fn decided(self) -> bool {
        matches!(self, Ending::Won(_) | Ending::MutualDefeat)
    }
}

/// How a game that ran `ticks` ticks ended. `decided` is `Some(Some(seat))` for a winner, `Some(None)` for
/// both defeated at once and `None` when the match had no result; `last_loss` is the last tick in which
/// something was destroyed (0: nothing was).
pub fn classify(decided: Option<Option<usize>>, ticks: u64, last_loss: u64, window: u64) -> Ending {
    match decided {
        Some(Some(seat)) => Ending::Won(seat),
        Some(None) => Ending::MutualDefeat,
        None if ticks.saturating_sub(last_loss) >= window => Ending::Stalemate,
        None => Ending::Capped,
    }
}

/// What one AI did in a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiRecord {
    /// The player it played.
    pub player: u8,
    /// `Ai::digest` at the end.
    pub digest: u64,
    /// `AiState::attacks_launched`.
    pub attacks: u32,
}

/// Everything deterministic about one game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRecord {
    /// The game number, from 0.
    pub index: u32,
    /// The seed of the world.
    pub world_seed: u64,
    /// The seed of the AIs.
    pub ai_seed: u64,
    /// The faction on each seat.
    pub factions: [Faction; 2],
    /// How it ended.
    pub ending: Ending,
    /// The tick the match was decided in (the tick of the last defeat); `None` for a draw by the cap.
    pub defeat_tick: Option<u64>,
    /// Ticks played.
    pub ticks: u64,
    /// The last tick in which something was destroyed; 0 when nothing was.
    pub last_loss: u64,
    /// The world's checksum at the end.
    pub final_checksum: u64,
    /// One entry per AI, ascending by player.
    pub ais: Vec<AiRecord>,
}

/// A match file read and checked, ready to be played many times.
pub struct Setup {
    name: String,
    loaded: LoadedGame,
    cells: (u32, u32),
    /// The text of each profile the seats use, by name: read once, so no file is read while a game runs.
    profiles: BTreeMap<String, String>,
    /// The seats, ascending by player id.
    seats: [PlayerId; 2],
    /// The faction of each seat in the file.
    file_factions: [Faction; 2],
}

impl Setup {
    /// The name of the match file.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The faction of each seat as the file writes it.
    pub fn file_factions(&self) -> [Faction; 2] {
        self.file_factions
    }
}

/// Reads the match at `match_path` with the assets under `assets`: exactly two players, every one a
/// computer opponent whose profile is read from `<assets>/ai/<profile>.ron`.
pub fn load_setup(assets: &Path, match_path: &Path) -> Result<Setup, String> {
    let (loaded, cells) = game::load_match_file(assets, match_path)?;
    let def = &loaded.def;
    if def.players.len() != 2 {
        return Err(format!(
            "{}: a tournament plays two seats, the match has {}",
            match_path.display(),
            def.players.len()
        ));
    }
    let mut profiles = BTreeMap::new();
    for p in &def.players {
        let Controller::Ai { profile } = &p.controller else {
            return Err(format!(
                "{}: player {} is not a computer opponent (controller: Ai(profile: ..)); a tournament has no human",
                match_path.display(),
                p.id
            ));
        };
        if !profiles.contains_key(profile) {
            let file = assets.join("ai").join(format!("{profile}.ron"));
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            profiles.insert(profile.clone(), text);
        }
    }
    let mut players: Vec<_> = def.players.iter().collect();
    players.sort_by_key(|p| p.id);
    Ok(Setup {
        name: def.name.clone(),
        seats: [PlayerId(players[0].id), PlayerId(players[1].id)],
        file_factions: [players[0].faction, players[1].faction],
        profiles,
        cells,
        loaded,
    })
}

/// The faction of each seat in `loaded`, by the player id of the seat.
fn seat_factions(loaded: &LoadedGame, seats: &[PlayerId; 2]) -> [Faction; 2] {
    let of = |seat: PlayerId| {
        loaded
            .def
            .players
            .iter()
            .find(|p| PlayerId(p.id) == seat)
            .map_or(Faction::Neutral, |p| p.faction)
    };
    [of(seats[0]), of(seats[1])]
}

/// Plays game `index`: the factions, the seeds, the world and the AIs of [`factions_for`] and [`seeds_for`],
/// the AIs thinking right before the step they are due at (as `run::run_full` and the game's sim thread do),
/// until the match has a result or `cap` ticks have run.
pub fn play_game(
    setup: &Setup,
    base_seed: u64,
    index: u32,
    cap: u64,
    stalemate_window: u64,
) -> Result<GameRecord, String> {
    let factions = factions_for(setup.file_factions, index);
    let (world_seed, ai_seed) = seeds_for(base_seed, index);
    // The loaded match lists its players in file order, which need not be ascending by id: the factions are
    // by seat (ascending id), so they are matched to the players by id.
    let by_player: Vec<Faction> = setup
        .loaded
        .def
        .players
        .iter()
        .map(|p| {
            if PlayerId(p.id) == setup.seats[0] {
                factions[0]
            } else {
                factions[1]
            }
        })
        .collect();
    let loaded = setup.loaded.with_factions(&by_player, setup.cells)?;
    // Boxed like the world of `run::run_full`: a `World` is large and Windows gives a thread 1 to 2 MB of stack.
    let mut world = Box::new(
        game::build_world_at_speed(
            world_seed,
            setup.cells,
            Fx::from_raw(GAME_SPEED_RAW),
            &loaded,
        )
        .map_err(|e| e.to_string())?,
    );
    let mut ais = sf_ai::match_ais(&loaded.def, &loaded.defs, ai_seed, &|name| {
        setup
            .profiles
            .get(name)
            .cloned()
            .ok_or_else(|| format!("profile {name} was not read"))
    })?;
    let mut wanted = Vec::new();
    let mut last_loss = 0u64;
    let mut decided = None;
    while world.tick().0 < cap {
        for ai in &mut ais {
            if ai.due(world.tick()) {
                wanted.clear();
                ai.think(&world, &mut wanted);
                for command in wanted.drain(..) {
                    world
                        .schedule(world.tick(), ai.player(), command)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        let report = world.step();
        if report.deaths > 0 {
            last_loss = report.tick.0;
        }
        if let Some(result) = world.match_result() {
            let winner = match result.verdict {
                Verdict::Won(player) => Some(
                    setup
                        .seats
                        .iter()
                        .position(|s| *s == player)
                        .ok_or_else(|| format!("the winner, {player:?}, is not a seat"))?,
                ),
                Verdict::Draw => None,
            };
            decided = Some((result.tick.0, winner));
            break;
        }
    }
    let ticks = world.tick().0;
    let ending = classify(
        decided.map(|(_, winner)| winner),
        ticks,
        last_loss,
        stalemate_window,
    );
    Ok(GameRecord {
        index,
        world_seed,
        ai_seed,
        // What the world and the AIs were made with, read back from the match they were made from: the
        // record does not repeat the schedule, it shows what was played.
        factions: seat_factions(&loaded, &setup.seats),
        ending,
        defeat_tick: decided.map(|(tick, _)| tick),
        ticks,
        last_loss,
        final_checksum: world.checksum(),
        ais: ais
            .iter()
            .map(|a| AiRecord {
                player: a.player().0,
                digest: a.digest(),
                attacks: a.state().attacks_launched,
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------------------------------
// The summary.
// ---------------------------------------------------------------------------------------------------

/// `part` of `whole` in permille, rounded down; 0 for an empty whole.
pub fn permille(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        0
    } else {
        // part <= whole <= u32::MAX, so the product fits in u64 and the quotient in u32.
        u32::try_from(u64::from(part) * 1000 / u64::from(whole)).unwrap_or(u32::MAX)
    }
}

/// A permille as printed: `523` is `52.3%`.
pub fn percent_text(permille: u32) -> String {
    format!("{}.{}%", permille / 10, permille % 10)
}

/// The median of `values`: the middle one, or for an even count the whole-number average (rounded down) of
/// the two in the middle; `None` for no values.
pub fn median(values: &[u64]) -> Option<u64> {
    let mut v = values.to_vec();
    v.sort_unstable();
    match v.len() {
        0 => None,
        n if n % 2 == 1 => Some(v[n / 2]),
        n => Some((v[n / 2 - 1] + v[n / 2]) / 2),
    }
}

/// Wins of one faction, over the games in which the two sides were different factions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactionRow {
    /// The faction's name.
    pub faction: String,
    /// Games it played against the other faction.
    pub games: u32,
    /// Of those, games it won.
    pub wins: u32,
}

/// Minimum, median and maximum duration of the decided games.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Durations {
    /// The shortest decided game, in ticks.
    pub min: u64,
    /// The median.
    pub median: u64,
    /// The longest.
    pub max: u64,
}

/// What a set of games adds up to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Games played.
    pub games: u32,
    /// Games each seat won (seat 0, seat 1).
    pub seat_wins: [u32; 2],
    /// Games both players lost in the same tick.
    pub mutual: u32,
    /// Games stopped by the cap that were stalemates.
    pub stalemates: u32,
    /// Games stopped by the cap that were not.
    pub capped: u32,
    /// Per faction, ascending by name, over the games between different factions.
    pub factions: Vec<FactionRow>,
    /// Of the decided games; `None` when none was decided.
    pub durations: Option<Durations>,
    /// How many different final checksums the games ended with.
    pub distinct_checksums: u32,
}

impl Summary {
    /// Draws: mutual defeats, stalemates and capped games.
    pub fn draws(&self) -> u32 {
        self.mutual + self.stalemates + self.capped
    }
}

/// Adds the games up.
pub fn summarize(records: &[GameRecord]) -> Summary {
    let mut seat_wins = [0u32; 2];
    let (mut mutual, mut stalemates, mut capped) = (0, 0, 0);
    let mut factions: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut durations = Vec::new();
    let mut checksums = BTreeSet::new();
    for r in records {
        checksums.insert(r.final_checksum);
        match r.ending {
            Ending::Won(seat) => seat_wins[seat] += 1,
            Ending::MutualDefeat => mutual += 1,
            Ending::Stalemate => stalemates += 1,
            Ending::Capped => capped += 1,
        }
        if r.ending.decided() {
            durations.push(r.ticks);
        }
        if r.factions[0] != r.factions[1] {
            for (seat, faction) in r.factions.iter().enumerate() {
                let row = factions.entry(format!("{faction:?}")).or_default();
                row.0 += 1;
                if r.ending == Ending::Won(seat) {
                    row.1 += 1;
                }
            }
        }
    }
    Summary {
        games: u32::try_from(records.len()).unwrap_or(u32::MAX),
        seat_wins,
        mutual,
        stalemates,
        capped,
        factions: factions
            .into_iter()
            .map(|(faction, (games, wins))| FactionRow {
                faction,
                games,
                wins,
            })
            .collect(),
        durations: match (
            durations.iter().min(),
            median(&durations),
            durations.iter().max(),
        ) {
            (Some(&min), Some(median), Some(&max)) => Some(Durations { min, median, max }),
            _ => None,
        },
        distinct_checksums: u32::try_from(checksums.len()).unwrap_or(u32::MAX),
    }
}

// ---------------------------------------------------------------------------------------------------
// Output.
// ---------------------------------------------------------------------------------------------------

fn faction_names(r: &GameRecord) -> String {
    format!("{:?} v {:?}", r.factions[0], r.factions[1])
}

/// A finished tournament: what was asked and what happened, for [`render_table`] and [`render_json`].
#[derive(Clone, Debug)]
pub struct Report {
    /// The name of the match.
    pub name: String,
    /// The base seed the games' seeds come from.
    pub base_seed: u64,
    /// The cap in ticks.
    pub cap: u64,
    /// The stalemate window in ticks.
    pub stalemate_window: u64,
    /// The games, in order.
    pub records: Vec<GameRecord>,
    /// [`summarize`] of the games.
    pub summary: Summary,
}

impl Report {
    /// Builds the report of `records` (summarising them).
    pub fn new(
        name: &str,
        base_seed: u64,
        cap: u64,
        stalemate_window: u64,
        records: Vec<GameRecord>,
    ) -> Report {
        Report {
            name: name.to_string(),
            base_seed,
            cap,
            stalemate_window,
            summary: summarize(&records),
            records,
        }
    }
}

/// The text table: one line per game, then the summary. Deterministic: no wall time.
pub fn render_table(report: &Report) -> String {
    let mut s = String::new();
    // Writing to a String cannot fail.
    let _ = write_table(&mut s, report);
    s
}

fn write_table(s: &mut String, report: &Report) -> std::fmt::Result {
    let Report {
        name,
        base_seed,
        cap,
        records,
        summary: sum,
        ..
    } = report;
    writeln!(
        s,
        "tournament: {name}, {} games, seed {}, cap {cap} ticks",
        sum.games,
        log::hex(*base_seed)
    )?;
    writeln!(
        s,
        "game  seats (seat 0 v seat 1)   result         ticks  defeat  attacks  final checksum"
    )?;
    for r in records {
        let result = match r.ending {
            Ending::Won(seat) => format!("seat {seat} won"),
            other => other.label().to_string(),
        };
        let attacks: Vec<String> = r.ais.iter().map(|a| a.attacks.to_string()).collect();
        writeln!(
            s,
            "{:>4}  {:<24}  {:<13} {:>6}  {:>6}  {:>7}  {}",
            r.index,
            faction_names(r),
            result,
            r.ticks,
            r.defeat_tick.map_or("-".to_string(), |t| t.to_string()),
            attacks.join("/"),
            log::hex(r.final_checksum)
        )?;
    }
    writeln!(s)?;
    for (seat, wins) in sum.seat_wins.iter().enumerate() {
        writeln!(
            s,
            "seat {seat} wins:    {wins} of {} ({})",
            sum.games,
            percent_text(permille(*wins, sum.games))
        )?;
    }
    for row in &sum.factions {
        writeln!(
            s,
            "{} wins: {} of {} games against the other faction ({})",
            row.faction,
            row.wins,
            row.games,
            percent_text(permille(row.wins, row.games))
        )?;
    }
    writeln!(
        s,
        "draws:         {} ({} mutual defeat, {} stalemate, {} capped)",
        sum.draws(),
        sum.mutual,
        sum.stalemates,
        sum.capped
    )?;
    match sum.durations {
        Some(d) => writeln!(
            s,
            "duration:      min {} / median {} / max {} ticks (decided games)",
            d.min, d.median, d.max
        )?,
        None => writeln!(s, "duration:      no game was decided")?,
    }
    writeln!(
        s,
        "distinct final checksums: {} of {} games",
        sum.distinct_checksums, sum.games
    )
}

/// The JSON: `format`, the arguments, one object per game, the summary, and `host` last (CI-style
/// `jq 'del(.host)'` leaves the deterministic part). Written by hand, like `log::render`.
pub fn render_json(report: &Report, host: &Host, wall_micros: &[u128]) -> String {
    let mut s = String::new();
    // Writing to a String cannot fail.
    let _ = write_json(&mut s, report, host, wall_micros);
    s
}

fn write_json(
    s: &mut String,
    report: &Report,
    host: &Host,
    wall_micros: &[u128],
) -> std::fmt::Result {
    let Report {
        name,
        base_seed,
        cap,
        stalemate_window: window,
        records,
        summary: sum,
    } = report;
    writeln!(s, "{{")?;
    writeln!(s, "  \"format\": {FORMAT},")?;
    writeln!(s, "  \"kind\": \"tournament\",")?;
    writeln!(s, "  \"match\": {},", log::json_string(name))?;
    writeln!(s, "  \"seed\": \"{}\",", log::hex(*base_seed))?;
    writeln!(s, "  \"cap\": {cap},")?;
    writeln!(s, "  \"stalemate_window\": {window},")?;
    writeln!(s, "  \"games\": [")?;
    for (i, r) in records.iter().enumerate() {
        let comma = if i + 1 < records.len() { "," } else { "" };
        let winner = match r.ending {
            Ending::Won(seat) => seat.to_string(),
            _ => "null".to_string(),
        };
        let ais: Vec<String> = r
            .ais
            .iter()
            .map(|a| {
                format!(
                    "{{\"player\": {}, \"digest\": \"{}\", \"attacks\": {}}}",
                    a.player,
                    log::hex(a.digest),
                    a.attacks
                )
            })
            .collect();
        writeln!(
            s,
            "    {{\"index\": {}, \"world_seed\": \"{}\", \"ai_seed\": \"{}\", \"seats\": [\"{:?}\", \"{:?}\"], \
             \"ending\": {}, \"winner_seat\": {winner}, \"defeat_tick\": {}, \"ticks\": {}, \"last_loss\": {}, \
             \"final_checksum\": \"{}\", \"ais\": [{}]}}{comma}",
            r.index,
            log::hex(r.world_seed),
            log::hex(r.ai_seed),
            r.factions[0],
            r.factions[1],
            log::json_string(r.ending.label()),
            r.defeat_tick.map_or("null".to_string(), |t| t.to_string()),
            r.ticks,
            r.last_loss,
            log::hex(r.final_checksum),
            ais.join(", ")
        )?;
    }
    writeln!(s, "  ],")?;
    writeln!(s, "  \"summary\": {{")?;
    writeln!(s, "    \"games\": {},", sum.games)?;
    writeln!(
        s,
        "    \"seat_wins\": [{}, {}],",
        sum.seat_wins[0], sum.seat_wins[1]
    )?;
    writeln!(
        s,
        "    \"seat_win_permille\": [{}, {}],",
        permille(sum.seat_wins[0], sum.games),
        permille(sum.seat_wins[1], sum.games)
    )?;
    let rows: Vec<String> = sum
        .factions
        .iter()
        .map(|f| {
            format!(
                "{{\"faction\": {}, \"games\": {}, \"wins\": {}, \"win_permille\": {}}}",
                log::json_string(&f.faction),
                f.games,
                f.wins,
                permille(f.wins, f.games)
            )
        })
        .collect();
    writeln!(s, "    \"factions\": [{}],", rows.join(", "))?;
    writeln!(s, "    \"mutual_defeats\": {},", sum.mutual)?;
    writeln!(s, "    \"stalemates\": {},", sum.stalemates)?;
    writeln!(s, "    \"capped\": {},", sum.capped)?;
    writeln!(s, "    \"draws\": {},", sum.draws())?;
    match sum.durations {
        Some(d) => writeln!(
            s,
            "    \"duration_ticks\": {{\"min\": {}, \"median\": {}, \"max\": {}}},",
            d.min, d.median, d.max
        )?,
        None => writeln!(s, "    \"duration_ticks\": null,")?,
    }
    writeln!(
        s,
        "    \"distinct_final_checksums\": {}",
        sum.distinct_checksums
    )?;
    writeln!(s, "  }},")?;
    writeln!(s, "  \"host\": {{")?;
    writeln!(s, "    \"os\": {},", log::json_string(host.os))?;
    writeln!(s, "    \"arch\": {},", log::json_string(host.arch))?;
    writeln!(s, "    \"debug_assertions\": {},", host.debug_assertions)?;
    writeln!(s, "    \"elapsed_ms\": {},", host.elapsed_micros / 1_000)?;
    let per_game: Vec<String> = wall_micros
        .iter()
        .map(|m| (m / 1_000).to_string())
        .collect();
    writeln!(s, "    \"game_elapsed_ms\": [{}]", per_game.join(", "))?;
    writeln!(s, "  }}")?;
    writeln!(s, "}}")
}

/// Plays every game of `args` on `setup` and returns the records in game order. `progress` is told after
/// each game (its number, its record and its wall time in microseconds); it is how the command line shows
/// that a long run is alive, and it cannot change a game.
pub fn run_games(
    setup: &Setup,
    args: &Args,
    mut progress: impl FnMut(&GameRecord, u128),
) -> Result<(Vec<GameRecord>, Vec<u128>), String> {
    let base = args.seed.unwrap_or(setup.loaded.def.seed);
    let mut records = Vec::new();
    let mut wall = Vec::new();
    for index in 0..args.games {
        // The clock only brackets a game; it is printed, never used.
        let started = Instant::now();
        let record = play_game(setup, base, index, args.cap, args.stalemate)
            .map_err(|e| format!("game {index}: {e}"))?;
        let micros = started.elapsed().as_micros();
        progress(&record, micros);
        records.push(record);
        wall.push(micros);
    }
    Ok((records, wall))
}

/// The asset directory for a match file: `--assets` if given, else the directory two levels above the file
/// (`assets/matches/m1_duel.ron` is under `assets`).
pub fn assets_for(args: &Args) -> Result<PathBuf, String> {
    if let Some(dir) = &args.assets {
        return Ok(dir.clone());
    }
    args.match_file
        .parent()
        .and_then(Path::parent)
        .map(|p| {
            if p.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                p.to_path_buf()
            }
        })
        .ok_or_else(|| {
            format!(
                "cannot tell the asset directory of {}: give --assets",
                args.match_file.display()
            )
        })
}

/// Runs the subcommand with the arguments after `tournament` and returns the exit code. The table goes to
/// `out`; the progress, the wall times and the problems go to `err`.
pub fn run_cli(args: Vec<String>, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let args = match parse_args(args) {
        Ok(Parsed::Help) => {
            let _ = out.write_all(USAGE.as_bytes());
            return EXIT_OK;
        }
        Ok(Parsed::Run(args)) => args,
        Err(message) => {
            let _ = writeln!(err, "sf_headless tournament: {message}\n\n{USAGE}");
            return EXIT_INVALID;
        }
    };
    let fail = |err: &mut dyn Write, message: String| {
        let _ = writeln!(err, "sf_headless tournament: {message}");
        EXIT_INVALID
    };
    let assets = match assets_for(&args) {
        Ok(a) => a,
        Err(e) => return fail(err, e),
    };
    let setup = match load_setup(&assets, &args.match_file) {
        Ok(s) => s,
        Err(e) => return fail(err, e),
    };
    let total = args.games;
    let started = Instant::now();
    let played = run_games(&setup, &args, |r, micros| {
        let _ = writeln!(
            err,
            "game {}/{total}: {} {}, {} ticks, {} ms",
            r.index + 1,
            faction_names(r),
            r.ending.label(),
            r.ticks,
            micros / 1_000
        );
    });
    let (records, wall) = match played {
        Ok(p) => p,
        Err(e) => return fail(err, e),
    };
    let host = Host::current(started.elapsed());
    let base = args.seed.unwrap_or(setup.loaded.def.seed);
    let report = Report::new(setup.name(), base, args.cap, args.stalemate, records);
    let _ = out.write_all(render_table(&report).as_bytes());
    let _ = writeln!(
        err,
        "wall time: {} ms for {} games",
        host.elapsed_micros / 1_000,
        report.records.len()
    );
    if let Some(path) = &args.json {
        let text = render_json(&report, &host, &wall);
        let written = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir),
            _ => Ok(()),
        }
        .and_then(|()| std::fs::write(path, text));
        if let Err(e) = written {
            return fail(err, format!("cannot write {}: {e}", path.display()));
        }
    }
    EXIT_OK
}

#[cfg(test)]
mod tests;
