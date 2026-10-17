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

## Engine demo and benchmark (M0)

`sf_demo` is the engine demo: a 1,024 m map (256 x 256 cells of 4 m) with 5,000
wandering units, the test pack's models, minimap, F1 panels and a sound on
clicks. Wheel = zoom toward the cursor, WASD / arrows / screen edge = pan,
middle mouse = grab the ground, F1 = debug panels, F11 = fullscreen, Esc = quit.

```sh
cargo run --release -p sf_demo
```

### Benchmark (M0): for Tijmen on the Windows laptop

Milestone M0 asks for **p99 frame time <= 16.7 ms (60 fps)** over a fixed
camera flight from close-up to the whole map and back with 5,000 units, on the
reference laptop (GTX 1650, 1920 x 1080). The flight takes about 75 seconds.

Before you start: plug the laptop into **mains power**, make sure Windows uses
the **NVIDIA GPU** for the terminal / `sf_demo.exe` (Settings > System >
Display > Graphics, or the NVIDIA control panel), and **close other GPU-heavy
apps** (browsers with video, games, screen recorders). Do not touch the mouse
or keyboard during the flight; the demo ignores camera input while it runs.

```sh
cargo run --release -p sf_demo -- --bench
```

It opens in real fullscreen at 1920 x 1080 without vsync, flies the path, writes
**`bench.json` in the folder you ran the command from** (or give
`--out C:\path\bench.json`) and quits. It prints a one-line summary and notes
(for example if vsync could not be turned off or the resolution is not 1080p).

**Paste back:** the whole `bench.json` (about 600 lines) and the last lines the
command printed (`bench: ...`). The important numbers are `frame_ms.p99`,
`gpu_ms` (or "n/a"), `display.actual`, `present.mode`, `criterion.caveats`, and
under `hitches` the slowest frames with the instance buffers created in them.

If it does not start: `SF_GPU_TRACE=1` prints every GPU start-up step to stderr
(set it as `$env:SF_GPU_TRACE=1` in PowerShell, `set SF_GPU_TRACE=1` in cmd), so
a hang shows where it sits. The game window uses DX12 on Windows;
`WGPU_BACKEND=vulkan` (or `dx12`) overrides that. Other flags: `--seconds 30`
shorter flight, `--frames 300` fixed number of frames (smoke test), `--windowed`
or `--resolution 1280x720` instead of 1080p fullscreen, `--present vsync` to
measure with vsync on, `--no-reserve` to turn the instance-buffer pre-allocation
off (follow-up 14).

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
`sf_engine`, `sf_render`, `sf_ui`, `sf_audio`, `sf_cutscene`, `sf_net`) and
`sf_game` (the game shell as a library, plus the development viewer binary);
`tools/` holds `sf_editor`, `sf_headless` and `sf_demo` (the engine demo and
benchmark, the same shell with the demo's defaults).
See PLAN.md section 1.1.
