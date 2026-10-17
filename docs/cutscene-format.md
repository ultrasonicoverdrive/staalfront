# Cutscene scene format (WP12.1)

For the **writer** (turning a Fountain briefing into a scene), the **artist**
(rigging figures, painting backgrounds) and the **implementer** (WP12.2, the
player). The source of truth for names and defaults is
`crates/sf_cutscene/src/model.rs`; the example to copy from is
[`assets/scenes/example_bridge.ron`](../assets/scenes/example_bridge.ron).

A scene is one cartoon, a **RON** file: background layers, cutout characters
and a timeline of events at fixed millisecond times. The cartoon player (WP12.2)
draws it; this crate (`sf_cutscene`) defines it, checks it and can say what the
frame looks like at any time, without drawing anything.

```
content/briefings/F1-M01.fountain ──fountain2scene──▶ assets/scenes/f1_m01.ron
                                                          │  edit by hand: art, voices, camera
                                                          ▼
                                       scene_check  ──▶  player (WP12.2)
```

Clean-room (B5): scenes contain only Staalfront's own content.

## 1. Quick start

```
# Fountain briefing -> skeleton scene (placeholder figures, silent sound slots)
cargo run -p sf_cutscene --bin fountain2scene -- content/briefings/F1-M01.fountain assets/scenes/f1_m01.ron

# Check a scene you edited; errors come with line numbers
cargo run -p sf_cutscene --bin scene_check -- assets/scenes/f1_m01.ron
```

Both look for the asset root in `./assets` (or `--assets DIR`) to list missing
files. `fountain2scene` exits 0 when the result is valid (warnings and missing
assets do not count), 1 when it is not, 2 on bad usage or unreadable files.
`scene_check` exits 0 when the scene loads.

In code:

```rust
let loaded = sf_cutscene::load_file(path, Some(assets_root))?;   // parse + validate
let scene = loaded.scene;
let state = scene.sample_step(prev_ms, now_ms);                  // once per frame
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
| `art` | `Image("path")` (PNG/SVG, drawn centred, one image pixel per stage unit), `Rect(w, h, color)` or `Circle(radius, color)`. The shapes are the placeholders. |
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

## 8. Not in this format yet

* Drawing and playing: WP12.2. Skipping and subtitle styling: WP12.2.
* Mouth shapes from speech: WP12.3 (`lipsync` only names the file).
* Video fallback: WP12.4.
* Live reload for the artist: WP12.6.
* Swapping a part's picture inside a pose (mouth shapes, blinking eyes): not
  needed for the placeholders; WP12.3 will decide how mouth shapes are chosen.
