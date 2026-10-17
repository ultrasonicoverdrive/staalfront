# Cutscene scene format (WP12.1)

For the **writer** (turning a Fountain briefing into a scene and previewing it),
the **artist** (rigging figures, painting backgrounds) and the **implementer**
(whoever hooks the player into the game). The source of truth for names and defaults is
`crates/sf_cutscene/src/model.rs`; the examples to copy from are the small
[`assets/scenes/example_bridge.ron`](../assets/scenes/example_bridge.ron) and the complete,
hand-directed mission 1 briefing
[`assets/scenes/f1_m01_briefing.ron`](../assets/scenes/f1_m01_briefing.ron) (section 10).

A scene is one cartoon, a **RON** file: background layers, cutout characters
and a timeline of events at fixed millisecond times. This crate (`sf_cutscene`)
defines it, checks it, can say what the frame looks like at any time without
drawing anything, and has the player that draws and plays it (section 8).

```
content/briefings/F1-M01.fountain ──fountain2scene──▶ assets/scenes/f1_m01.ron
                                                          │  edit by hand: art, voices, camera
                                                          ▼
                                       scene_check  ──▶  play_scene (window) / the game's player
```

Clean-room (B5): scenes contain only Staalfront's own content.

## 1. Quick start

```
# Fountain briefing -> skeleton scene (placeholder figures, silent sound slots)
cargo run -p sf_cutscene --bin fountain2scene -- content/briefings/F1-M01.fountain assets/scenes/f1_m01.ron

# Check a scene you edited; errors come with line numbers
cargo run -p sf_cutscene --bin scene_check -- assets/scenes/f1_m01.ron

# Watch it (section 8): a window, Space pauses, Esc quits
cargo run -p sf_cutscene --bin play_scene -- assets/scenes/f1_m01.ron

# Or skip the conversion step and watch a Fountain file directly
cargo run -p sf_cutscene --bin play_scene -- --fountain content/briefings/F1-M01.fountain

# While writing: the window reloads every time you save (section 8.5)
cargo run -p sf_cutscene --bin play_scene -- --watch --fountain content/briefings/F1-M01.fountain
```

Both look for the asset root in `./assets` (or `--assets DIR`) to list missing
files. `fountain2scene` exits 0 when the result is valid (warnings and missing
assets do not count), 1 when it is not, 2 on bad usage or unreadable files.
`scene_check` exits 0 when the scene loads.

In code:

```rust
let loaded = sf_cutscene::load_file(path, Some(assets_root))?;   // parse + validate
let scene = loaded.scene;
let state = scene.sample_step(prev_ms, now_ms);                  // once per frame (no player)
let mut player = sf_cutscene::Player::new(scene);                // or let the player keep the time (section 8)
```

## 2. Units and conventions

| Thing | Convention |
|---|---|
| Time | Integer **milliseconds** from the start of the scene (`at`, `duration_ms`). Negative is an error. |
| Stage | A frame of `stage` units (default 1920 x 1080). **Origin = centre of the frame, `+x` right, `+y` down.** Camera zoom 1 shows exactly the stage. |
| Angles | Degrees, **positive = clockwise on screen**. |
| Colours | `"#rrggbb"` or `"#rrggbbaa"`. |
| Asset paths | Relative to the asset root (`assets/`), forward slashes, no `..`, not absolute. |
| Names | Characters, parts, poses, layers and tracks are looked up by name; names must be unique in their scope. |
| Unknown fields | Rejected, with a line number, so typos do not pass silently. |

Add `#![enable(implicit_some)]` as the first line (the generator does) and an
optional value can be written without `Some(...)`: `voice: "voice/l1.ogg"`.
Comments with `//` and `/* */` are fine.

## 3. A scene

```ron
#![enable(implicit_some)]
Scene(
    format: 1,                 // the only version so far
    name: "example_bridge",    // identifier
    title: "Hold the Crossing",// optional
    source: "content/briefings/EXAMPLE-bridge.fountain",   // optional: where it came from
    stage: (width: 1920, height: 1080),                     // optional
    layers: [ ... ],
    characters: [ ... ],
    timeline: (tracks: [ ... ]),
)
```

### 3.1 Layers (backgrounds, parallax)

```ron
(name: "mountains", art: Image("art/bg/mountains.png"), parallax: 0.4, z: -20, shots: ["field_tent"]),
(name: "floor",     art: Rect(w: 2400.0, h: 420.0, color: "#40495b"), pos: (0.0, 540.0), z: -19),
(name: "mist",      art: Rect(w: 600.0, h: 200.0, color: "#ffffff40"), parallax: 2.0, z: 5),
```

