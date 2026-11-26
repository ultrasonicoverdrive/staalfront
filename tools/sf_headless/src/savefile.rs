//! The save file (WP13.1e-1, `docs/rules/save.md`, "The save file"): one container around the world's save,
//! the save of every AI and the command log, so a game can be stopped and loaded again, and replayed.
//!
//! The world (`World::save`, `sf_sim`) and each AI (`Ai::save`, `sf_ai`) save themselves; this module only
//! puts their bytes in one file with what the host needs to know before it builds anything (which match, which
//! AI level, which formats), the commands handed to the world so far, and a hash over the whole. It lives in the
//! `sf_headless` library because `sf_game` already depends on it (like the record format): one definition for
//! the headless runner and the game.
//!
//! Layout, little-endian ([`ByteWriter`] sections: a tag byte, a `u32` length, the body):
//!
//! | Part | Content |
//! |---|---|
//! | magic | `SFSG` ([`MAGIC`]) |
//! | version | `u32` [`CONTAINER_VERSION`] |
//! | section `I` | [`Header`]: kind, name, AI level, seed, cells, speed, local player, the world and AI format versions, the rules digest, the tick, the world checksum |
//! | section `W` | the bytes of `World::save`, whole |
//! | section `A` | `u8` count, then per AI ascending by player: `u8` player, the bytes of `Ai::save` |
//! | section `L` | `u32` count, then per command: `u64` arrived, `u64` effective, `u8` player, `sf_sim::save::write_command` |
//! | section `H` | `u32` host version ([`HOST_VERSION`]), then a byte run: host state, empty in version 1 |
//! | trailer | `u64` FNV-1a 64 over every byte before it |
//!
//! Reading ([`read_header`], then [`load`]) is all-or-nothing: nothing is handed back unless every part passed.

use std::fmt;

use sf_ai::Ai;
use sf_math::bytes::{ByteError, ByteReader, ByteWriter};
use sf_math::hash::Fnv1a64;
use sf_sim::save::{FORMAT_VERSION, SaveError, read_command, write_command};
use sf_sim::{Command, PlayerId, Tick, World};

use crate::scenario::Scenario;

/// The first four bytes of a save file. Not the world's own `SFSV`: a bare world save is not a save file.
pub const MAGIC: [u8; 4] = *b"SFSG";
/// The version of the container layout this build writes and reads.
pub const CONTAINER_VERSION: u32 = 1;
/// The version of the host section this build writes and reads.
pub const HOST_VERSION: u32 = 1;

/// Section tags.
const IDENTITY: u8 = b'I';
const WORLD: u8 = b'W';
const AIS: u8 = b'A';
const LOG: u8 = b'L';
const HOST: u8 = b'H';

/// Bytes of the shortest possible file: magic, version and trailer.
const MIN_LEN: usize = 4 + 4 + 8;
/// The fewest bytes one logged command takes: two ticks, the player and the command tag.
const MIN_COMMAND: usize = 8 + 8 + 1 + 1;

/// What made the save: the scenario runner or a match of the game.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaveKind {
    /// A scenario of `sf_headless` (also a recorded game run as a scenario).
    Scenario = 0,
    /// A match of `sf_game` (WP13.1e-3).
    Match = 1,
}

/// What the host knows about the game before it builds a world: which game this is. A load is refused when the
/// saved identity is not the one the host would build ([`SaveFileError::OtherMatch`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Who wrote it.
    pub kind: SaveKind,
    /// The scenario's display name, or the match path relative to the assets (`record::match_path_text`).
    pub name: String,
    /// The AI level of a match (`--ai`), or for a scenario the profile file of each AI, ascending by player, joined
    /// by `,`. `None`: no AI.
    pub ai_level: Option<String>,
    /// The world seed.
    pub seed: u64,
    /// The map size in cells.
    pub cells: (u32, u32),
    /// The walking speed as a raw fixed-point number (`WorldConfig::speed`).
    pub speed_raw: i64,
    /// The player the host plays for (0 for a scenario).
    pub local_player: u8,
}

