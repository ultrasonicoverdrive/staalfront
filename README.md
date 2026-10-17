# Staalfront

Staalfront is a real-time strategy game in Rust, built on its own thin engine
(wgpu + winit) with a fully deterministic, headless-testable simulation.
The simulation (`sf_sim`) never touches the GPU, floats, the clock or I/O.

The full plan, architecture and quality rules are in [PLAN.md](PLAN.md).
Project status: phase 0 (workspace and CI); there is no gameplay yet.

## Build and test

The toolchain is pinned in `rust-toolchain.toml` (rustup installs it on first use).

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/check-sim-determinism.sh   # rejects f32/f64/HashMap/HashSet in sf_sim
```

## Original Red Alert audio (optional)

Staalfront ships only its own CC0 sounds. If you own Red Alert, it can play
the original sound effects instead, read at runtime from **your own files**;
nothing of Red Alert is ever part of Staalfront.

1. Get the game files. EA released Red Alert as freeware in 2008; the official
   freeware download (the two CD images) or an existing install both work.
   The files Staalfront reads are the `.mix` archives (for example `MAIN.MIX`
   from the CD image, or `REDALERT.MIX` from an install); copy them, or the
   whole install, into one directory.
2. Point Staalfront at that directory, either for one run with
   `sf_game --ra-audio <directory>`, or permanently in
   `assets/config/audio.ron`: `ra_audio: (enabled: true, path: "<directory>")`.
3. Which original sound plays for which event is set in
   `assets/audio/ra_mapping.ron`. The F1 panel's *Sound browser* lists every
   archive and sound found, plays them, and shows the names (or `#id`s) to put
   in the mapping.

Missing, unreadable or unknown files are fine: those events keep their CC0
sound and the game prints one warning.

### Extracting the sounds and music to WAV (`sf_ra_extract`)

Instead of the `.mix` archives, Staalfront can also read a folder of WAV
files made from **your own** Red Alert disc by the `sf_ra_extract` tool. It
reads a CD image (`.iso`), a mounted disc or install directory, or one `.mix`
file, and writes every sound effect, voice and music track as 16-bit WAV.
Videos (`.vqa`) are skipped (only counted), including their sound. Zip files
are not read: unzip first. On Windows:

```
cargo run --release -p sf_ra_extract -- "C:\Downloads\RedAlert\CD1\redalert.iso" --out C:\Games\RedAlertSounds
cargo run --release -p sf_game -- --ra-audio C:\Games\RedAlertSounds
```

The WAVs land in `<out>\<archive>\<name>.wav` (nested archives as
subfolders). `--dry-run` only lists what would be written; the summary at the
end shows the archives found, sounds written, failed entries with the reason,
skipped videos and the total size. Run it once per CD image with the same
`--out` folder if you have both. The game looks a name up in the WAV folder
first and falls back to the CC0 sound as before; the F1 sound browser lists
the WAV folders too.

**File names.** MIX archives store only a hash of each file name, and
Staalfront ships no list of Red Alert file names, so without help the files
are called `#1A2B3C4D.wav` (the id the mapping file accepts as well). If you
have or make a list of file names (one per line, such as `CLICK.AUD`; for
example written down from the F1 sound browser while listening), pass it with
`--names names.txt` and those files get their name (`CLICK.wav`).

**Keep the output out of any repository.** The tool refuses an `--out` folder
inside a git checkout (a `.git` above it); extracted Red Alert audio may never
be committed, shared or uploaded (B11).

## Layout

`crates/` holds the libraries (`sf_math`, `sf_sim`, `sf_data`, `sf_ai`,
`sf_engine`, `sf_render`, `sf_ui`, `sf_audio`, `sf_cutscene`, `sf_net`) and the
game binary `sf_game`; `tools/` holds `sf_editor`, `sf_headless` and `sf_demo`.
See PLAN.md section 1.1.