| Field | Meaning |
|---|---|
| `art` | `Image("path")` (a PNG, drawn centred, one image pixel per stage unit), `Rect(w, h, color)`, `RoundRect(w, h, radius, color)` or `Circle(radius, color)`. The shapes are the placeholders. |
| `pos` | Where the layer sits in the world. Default `(0, 0)`. |
| `parallax` | How strongly the layer follows a camera pan. `1` (default) moves with the world, where the characters stand; `0` is glued to the frame (a sky); `0.5` is far away; `2` is a foreground that slides past faster. |
| `z` | Draw order. **`z < 0` is behind all characters, `z >= 0` in front of them.** Lower is drawn first. Default 0. |
| `shots` | Names of the shots in which the layer is visible; empty (default) = always. See `CameraCut` below. |

### 3.2 Characters (cutout rigs)

```ron
(
    id: "major_halversen",         // what events say
    name: "Major Halversen",       // what the subtitle prints (default: id)
    scale: 1.0, z: 0,              // optional: size and draw order between characters
    parts: [
        (name: "torso", art: Rect(w: 130.0, h: 220.0, color: "#3a5f8a")),
        (name: "arm_r", parent: "torso", offset: (83.0, -95.0), pivot: (0.0, -75.0), z: 2,
         art: Image("art/halversen/arm_r.png")),
        (name: "head",  parent: "torso", offset: (0.0, -165.0), z: 3, art: Circle(radius: 55.0, color: "#e8c39e")),
    ],
    poses: [
        (name: "neutral", parts: []),
        (name: "point",   parts: [(part: "arm_r", angle: -75.0)]),
    ],
)
```

**Parts** form a tree (`parent`; no parent = root). The character's position is
where the roots hang.

| Field | Meaning |
|---|---|
| `offset` | Where this part's **joint** rests, relative to the parent's joint (roots: relative to the character position). |
| `pivot` | Where the joint is **inside the picture**, measured from the picture's centre. A hanging arm drawn with the shoulder at its top edge has `pivot: (0, -height/2)`. Default `(0, 0)` = rotate about the centre. |
| `z` | Draw order inside the character, independent of the tree. Lower first, ties by list order. |
| `art` | As for layers. |

**Poses** are named arrangements. A pose lists the parts that differ from rest
(`angle`, `offset` added to the rest offset, `scale` default `(1, 1)`); parts
it does not list stand at rest. Poses are absolute, not "on top of the previous
pose".

Angles blend **linearly**, no shortest-way wrap-around: `-170` to `170` swings
the long way round. Choose the numbers to choose the direction.

A character **without parts** is a voice only (radio, narrator, off-screen).

**For the artist.** Deliver each figure as separate pictures per part (head,
torso, upper arm, ...). For each, write down where its joint is in the picture
(`pivot`) and where that joint sits relative to its parent (`offset`). The
generator's placeholder figure shows the pattern. Art faces **right**; a
character that `Enter`s or `Move`s with `facing: Left` is mirrored.

## 4. The timeline

```ron
timeline: (tracks: [
    (name: "dialogue", events: [
        (at: 2000, kind: Dialogue(speaker: "major_halversen", text: "Commander.", duration_ms: 1500)),
    ]),
    (name: "camera", events: [
        (at: 0, kind: CameraCut(shot: "tent")),
    ]),
])
```

A **track** is a named lane of events, **sorted by `at`** (unsorted is an error).
Tracks are for your overview ("camera", "dialogue", "audio"); the player merges
them. Events at the same time play in track order, then in list order.

### 4.1 Event kinds

Durations are `duration_ms`; `easing` defaults to `EaseInOut` where it exists.
Unlisted fields are required.

| Event | Fields (default) | Does |
|---|---|---|
| `Enter` | `character`, `pos`, `facing` (`Right`), `pose` (none = all parts at rest) | The character steps onto the stage. Error if already on stage. |
| `Exit` | `character` | Leaves the stage. |
| `Move` | `character`, `to`, `duration_ms`, `easing`, `facing` (unchanged) | Walks to `to`. |
| `SetPose` | `character`, `pose`, `duration_ms` (0), `easing`, `hint` (none) | Blends from what the character looks like **now** to the pose. `hint` is the writer's stage direction, informational. |
| `Dialogue` | `speaker`, `text`, `duration_ms` (> 0), `voice` (none), `lipsync` (none), `delivery` (`OnScreen`) | Subtitle for `duration_ms`; the voice file starts with it. `delivery`: `OnScreen`, `VoiceOver` (radio, narrator: V.O.), `OffScreen` (O.S.). `lipsync` is the mouth-shape file for `voice` (WP12.3). |
| `Sfx` | `sound` (none), `note` (""), `volume` (1.0) | One-shot sound. |
| `Music` | `action` (`Play`/`Stop`), `file` (none), `note`, `fade_ms` (0) | Start or stop the music. |
| `Ambience` | same as `Music` | Start or stop the ambient bed. |
| `CameraPan` | `to`, `duration_ms`, `easing` | Moves the camera centre. |
| `CameraZoom` | `to` (> 0), `duration_ms`, `easing` | Zoom, 1 = the whole stage. |
| `CameraCut` | `center` ((0,0)), `zoom` (1), `shot` (none) | Jumps to a framing. With `shot: "name"` layers listing that name become visible and others disappear. |
| `CameraShake` | `amplitude`, `duration_ms` (> 0) | Shakes the frame; the shake fades out linearly. Deterministic. |
| `Transition` | `kind` (`Cut`, `FadeIn`, `FadeOut`), `duration_ms` (0), `color` (`"#000000"`) | `FadeIn`: from solid colour to the picture. `FadeOut`: from the picture to solid colour, which **stays** until a `FadeIn` or a `Cut`. `Cut`: removes any fade at once. |
| `Wait` | `duration_ms` | Nothing happens; gives the scene length when the last real event is earlier. |
| `MapOverlay` | `note`, `markers` ([]), `duration_ms` (> 0) | The briefing map insert (`[[MAP: ...]]`). `markers`: `(label, at, to)`; with `to` an arrow, without a pin. |