impl Identity {
    /// The identity of a game of `kind` with this name, AI level and local player on `world`: seed, cells and speed
    /// are the world's.
    pub fn of(
        kind: SaveKind,
        name: &str,
        ai_level: Option<String>,
        local_player: u8,
        world: &World,
    ) -> Identity {
        Identity {
            kind,
            name: name.to_string(),
            ai_level,
            seed: world.config().seed,
            cells: world.map_size(),
            speed_raw: world.config().speed.raw(),
            local_player,
        }
    }

    /// The identity of a run of `scenario` on `world` (a world the scenario built or a fresh one of it).
    pub fn of_scenario(scenario: &Scenario, world: &World) -> Identity {
        let level = (!scenario.ai.is_empty()).then(|| {
            let mut specs: Vec<_> = scenario.ai.iter().collect();
            specs.sort_by_key(|a| a.player);
            specs
                .iter()
                .map(|a| a.profile.as_str())
                .collect::<Vec<_>>()
                .join(",")
        });
        Identity::of(
            SaveKind::Scenario,
            scenario.display_name(),
            level,
            0,
            world,
        )
    }
}

/// Section `I`: the identity and the copies that let a host say what is wrong before it builds anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Which game.
    pub identity: Identity,
    /// The world's save format (`sf_sim::save::FORMAT_VERSION` of the build that wrote it).
    pub world_format: u32,
    /// The AI's save format (`sf_ai::AI_FORMAT_VERSION`).
    pub ai_format: u32,
    /// `World::rules_digest` of the saved world.
    pub rules_digest: u64,
    /// The ticks the world had run.
    pub tick: Tick,
    /// `World::checksum` at the save.
    pub checksum: u64,
}

/// A command as it was handed to the world: what WP13.2 replays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedCommand {
    /// Ticks the world had run when the command was handed to it (pending commands are in the checksum).
    pub arrived: Tick,
    /// The tick it runs at.
    pub effective: Tick,
    /// Who sent it.
    pub player: PlayerId,
    /// What it asks.
    pub command: Command,
}

/// Everything a save file holds, restored.
#[derive(Debug)]
pub struct Loaded {
    /// Section `I`.
    pub header: Header,
    /// The world restored onto the fresh world the host gave.
    pub world: World,
    /// The AIs restored onto the fresh AIs the host gave, against the restored world, ascending by player.
    pub ais: Vec<Ai>,
    /// The commands handed to the world up to the save, in the order they were handed.
    pub log: Vec<LoggedCommand>,
    /// The host section's bytes (empty in version 1).
    pub host: Vec<u8>,
}

/// Why a save file could not be written or loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveFileError {
    /// Shorter than the magic, the version and the trailer.
    Truncated,
    /// The trailer hash does not match: cut short, changed, or not a save file.
    Damaged,
    /// The file does not start with [`MAGIC`].
    BadMagic,
    /// A format this build does not read: which (`"save file"`, `"world"`, `"AI"`, `"host"`), the file's and ours.
    Version {
        what: &'static str,
        found: u32,
        reads: u32,
    },
    /// A section is malformed (only a hand-made file with a matching hash gets here); names what.
    Malformed(&'static str),
    /// The save is of another game than the host would build: names the field that differs.
    OtherMatch(&'static str),
    /// The world section was refused by `World::restore`.
    World(SaveError),
    /// The section of this player's AI was refused by `Ai::restore`.
    Ai(PlayerId, SaveError),
    /// The command log is impossible: names how.
    Log(&'static str),
    /// The copies in section `I` do not match the restored world: names which.
    Inconsistent(&'static str),
    /// Bytes after the last section, before the trailer.
    TrailingBytes,
}

impl fmt::Display for SaveFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveFileError::Truncated => write!(f, "the save file is too short"),
            SaveFileError::Damaged => write!(f, "the save file is damaged (its hash does not match)"),
            SaveFileError::BadMagic => write!(f, "not a save file"),
            SaveFileError::Version { what, found, reads } => {
                write!(f, "{what} format {found}, this build reads {reads}")
            }
            SaveFileError::Malformed(what) => write!(f, "the save file is malformed: {what}"),
            SaveFileError::OtherMatch(what) => {
                write!(f, "the save is of another game: other {what}")
            }
            SaveFileError::World(e) => write!(f, "the world in the save: {e}"),
            SaveFileError::Ai(p, e) => write!(f, "the AI of player {} in the save: {e}", p.0),
            SaveFileError::Log(what) => write!(f, "the command log in the save: {what}"),
            SaveFileError::Inconsistent(what) => {
                write!(f, "the save file contradicts itself: {what}")
            }
            SaveFileError::TrailingBytes => write!(f, "the save file has bytes after its sections"),
        }
    }
}

