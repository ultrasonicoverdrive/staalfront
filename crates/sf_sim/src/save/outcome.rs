//! Section `G`: the match outcome (WP13.1c-3a, `docs/rules/save.md`). Whether the match began, the
//! defeats in their order and the result. The players are rules (in the rules digest); `dirty` is
//! false between steps (`evaluate_match_rules` clears it at the end of every step, and only
//! `despawn` and `sell_command`, both inside a step, set it), so it is not saved.

use sf_math::bytes::{ByteReader, ByteWriter};

use super::{SaveError, SaveResult, ensure};
use crate::ids::{PlayerId, Tick};
use crate::outcome::{MatchResult, Outcome, Verdict};

/// Every saved field; destructured without `..`.
pub(crate) fn save(w: &mut ByteWriter, o: &Outcome) -> SaveResult<()> {
    let Outcome {
        // Rules: in the rules digest.
        rules: _,
        begun,
        // Derived: false between steps (see the module text).
        dirty: _,
        defeats,
        result,
    } = o;
    w.bool(*begun);
    w.usize(defeats.len())?;
    for &(player, tick) in defeats {
        w.u8(player.0);
        w.u64(tick.0);
    }
    match result {
        None => w.u8(0),
        Some(MatchResult {
            tick,
            verdict: Verdict::Won(p),
        }) => {
            w.u8(1);
            w.u64(tick.0);
            w.u8(p.0);
        }
        Some(MatchResult {
            tick,
            verdict: Verdict::Draw,
        }) => {
            w.u8(2);
            w.u64(tick.0);
        }
    }
    Ok(())
}

/// The outcome of a world at tick `now`. Refused when it is not one `decide` writes (the bounds,
/// each with its writer, are in `docs/rules/save.md`).
pub(crate) fn restore(r: &mut ByteReader<'_>, o: &mut Outcome, now: Tick) -> SaveResult<()> {
    let players = o.rules.players().to_vec();
    let n = players.len();
    let begun = r.bool()?;
    let count = r.count(9)?;
    let mut defeats: Vec<(PlayerId, Tick)> = Vec::with_capacity(count);
    for _ in 0..count {
        defeats.push((PlayerId(r.u8()?), Tick(r.u64()?)));
    }
    let result = match r.u8()? {
        0 => None,
        1 => {
            let tick = Tick(r.u64()?);
            Some(MatchResult {
                tick,
                verdict: Verdict::Won(PlayerId(r.u8()?)),
            })
        }
        2 => Some(MatchResult {
            tick: Tick(r.u64()?),
            verdict: Verdict::Draw,
        }),
        _ => return Err(SaveError::Corrupt("unknown match result")),
    };
    // `decide` pushes a defeat only for a player of the rules not defeated before, at the tick of
    // the step that runs it (the tick counter advances after it), and only once the match began.
    ensure(
        defeats.iter().enumerate().all(|(k, &(p, t))| {
            players.binary_search(&p).is_ok() && t < now && defeats[..k].iter().all(|d| d.0 != p)
        }),
        "a defeat the match stage never records",
    )?;
    // One check per tick, in ascending tick; within it the players in ascending order.
    ensure(
        defeats
            .windows(2)
            .all(|d| (d[0].1, d[0].0) < (d[1].1, d[1].0)),
        "the defeats are out of order",
    )?;
    ensure(
        begun || (defeats.is_empty() && result.is_none()),
        "a defeat before the match began",
    )?;
    // The result is set in the step of the last defeat: a win when one player is left, a draw
    // when the last ones (two or more, else the one left would have won before) fall together.
    let last = defeats.last().map(|d| d.1);
    let fits = match result {
        None => defeats.len() + 2 <= n,
        Some(MatchResult {
            tick,
            verdict: Verdict::Won(p),
        }) => {
            defeats.len() + 1 == n
                && Some(tick) == last
                && players.binary_search(&p).is_ok()
                && defeats.iter().all(|d| d.0 != p)
        }
        Some(MatchResult {
            tick,
            verdict: Verdict::Draw,
        }) => {
            defeats.len() == n
                && Some(tick) == last
                && defeats.iter().rev().take(2).all(|d| d.1 == tick)
        }
    };
    ensure(fits, "a match result the match stage never leaves")?;
    o.begun = begun;
    o.dirty = false;
    o.defeats = defeats;
    o.result = result;
    Ok(())
}

/// The coverage record of the section (`World::save_coverage`).
pub(crate) fn coverage(o: &Outcome, out: &mut Vec<&'static str>) {
    let mut add = |name: &'static str, on: bool| {
        if on && !out.contains(&name) {
            out.push(name);
        }
    };
    add("outcome.begun", o.begun);
    add("outcome.defeats", !o.defeats.is_empty());
    add(
        "outcome.result.won",
        matches!(
            o.result,
            Some(MatchResult {
                verdict: Verdict::Won(_),
                ..
            })
        ),
    );
    add(
        "outcome.result.draw",
        matches!(
            o.result,
            Some(MatchResult {
                verdict: Verdict::Draw,
                ..
            })
        ),
    );
}