`sound`, `file`, `voice`, `lipsync` are **asset paths**. They may be left out
(`None`) while the recording or sound does not exist yet; the free text
`note` keeps what the writer wanted ("radio static").

### 4.2 How values change

`Move`, `SetPose`, `CameraPan` and `CameraZoom` start from **whatever the value
is when the event begins**. If a pose change starts while the previous one is
still running, it continues smoothly from the halfway pose. After the duration
the value stays until the next event touches it.

Easing turns progress `0..1` into a blend weight (`0` = old value, `1` = new):

| Easing | Curve |
|---|---|
| `Linear` | `p` |
| `EaseIn` | `p²` (slow start) |
| `EaseOut` | `1 - (1-p)²` (slow end) |
| `EaseInOut` | `2p²`, then mirrored (slow both ends) |
| `Step` | `0` until the end, then `1`: no in-between frames, the old value is held. For an immediate change use `duration_ms: 0` with any easing. |

Every curve gives exactly `0` at the start and exactly `1` at the end.

## 5. Checks

`load_str` / `load_file` / `scene_check` report **errors** (the scene is
rejected) and **warnings** (it plays, but probably not as intended), each with
the line of the offending value:

| Error | |
|---|---|
| Syntax, wrong or unknown field, wrong variant | from `ron`, with line and column |
| Unknown character, part, pose, parent, shot | including `Enter ... pose:` and `layers[].shots` |
| Part parents form a loop; duplicate names | characters, parts, poses, layers, tracks |
| Negative time or duration; zero-length dialogue/shake/map | |
| **Unsorted events in a track** | |
| **One speaker overlaps their own line** | different speakers may talk over each other; back to back is fine |
| Character acts without being on stage; enters twice; exits when absent | `Move`, `SetPose`, `Exit`, `Enter` |
| Non-positive sizes, zoom; non-finite numbers | |
| **Out of range**: an angle beyond 3600 degrees, a position, offset, pivot, shape size, corner radius or shake beyond 100 000 stage units, a scale or parallax beyond 1000, a zoom outside 0.001 to 1000 | almost always a typo (an extra zero); the limits are `MAX_ANGLE_DEG`, `MAX_COORD`, `MAX_SCALE`, `MIN_ZOOM`, `MAX_ZOOM` in `validate.rs` |
| Asset path that is empty, absolute, has `\`, or contains `..` | scenes cannot reach outside the asset root |
| Unsupported `format` | |

| Warning | |
|---|---|
| An `OnScreen` line whose speaker is not on stage | use `delivery: VoiceOver`/`OffScreen`, or add an `Enter` |

**Missing assets are not errors.** With an asset root, the loader lists every
referenced file that does not exist (`LoadedScene::missing_assets`, printed as
`note: missing ...`). That list is the to-do list for the artist and the
recording session; placeholders keep the scene playable meanwhile.

## 6. Evaluating a scene (for the player, WP12.2)

No rendering is involved; all of it is pure and unit-tested.

```rust
let state: SceneState = scene.sample(t_ms);                 // the frame at t
let state = scene.sample_step(Some(prev_ms), t_ms);         // + sound events in (prev, t]
let fired = scene.fired_between(Some(prev_ms), t_ms);       // only the sound events
```

`SceneState` holds:

* `camera` (`center`, `zoom`, `shake`) and `shot`.
* `back_layers`, `characters`, `front_layers` **in draw order**. A layer's
  `position` already has the parallax folded in (draw it like a world object at
  `(position - camera.center) * zoom`). `shake` is added to everything,
  layers included.
* Per character: `position`, `facing` and `parts` in draw order, each with the
  blended `pose`, the world position of its `joint` and a `world` matrix that
  maps the part's picture (centred on its own origin) into the world, pivot,
  tree, scale and facing included.
* `subtitle` (speaker, name, text, `delivery`, `voice`, `lipsync`, elapsed time;
  if lines of two speakers overlap, the one that started last), `fade`
  (`color`, `alpha` 0..1), `map_overlay`, `finished`.
* `music` and `ambience`: **what is playing at `t`** (`LoopState`: file, note, fade, when it
  started), found from the last `Music` / `Ambience` event up to `t`; `None` after a `Stop`
  or before the first `Play`. This is what a player restarts after a seek.
  `scene.voices_running_at(t)` lists the voice lines that started before `t` and are still
  running, with how far into the line `t` is.
* `sounds`: the sound events that **started in `(prev, t]`**: `Sfx`, `Music`,
  `Ambience`, and `Dialogue` with a `voice` file. `prev = None` on the first
  call fires everything from time 0 on. Consecutive intervals `(a, b]`, `(b, c]`
  fire every event exactly once, whatever the frame length; a seek backwards
  fires nothing.

## 7. From Fountain

`fountain2scene` turns `content/briefings/*.fountain` (conventions in
`content/README.md`) into a scene that validates and plays, with geometric
placeholder figures, so the artist has something to refine.

| Fountain | Scene |
|---|---|
| title page `Title:` | `title` |
| Scene heading `INT. FIELD COMMAND TENT - DAWN` | A **shot** named after the place (`field_command_tent_dawn`): a `CameraCut`, two placeholder background layers that show only in that shot, `Enter` for the characters named in capitals in its action lines and for those who speak, a 2 s `Wait` to establish the place. The previous shot's characters `Exit`. |
| Cue + dialogue | `Dialogue`; **duration = words / 2.5 per second, at least 1.5 s**; 0.3 s pause after every line. Lines wrapped over several text lines are joined. |
| `(V.O.)` / `(O.S.)` after the cue | `delivery: VoiceOver` / `OffScreen`; such a speaker is not put on stage (a character who only ever speaks like this is voice-only, without parts). |
| Parenthetical `(tired, steady)` | `SetPose` at the start of the line with `hint: "tired, steady"`, choosing a placeholder pose by keyword: `point` (points), `subdued` (tired, quiet, wary, worried, concerned...), `forceful` (firm, sharp, urgent, clipped, alert, ...), otherwise `neutral`. A parenthetical in the middle of a speech starts a new line of dialogue. |
| `[[SFX: x]]`, `[[MUSIC: x]]`, `[[AMBIENCE: x]]` | `Sfx` / `Music` / `Ambience` with `note: "x"` and no file. `MUSIC: stop` (also `off`, `none`, `silence`) is `action: Stop`. Notes before the first line of a shot fire at the **start of the shot**, later ones where they stand. |
| `[[SHOT: map]]` | `CameraCut` to the shot `map`: a full-frame placeholder map layer in front of everything. It lasts until the end of the next dialogue line (3 s if none follows), then the camera cuts back. |
| `[[MAP: x]]` | `MapOverlay` with `note: "x"` and no markers (add arrows and pins by hand) for the length of the map shot. |
| `[[SHOT: wide]]` / `[[SHOT: reaction NAME]]` | Zoom 0.85 for the rest of the shot / a 1.5 s close-up on that character. Other `SHOT` values have no preset and are reported. |
| `> FADE IN:` / `> FADE OUT.` | `Transition` `FadeIn` / `FadeOut`, 0.8 s. |
| `CUT TO:` | `Transition` `Cut`. Other transitions become cuts with a warning. |

Action lines are only used to see who is in the room, then dropped. Free notes
(`[[Example only ...]]`) are ignored. Every file under `content/briefings/`,
the template included, converts and validates (a test keeps that true), and
every spoken line survives word for word.

Each generated event ends in `// line N`, the line in the Fountain file it
came from. The output is a **starting point**: regenerate with care, it
overwrites hand edits. Add real art, voices and direction in the RON.

## 8. Playing a scene (WP12.2)

Two pieces: the **player** (`sf_cutscene::Player`), a state machine that keeps the time, and
the **renderer** (`sf_cutscene::render::CutsceneRenderer`), which draws one frame. The
player has no clock of its own: whoever calls it passes the wall-clock milliseconds, so it is
testable and the game can slave it to anything.

### 8.1 Previewing a briefing (writer, artist)

```
cargo run -p sf_cutscene --bin play_scene -- assets/scenes/f1_m01.ron
cargo run -p sf_cutscene --bin play_scene -- --fountain content/briefings/F1-M01.fountain
```

The second form converts the Fountain file in memory (nothing is written). Add `--watch` and
the window reloads by itself whenever you save the file (section 8.5); without it the loop is:
edit the `.fountain` file, close the window, run it again. Warnings of the converter and
files that are missing (art, voices, sounds) are printed first; missing art is drawn as a
magenta box, missing sounds are silent. Sound cues are **printed** as they come up
(`[ 20.1 s] sfx: radio static`), because there is no sound yet. Options: `--assets DIR`
(otherwise found like the game does: `SF_ASSETS`, or `assets/` near the program),
`--quit-after MS` (leave by itself, for smoke tests), `--watch` (section 8.5). The window can be resized; the picture
keeps its shape (bars appear), and the subtitle size follows the picture height.

| Key | Does |
|---|---|
| Space, click | pause / continue |
| Left, Right | back / forward 2 seconds |
| J, L | back / forward 1 second |
| `,` and `.` | back / forward 0.1 second |
| N, Tab | start of the next shot (at the last shot: the end) |
| Home, R | start over |
| End | skip to the end (the last frame stays) |
| H | show / hide the writer's HUD (section 8.5) |
| Esc | quit |

A thin bar along the bottom edge shows how far the scene is, with a tick where every shot
starts; two small bars in the top right corner mean "paused". The window title shows the
time. **Shots** are the `CameraCut` events that name a shot (section 4.1); `N` jumps from
one to the next, so name a shot at every place you want to be able to skip to.

### 8.2 What is drawn

(Frames of the example briefing, made by the screenshot test with `SF_SAVE_SHOTS`:
[`docs/screenshots/wp12.2/`](screenshots/wp12.2/): mid-dialogue, during the fade-out, the map
overlay, a 1080p frame, and a window of another shape with bars.)

Back to front: the background layers (`z < 0`) with their parallax under the camera's pan,
zoom and shake; the characters, every part as the sampler placed it (parts in their `z`
order, rotated about their joints, mirrored for `facing: Left`); the layers in front
(`z >= 0`); the map overlay (pins, arrows and their labels, glued to the frame). Then, over
the whole window, the fade colour, and the **subtitle**: a dark box near the bottom of the
picture with the speaker's name (yellow; light blue for `VoiceOver`, grey for `OffScreen`)
and the line in white, wrapped to at most 70 % of the picture width. The text is 3.4 % of
the picture height (37 px at 1080p, 24 px at 720p, never under 14 px). A line that would fill
more than 40 % of the picture shrinks the text, down to 14 px; if it still does not fit, the
rest is cut and the box ends in `...` (the box always stays inside the picture; keep lines
short, a subtitle is not a paragraph).

* The shapes (`Rect`, `RoundRect`, `Circle`) are the placeholder art; they are drawn smooth
  at any rotation. `Image` parts and layers need an 8-bit RGB or RGBA **PNG** below
  `assets/`; **SVG is not supported by the player yet** (export a PNG). A picture that cannot
  be loaded is a magenta box.
* Fades: `FadeOut` to a colour, `FadeIn` from it, `Cut` removes it; subtitles stay on top of
  a fade so a line is never lost in the dark.
* The font is the UI's (Inter, `sf_ui`); the characters of the Latin-1 and Latin Extended-A
  blocks are there.

### 8.3 Controls for the game

`sf_cutscene::Bindings` maps keys to what the player can do. `Bindings::game()` is for a
briefing in the game: **Esc, Space or a click skips to the end**, one configurable key
(`next_shot`, default Enter) jumps to the next shot. `Bindings::viewer()` is the table
above. Bindings are RON (`Bindings::from_ron_str("(next_shot: [Char('k')])")`; unnamed
fields keep the game preset). The player takes a `Key` (`Escape`, `Space`, `Enter`, `Tab`,
`Left`, `Right`, `Home`, `End`, `Click`, `Char('x')`), so it does not depend on a window
system: `player.press(key, &bindings)`.

### 8.4 Using the player in code

```rust
let mut player = Player::new(scene);
let mut cutscenes = CutsceneRenderer::new(&gpu.device, &gpu.queue, renderer.color_format(), assets_root);
// every frame:
player.update(wall_ms, &mut sound_sink);            // wall_ms: any monotonic ms clock
let state = player.sample();
renderer.render_with(|target| cutscenes.render(target, player.scene(), &state, None))?;
if player.is_finished() { /* the scene is over: go on with the mission */ }
```

`Player` controls: `play`, `pause`, `toggle_pause`, `seek(ms)`, `seek_by(ms)`, `restart`,
`skip_to_end`, `skip_to_next_shot`; state: `mode()` (`Playing`, `Paused`, `Finished`),
`time_ms()`, `duration_ms()`, `shot_starts()`. A frame that took more than 500 ms of wall
time advances the scene by only 500 ms (a hitch must not skip dialogue).
`CutsceneRenderer::render` adds one pass to the engine renderer's frame (it clears the
target to black and draws the picture, letterboxed); it does not touch `sf_render`.

**Sound** goes out through the `SoundSink` trait (a closure `FnMut(&SoundEvent)` works too),
so `sf_audio` can be hooked in later without `sf_cutscene` depending on it. The player never
plays anything itself. What it sends:

| `SoundEvent` | When |
|---|---|
| `Sfx`, `Voice { offset_ms: 0 }`, `Bed { Music / Ambience, Play / Stop }` | once, as the time passes the event |
| `Pause`, `Resume` | when the player is paused / continues; nothing else happens while paused |
| `StopAll`, then `Bed` (music, ambience) and `Voice { offset_ms }` for what is sounding | after a **seek** or a jump to the next shot: sound is put back the way it would be had the scene played normally up to the new time. Sound effects in between are not played; an event exactly at the new time fires normally after the restore. A seek made while paused is carried out when playback continues. |
| `StopAll` | **skip to the end**: the scene is over. A scene that plays to its end sends nothing at the end. |

### 8.5 Writing with a live preview (`--watch`, WP12.6)

For the writer (and the artist): leave the viewer open next to your editor and **save**; the
window shows the new version within about half a second, at the same moment of the scene you
were looking at. It is a flag of `play_scene`, so everything in 8.1 (keys, `--assets`) works
the same.

**Tijmen, writing a briefing.** In PowerShell, from your clone, once:

```powershell
cd C:\Users\Tijmen!\staalfront
cargo run -p sf_cutscene --bin play_scene -- --watch --fountain content\briefings\F1-M01.fountain
```

(The first run builds for a few minutes; after that it starts at once.) Open the same
`.fountain` file in your editor and write. Every save re-converts the briefing (nothing is
written to disk) and plays on from where it was. For a scene RON file (the artist), leave out
`--fountain`: `... play_scene -- --watch assets\scenes\example_bridge.ron`. To leave: Esc in the
window, or Ctrl+C in the terminal.

| Key | Does |
|---|---|
| H | show / hide the **writer's HUD**, a small box top left: current time / total and whether it plays, the shot (`2 / 5  field_command_tent_dawn`), the active speaker (with `(V.O.)` / `(O.S.)`), and the path of the file being watched. On by default with `--watch`. |
| J, L | back / forward 1 second |
| `,` and `.` | back / forward 0.1 second (hold to repeat is not supported: tap) |
| Space | pause; stepping while paused moves the picture and the subtitle, so you can read a line at your own speed |
| N, Home, End, Left, Right | as in 8.1 |

**What is watched.** The scene file (or the `.fountain` file) and every picture the scene
refers to, also pictures that do not exist yet: the moment the artist saves one, it appears.
The files are looked at every 250 ms; a change is applied after the first look that finds
nothing new, so a save shows up 0.25 to 0.5 s later, and a write that takes several steps (an
editor that writes a temporary file and renames it, deletes and recreates the file, or writes
half the file first and the rest 50 ms later) gives one reload after the last step, never one
with a half-written file. Saving without changing anything (or `touch`) reloads too; that is
the way to force one. A scene or Fountain file over 8 MiB is not read at all: the red box says
`file too large (9 MiB, limit 8 MiB)` and the last good version stays. Sound is not played by the viewer; cues are printed in the
terminal as before, and after a reload the music, ambience and voice line that belong to the
current moment are announced again, as after a seek.

**What happens to the time.** The playhead stays at the same time `t` in the new version. If
the new version is shorter than `t`, it stays at the end. A paused scene stays paused; a scene
that had reached its end plays on if you made it longer.

**When the file has an error.** The window never closes and never goes blank. It keeps playing
the **last good version** and shows a red box in the top left corner, and the same lines are
printed in the terminal:

```
Reload failed - still showing the last good version:
assets/scenes/example_bridge.ron:22:105: Expected float
assets/scenes/example_bridge.ron:105: track "stage", event 0: Enter at 0 ms: unknown character "zed"
```

Each line reads `file:line:column: message` as in `scene_check`; a message that only knows
the line (an unknown character, two lines at once: everything the validator finds) has no
column. A Fountain file almost never fails to convert (the converter turns oddities into
warnings, printed in the terminal when the file loads, see section 7), so errors are mostly a
thing of the RON. If one does happen, the line is the line **in your Fountain file** (the
converter keeps track of where every event came from); when the problem belongs to the
generated scene rather than to a line of yours, the message says "(generated scene, line N)".
More than 12 errors show the
first 12 and "... and N more". Fix the file and save: the red box disappears by itself and the
terminal says `error fixed, reloaded`. The window title also says `ERROR, see the overlay`
while the last save is not valid. If the file is already broken when you start with `--watch`,
the window opens empty with the errors on it and loads the scene the moment it is valid
(without `--watch`, a broken file is reported and the program ends with status 1). A file that
does not exist ends the program with status 2 either way.

**Pictures.** A picture that cannot be loaded (missing, not a PNG, an unsupported kind) stays a
**magenta box**, and an amber box lists each with the reason (`art/halversen/head.png: file not
found`). Fix or add the file, save it, and the box and the list update. Pictures are read again
on every reload, so a repainted PNG shows up too. SVG is still not supported (8.2).

Frames of this: [`docs/screenshots/wp12.6/`](screenshots/wp12.6/): the error overlay with the
HUD, the pictures list while two pictures do not load, and the same scene after one was saved.

**How it is built.** Polling, not a file-watch library: every 250 ms the watched files are
compared by size and modification time; a file that changed, or whose time is less than 2 s old,
is also hashed (file systems with coarse time stamps, such as FAT, or Windows updating the time
late, can show two saves as alike in size and time, and the hash tells them apart; a file that
has not been touched for 2 s only costs a `stat`; files over 32 MiB are never hashed). The reload
waits for a quiet period of at least one poll interval after the last change that was seen (the
150 ms "debounce" is raised to the interval; shorter, it would end before the next look). In code:
`sf_cutscene::preview::{Source, Preview, LivePreview}` (the reload state machine and the HUD
text) and `sf_cutscene::watch::{SceneWatcher, Debounce}` (both take the clock as a number of
milliseconds, so they are tested with a fake one); `Player::replace_scene` keeps the playhead.

## 9. Not in this format yet

* SVG pictures in the player (PNG only for now), pictures with a palette or grey scale.
* Mouth shapes from speech: WP12.3 (`lipsync` only names the file).
* Video fallback: WP12.4.
* Swapping a part's picture inside a pose (mouth shapes, blinking eyes): not
  needed for the placeholders; WP12.3 will decide how mouth shapes are chosen.

Found while directing the demo scene (section 10); none of them blocked it, each has a
workaround there:

* **Layers do not animate.** A lamp cannot light up on the radio click, a flag cannot move. The
  only change a layer knows is "visible in this shot".
* **No idle or walk motion.** A `Move` glides; there is no looping bob or breathing, so a figure
  that stands still is perfectly still. (In the demo the table hides the legs, which hides the
  gliding.)
* **No rest rotation for parts and layers.** A slanted prop or a table seen in perspective needs a
  picture; the placeholders draw the table edge-on.
* **A part's draw order is fixed per part, not per pose.** "The hand is in front of the head only
  in the radio pose" is solved by giving that hand a high `z` for all poses.
* **Times are absolute.** A pause that grows pushes every later event, in all tracks, by hand;
  a relative time (`after: <event>`) would make the pauses cheap to tune.
* **A map overlay shows all its markers at once**, so pins cannot appear as the line mentions
  them, and the overlay is glued to the frame while the map layers zoom with the camera: the demo
  does not zoom during the map shot, or the pins would slide off their places.

## 10. How the demo scene was directed (WP12.5)

[`assets/scenes/f1_m01_briefing.ron`](../assets/scenes/f1_m01_briefing.ron) is the mission 1
briefing, complete: two speakers, one room, a map insert, camera work, 62 seconds. It is a model
for the artist who replaces the placeholders and for the writer who directs the next briefing.
Frames: [`docs/screenshots/wp12.5/`](screenshots/wp12.5/) (first line, the map, a two-shot, the
fade-out), made by `demo_briefing_at_four_moments` with `SF_SAVE_SHOTS`.

```
cargo run -p sf_cutscene --bin play_scene -- --watch assets/scenes/f1_m01_briefing.ron
cargo run -p sf_cutscene --bin scene_check -- assets/scenes/f1_m01_briefing.ron   # 0 warnings, 0 missing assets
```

### 10.1 The workflow

1. `fountain2scene content/briefings/F1-M01.fountain assets/scenes/f1_m01_briefing.ron`: the
   skeleton (section 7).
2. Direct it **by hand in the RON**. Every event that differs from the generator's carries
   `// hand edit: why`; events that carry only `// line N` are untouched. The header of the file
   lists what was added. A test (`demo_scene_marks_every_hand_edit`) fails when an edit is not marked.