impl std::error::Error for SaveFileError {}

impl From<ByteError> for SaveFileError {
    fn from(e: ByteError) -> SaveFileError {
        SaveFileError::Malformed(match e {
            ByteError::Truncated => "a section ends early",
            ByteError::TooLong => "a length does not fit 32 bits",
            ByteError::BadBool => "a flag is neither 0 nor 1",
            ByteError::BadSection => "a section has the wrong tag or length",
        })
    }
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a64::new();
    h.write_bytes(bytes);
    h.finish()
}

fn put_text(w: &mut ByteWriter, s: &str) -> Result<(), ByteError> {
    w.bytes(s.as_bytes())
}

fn get_text(r: &mut ByteReader<'_>) -> Result<String, SaveFileError> {
    let b = r.bytes()?;
    String::from_utf8(b.to_vec()).map_err(|_| SaveFileError::Malformed("a name is not UTF-8"))
}

/// The save file of `world`, `ais` (ascending by player, at most one per player) and `log` (in the order handed to
/// the world), for the game `identity`. Byte for byte the same on every machine.
pub fn write(
    identity: &Identity,
    world: &World,
    ais: &[Ai],
    log: &[LoggedCommand],
) -> Result<Vec<u8>, SaveFileError> {
    if ais.windows(2).any(|p| p[0].player() >= p[1].player()) {
        return Err(SaveFileError::Malformed("the AIs are not ascending by player"));
    }
    let n_ais = u8::try_from(ais.len()).map_err(|_| SaveFileError::Malformed("too many AIs"))?;
    let world_bytes = world.save().map_err(SaveFileError::World)?;
    let mut ai_bytes = Vec::with_capacity(ais.len());
    for ai in ais {
        ai_bytes.push(ai.save().map_err(|e| SaveFileError::Ai(ai.player(), e))?);
    }
    let mut w = ByteWriter::new();
    w.raw(&MAGIC);
    w.u32(CONTAINER_VERSION);
    w.section(IDENTITY, |s| -> Result<(), SaveFileError> {
        s.u8(identity.kind as u8);
        put_text(s, &identity.name)?;
        s.bool(identity.ai_level.is_some());
        if let Some(level) = &identity.ai_level {
            put_text(s, level)?;
        }
        s.u64(identity.seed);
        s.u32(identity.cells.0);
        s.u32(identity.cells.1);
        s.i64(identity.speed_raw);
        s.u8(identity.local_player);
        s.u32(FORMAT_VERSION);
        s.u32(sf_ai::AI_FORMAT_VERSION);
        s.u64(world.rules_digest());
        s.u64(world.tick().0);
        s.u64(world.checksum());
        Ok(())
    })?;
    w.section(WORLD, |s| -> Result<(), SaveFileError> {
        s.raw(&world_bytes);
        Ok(())
    })?;
    w.section(AIS, |s| -> Result<(), SaveFileError> {
        s.u8(n_ais);
        for (ai, bytes) in ais.iter().zip(&ai_bytes) {
            s.u8(ai.player().0);
            s.bytes(bytes)?;
        }
        Ok(())
    })?;
    w.section(LOG, |s| -> Result<(), SaveFileError> {
        s.usize(log.len())?;
        for c in log {
            s.u64(c.arrived.0);
            s.u64(c.effective.0);
            s.u8(c.player.0);
            write_command(s, &c.command)?;
        }
        Ok(())
    })?;
    w.section(HOST, |s| -> Result<(), SaveFileError> {
        s.u32(HOST_VERSION);
        s.bytes(&[])?;
        Ok(())
    })?;
    let mut out = w.finish();
    let h = hash(&out);
    out.extend_from_slice(&h.to_le_bytes());
    Ok(out)
}

