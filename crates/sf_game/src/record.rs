//! The command record (WP1.16a, `sf_game --match --record FILE`).
//!
//! The sim thread keeps every command it was handed with the tick it actually ran at and the tick the
//! world had reached when it got it ([`crate::sim_thread::SimConfig::record_commands`]); at the end of
//! the play this module writes them as an `sf_headless` scenario, so that
//! `sf_headless FILE` runs the same match without a window, for a profiler. The file
//! format, with the `game` block and the exact values, is defined in `sf_headless::scenario`
//! (`exact.rs`); `docs/rules/hitch_log.md` ("The command record") describes it.
//!
//! The replay is exact: the test `tests/record_replay.rs` plays a match through the sim thread, writes
//! the record with [`render`], loads it with `sf_headless` and gets the same checksum at every tick.

use std::path::{Path, PathBuf};

use sf_headless::scenario::{GameSpec, RecordedCommand, render_recording};

use crate::sim_thread::SimExit;

/// The match file as the record names it: relative to the assets when it lies under them, else as given.
pub fn match_path_text(match_file: &Path, assets: &Path) -> String {
    let text = match match_file.strip_prefix(assets) {
        Ok(rest) => rest.to_string_lossy().into_owned(),
        Err(_) => match_file.to_string_lossy().into_owned(),
    };
    text.replace('\\', "/")
}

/// The scenario text of a finished play: what `exit` (the stopped sim thread) ran, in the world of
/// `cells` with the match file `match_file` under `assets`.
pub fn render(
    exit: &SimExit,
    cells: (u32, u32),
    assets: &Path,
    match_file: &Path,
) -> Result<(String, usize), String> {
    let config = crate::bench_scene::world_config(cells);
    let abs = std::path::absolute(assets).unwrap_or_else(|_| assets.to_path_buf());
    let game = GameSpec {
        assets: abs.to_string_lossy().replace('\\', "/"),
        match_path: match_path_text(match_file, assets),
        speed_raw: config.speed.raw(),
    };
    // A command handed over after the last tick ran changes no checksum of the record.
    let commands: Vec<RecordedCommand> = exit
        .commands
        .iter()
        .filter(|c| c.arrived.0 < exit.ticks)
        .map(|c| RecordedCommand::new(c.arrived, c.effective, c.player, &c.command))
        .collect();
    let text = render_recording(
        "recorded_match",
        config.seed,
        cells,
        &game,
        exit.ticks,
        &commands,
    )
    .map_err(|e| e.to_string())?;
    Ok((text, commands.len()))
}

/// Writes the record to `path`; the number of commands in it.
pub fn write(
    path: &Path,
    exit: &SimExit,
    cells: (u32, u32),
    assets: &Path,
    match_file: &Path,
) -> Result<usize, String> {
    let (text, n) = render(exit, cells, assets, match_file)?;
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(n)
}

/// The commands of a game record (`--record`) that come after a save of that game (WP13.1e-2,
/// `docs/rules/save.md` "Resuming a game record"): the record's first commands must be the save's log (section
/// `L`), one for one, and what follows them is still to be fed. A game record is resumed by this **count**, never
/// by the save tick (`Arrivals::skip_to`): in the game a command can arrive at the save tick after the save was
/// taken, and it is then in the record but not in the save.
///
/// `record_ticks` is the record's length (`Scenario::ticks`). [`render`] leaves out every command that arrived
/// when the play had run `record_ticks` ticks or more (it changes no checksum of the record), so of the log only
/// the commands that arrived before `record_ticks` are looked for; when the record ends at or before the save
/// tick there is nothing left to feed and the answer is empty. Refused when the record does not begin with that
/// part of the log, or holds commands beyond it although it ends before the last logged arrival.
pub fn commands_after_save<'a>(
    recorded: &'a [RecordedCommand],
    record_ticks: u64,
    saved: &[sf_headless::savefile::LoggedCommand],
) -> Result<&'a [RecordedCommand], String> {
    // The log is ascending by arrival (`savefile::load` checks it).
    let in_record = saved
        .iter()
        .take_while(|s| s.arrived.0 < record_ticks)
        .count();
    if recorded.len() < in_record {
        return Err(format!(
            "the record holds {} commands, the save {in_record} that arrived within the record",
            recorded.len()
        ));
    }
    for (i, (r, s)) in recorded.iter().zip(&saved[..in_record]).enumerate() {
        if *r != RecordedCommand::new(s.arrived, s.effective, s.player, &s.command) {
            return Err(format!(
                "command {i} of the record is not command {i} of the save"
            ));
        }
    }
    let rest = &recorded[in_record..];
    if in_record < saved.len() && !rest.is_empty() {
        return Err("the record ends before the save but holds commands after it".to_owned());
    }
    Ok(rest)
}

/// Where the record of a game that a restart ended goes: `FILE` with `.restartN` before the extension.
pub fn restart_path(path: &Path, n: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let name = match path.extension() {
        Some(ext) => format!("{stem}.restart{n}.{}", ext.to_string_lossy()),
        None => format!("{stem}.restart{n}"),
    };
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_match_file_is_named_under_the_assets_or_as_given() {
        let assets = Path::new("/game/assets");
        assert_eq!(
            match_path_text(Path::new("/game/assets/matches/m1.ron"), assets),
            "matches/m1.ron"
        );
        assert_eq!(
            match_path_text(Path::new("/elsewhere/m2.ron"), assets),
            "/elsewhere/m2.ron"
        );
    }

    #[test]
    fn a_restart_gets_its_own_file() {
        assert_eq!(
            restart_path(Path::new("out/play.ron"), 2),
            Path::new("out/play.restart2.ron")
        );
        assert_eq!(
            restart_path(Path::new("play"), 1),
            Path::new("play.restart1")
        );
    }
}