3. **The text is the writer's.** Do not change, add or drop a spoken line, a speaker or a sound
   cue name in the RON. A test (`demo_scene_tells_the_briefing_word_for_word`) compares every line,
   its speaker and the order with the Fountain file, twice: through the importer and by an
   independent reading. Change the story in the `.fountain` file and redo the hand edits.
   (Regenerating overwrites the RON; keep a copy of the hand edits, `git diff` helps.)
4. Preview while you work with `--watch`; the writer's HUD (`H`) shows the shot and the speaker.

### 10.2 What was decided

| Part of the scene | Decision |
|---|---|
| Location | One heading, so one shot, `forward_command_post_dawn`: a tent, dawn outside the open flap, a trestle table with the map, a field radio (the ambience cue), a hanging lantern. |
| Parallax | **Three depths**: `0.6` (tent wall, the flap with the dawn, notice board), `1.0` (floor, centre pole, table, props: where the figures stand), `1.2` (the lantern on its cord and the canvas edge, in front of the figures and sliding past faster). The table (`z >= 0`) is in front of the figures so they stand behind it. All of it is `Rect`, `RoundRect` and `Circle`. |
| Map | `[[SHOT: map]]` is a second shot, `map`, with layers of its own at `z >= 40` and `parallax: 0`: the paper, farmland (south-west), the ore ridge (north), the road junction (centre) with the camp, the east road fading out into unlit hills. The `MapOverlay` adds pins and an arrow on top. |
| Entering, leaving | Elian walks in through the flap (`Enter` off stage left, `Move`), Mira follows from the right. At the end they turn (`facing`) and walk out during the fade, then `Exit`. |
| Poses from parentheticals | serious → `lean`, dry → `arms_crossed`, measured → `neutral`, firm → `arms_crossed`, analytical → `lean`, pointing at map → `point`, concerned → `arms_crossed`, neutral → `neutral`. Added: `lean` while Elian listens to the doubt, `radio` (hand to the headset) just before the radio click. The original words stay as `hint`. |
| Camera | A slow drift towards Elian on the first line; a pan to the speaker when it matters (`EaseInOut`, 0.7 to 0.9 s); a cut to the map one beat after Mira's line, back to the post framed on Elian; a wide two-shot for the pointing; a **push-in from 1.0 to 1.4 on Mira's doubt** (`EaseIn`, over the whole line); back to Elian for the last line; wide again for the radio. No shake: the briefing has no explosion. |
| Timing | Dialogue durations are the importer's (words / 2.5 per second); a test keeps them within 30 % of it. Only the pauses grew where the scene reads better: half a second to read the map, 0.9 s before Mira's doubt, 0.6 s before the closing line, a 0.5 s beat between the radio click and the fade. |
| V.O. | Nobody in this briefing speaks over the radio, so there is no off-screen speaker. If a briefing has one, give that character no parts (section 3.2) and put a radio or a framed picture on the stage as a layer, with `delivery: VoiceOver`. |
| Sound | `sound`, `file`, `voice` and `lipsync` stay `None`; the cue names (`radio click`, `restrained military tension`, ...) are in `note`. |