/// The body of the file (between the version and the trailer) once length, hash, magic and version passed.
fn body(bytes: &[u8]) -> Result<ByteReader<'_>, SaveFileError> {
    if bytes.len() < MIN_LEN {
        return Err(SaveFileError::Truncated);
    }
    let (data, trailer) = bytes.split_at(bytes.len() - 8);
    let mut t = [0u8; 8];
    t.copy_from_slice(trailer);
    if hash(data) != u64::from_le_bytes(t) {
        return Err(SaveFileError::Damaged);
    }
    let mut r = ByteReader::new(data);
    if r.take(MAGIC.len())? != MAGIC {
        return Err(SaveFileError::BadMagic);
    }
    let version = r.u32()?;
    if version != CONTAINER_VERSION {
        return Err(SaveFileError::Version {
            what: "save file",
            found: version,
            reads: CONTAINER_VERSION,
        });
    }
    Ok(r)
}

fn identity_section(r: &mut ByteReader<'_>) -> Result<Header, SaveFileError> {
    let mut s = r.section(IDENTITY)?;
    let kind = match s.u8()? {
        0 => SaveKind::Scenario,
        1 => SaveKind::Match,
        _ => return Err(SaveFileError::Malformed("unknown kind")),
    };
    let name = get_text(&mut s)?;
    let ai_level = if s.bool()? {
        Some(get_text(&mut s)?)
    } else {
        None
    };
    let identity = Identity {
        kind,
        name,
        ai_level,
        seed: s.u64()?,
        cells: (s.u32()?, s.u32()?),
        speed_raw: s.i64()?,
        local_player: s.u8()?,
    };
    let header = Header {
        identity,
        world_format: s.u32()?,
        ai_format: s.u32()?,
        rules_digest: s.u64()?,
        tick: Tick(s.u64()?),
        checksum: s.u64()?,
    };
    s.end()?;
    if header.world_format != FORMAT_VERSION {
        return Err(SaveFileError::Version {
            what: "world",
            found: header.world_format,
            reads: FORMAT_VERSION,
        });
    }
    if header.ai_format != sf_ai::AI_FORMAT_VERSION {
        return Err(SaveFileError::Version {
            what: "AI",
            found: header.ai_format,
            reads: sf_ai::AI_FORMAT_VERSION,
        });
    }
    Ok(header)
}

/// Section `I` of a save file, after its length, hash, magic and version are checked and before anything is
/// built: from it the host decides which world and AIs to build for [`load`]. Never panics, whatever the bytes.
pub fn read_header(bytes: &[u8]) -> Result<Header, SaveFileError> {
    let mut r = body(bytes)?;
    identity_section(&mut r)
}

/// The first field in which `saved` is not `expected`.
fn differs(saved: &Identity, expected: &Identity) -> Option<&'static str> {
    if saved.kind != expected.kind {
        Some("kind of game")
    } else if saved.name != expected.name {
        Some("match")
    } else if saved.ai_level != expected.ai_level {
        Some("AI level")
    } else if saved.seed != expected.seed {
        Some("seed")
    } else if saved.cells != expected.cells {
        Some("map size")
    } else if saved.speed_raw != expected.speed_raw {
        Some("speed")
    } else if saved.local_player != expected.local_player {
        Some("local player")
    } else {
        None
    }
}