### 10.3 The rigs, and the names an artist must keep

Each speaking character is a cutout rig of about 12 parts, built from rounded rectangles and
circles in the faction's colours (Meridian: sand, olive, weathered blue; the sun on the sleeve is
the Meridian emblem). `scale: 1.8`; art faces **right**; the character position is the **hips**.

| Part | Parent | Joint inside the picture (`pivot`) | Rest `offset` | `z` | |
|---|---|---|---|---|---|
| `legs` | root | (0, -85) top | (0, 0) | -1 | a root of its own, so legs stay upright when the torso leans |
| `torso` | root | (0, 105) hips | (0, 0) | 0 | `lean` rotates about the hips |
| `head` | `torso` | (0, 56) neck | (0, -204) | 5 | |
| `mouth` | `head` | (0, 0) | (15, -34) | 7 | **the lip-sync part** (WP12.3) |
| `upper_arm_l`, `upper_arm_r` | `torso` | (0, -50) shoulder | (-78, -186), (78, -186) | 1, 3 | `_r` is the arm on the side the figure faces |
| `forearm_l`, `forearm_r` | upper arm | (0, -45) elbow | (0, 90) | 6, 4 | |
| `hand_l`, `hand_r` | forearm | (0, 0) | (0, 88) | 9, 4 | `hand_l` is above the head so it shows at the ear |
| `eye` | `head` | (0, 0) | (22, -66) | 7 | |
| Elian only | | | | | `cap`, `cap_brim`, `eye_bag`, `pocket_map`, `patch` |
| Mira only | | | | | `hair`, `headset_band`, `headset_cup`, `headset_mic`, `tablet`, `tablet_screen`, `patch` |

**Keep these names** when you replace a shape by a picture, because poses and events look parts
up by name: **`torso`, `head`, `mouth`, `upper_arm_l`, `upper_arm_r`, `forearm_l`, `forearm_r`**
(and `legs`, `hand_l`, `hand_r`, which the poses do not touch but the layering needs). The extra
parts (cap, hair, headset) are free: rename or drop them, no pose mentions them. Keep the **parent chain** (arm -> forearm -> hand, everything else -> torso) and
draw each picture so that its joint sits where the table says; then the angles in the poses stay
right. Change a joint, and the poses need new angles.

**The poses, five names** (the artist may change the numbers, not the names): `neutral` (arms
hang, a little bent), `point` (right arm out and down, onto the map), `lean` (torso 12 degrees
forward, both arms straight onto the table), `arms_crossed` (forearms across the chest), `radio`
(left forearm up to the headset; Mira only). Elian has the first four, Mira all five: 3 to 5 poses
per character is the budget.

Angles are degrees, clockwise positive, relative to the parent; check a new pose in the
preview, not on paper.

### 10.4 Tests

`crates/sf_cutscene/tests/briefings.rs` (text, markers, parts, poses, depths, direction, timing,
the CLI) and `tests/screenshot.rs` (`demo_briefing_at_four_moments`: the first line, the map shot,
a two-shot, the fade-out; each frame is not empty, and where a line is spoken the subtitle is
really drawn, checked the way WP12.2 does it and shown to **fail** on the same frame without the
subtitle; where nobody speaks, nothing bright sits where a subtitle would be). The four frames are
the PNGs above.