/// Loads a save file: the world restored onto `fresh` (the tick-0 world the host built for `expected`), then each
/// AI restored onto its fresh AI (`fresh_ais`, ascending by player, as for a new game) against the restored world,
/// then the command log, checked. All-or-nothing; never panics, whatever the bytes.
pub fn load(
    bytes: &[u8],
    expected: &Identity,
    fresh: World,
    fresh_ais: Vec<Ai>,
) -> Result<Loaded, SaveFileError> {
    let mut r = body(bytes)?;
    let header = identity_section(&mut r)?;
    if let Some(what) = differs(&header.identity, expected) {
        return Err(SaveFileError::OtherMatch(what));
    }
    if header.rules_digest != fresh.rules_digest() {
        return Err(SaveFileError::OtherMatch("rules or assets"));
    }

    let mut w = r.section(WORLD)?;
    let world = World::restore(fresh, w.take(w.remaining())?).map_err(SaveFileError::World)?;
    if world.tick() != header.tick {
        return Err(SaveFileError::Inconsistent("the tick"));
    }
    if world.checksum() != header.checksum {
        return Err(SaveFileError::Inconsistent("the checksum"));
    }

    let mut s = r.section(AIS)?;
    let n = usize::from(s.u8()?);
    if n != fresh_ais.len() {
        return Err(SaveFileError::OtherMatch("number of AIs"));
    }
    let mut ais = Vec::with_capacity(n);
    for fresh_ai in fresh_ais {
        let player = PlayerId(s.u8()?);
        if player != fresh_ai.player() {
            return Err(SaveFileError::OtherMatch("AI players"));
        }
        let saved = s.bytes()?;
        ais.push(Ai::restore(fresh_ai, saved, &world).map_err(|e| SaveFileError::Ai(player, e))?);
    }
    s.end()?;

    let mut s = r.section(LOG)?;
    let n = s.count(MIN_COMMAND)?;
    let mut log = Vec::with_capacity(n);
    for _ in 0..n {
        let arrived = Tick(s.u64()?);
        let effective = Tick(s.u64()?);
        let player = PlayerId(s.u8()?);
        let command =
            read_command(&mut s).map_err(|_| SaveFileError::Log("a command is malformed"))?;
        if arrived > effective {
            return Err(SaveFileError::Log("a command runs before it arrived"));
        }
        if arrived > header.tick {
            return Err(SaveFileError::Log("a command arrived after the save"));
        }
        if log.last().is_some_and(|l: &LoggedCommand| l.arrived > arrived) {
            return Err(SaveFileError::Log(
                "the commands are not in the order they arrived",
            ));
        }
        log.push(LoggedCommand {
            arrived,
            effective,
            player,
            command,
        });
    }
    s.end()?;

    let mut s = r.section(HOST)?;
    let version = s.u32()?;
    if version != HOST_VERSION {
        return Err(SaveFileError::Version {
            what: "host",
            found: version,
            reads: HOST_VERSION,
        });
    }
    let host = s.bytes()?.to_vec();
    s.end()?;

    if !r.is_empty() {
        return Err(SaveFileError::TrailingBytes);
    }
    Ok(Loaded {
        header,
        world,
        ais,
        log,
        host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_of_the_layout_are_pinned() {
        assert_eq!(MAGIC, [0x53, 0x46, 0x53, 0x47]);
        assert_eq!(CONTAINER_VERSION, 1);
        assert_eq!(HOST_VERSION, 1);
        assert_eq!(
            [IDENTITY, WORLD, AIS, LOG, HOST],
            [0x49, 0x57, 0x41, 0x4c, 0x48]
        );
        assert_eq!(SaveKind::Scenario as u8, 0);
        assert_eq!(SaveKind::Match as u8, 1);
    }

    #[test]
    fn short_and_strange_bytes_are_refused() {
        assert_eq!(read_header(&[]), Err(SaveFileError::Truncated));
        assert_eq!(read_header(&[0; MIN_LEN - 1]), Err(SaveFileError::Truncated));
        assert_eq!(read_header(&[0; MIN_LEN]), Err(SaveFileError::Damaged));
        // A valid hash over a body without the magic.
        let mut b = b"SFSVxxxx".to_vec();
        let h = hash(&b);
        b.extend_from_slice(&h.to_le_bytes());
        assert_eq!(read_header(&b), Err(SaveFileError::BadMagic));
        let mut b = b"SFSG".to_vec();
        b.extend_from_slice(&2u32.to_le_bytes());
        let h = hash(&b);
        b.extend_from_slice(&h.to_le_bytes());
        assert_eq!(
            read_header(&b),
            Err(SaveFileError::Version {
                what: "save file",
                found: 2,
                reads: 1
            })
        );
    }
}
