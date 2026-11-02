# Cursor context

Work package WP3.5 (PLAN.md phase 3). Code: `crates/sf_game/src/cursor.rs` (the
decision), `commands.rs` (`right_click`), `runner.rs` (`update_cursor`), in `sf_ui`
`cursor.rs` (the pictures) and `Texture::Sprites`, in `sf_engine` `Edges`, and the
placeholder set in `assets/ui/cursors/` written by `tools/sf_icon_gen --cursors`.
Tests: `crates/sf_game/src/cursor/tests.rs`, `crates/sf_game/tests/cursor_screenshot.rs`,
`crates/sf_ui/src/cursor.rs`, `tools/sf_icon_gen/src/cursors.rs`.

The cursor shows **what a click will do**, and a right-click does exactly that.

## One source of truth

`cursor_context(world, selection, hover, modifiers) -> CursorKind` is a pure
function: the same inputs give the same answer, nothing is changed, nothing is
allocated, and no window or GPU is needed to test it. The runner calls it once a frame to
choose the picture, and `commands::right_click` calls the same function to decide the
order, then maps the answer with `order_for(kind, hover)`, the only place where a cursor
becomes an `Order`. So the picture and the click cannot disagree
(`a_right_click_gives_exactly_the_order_the_cursor_shows` sweeps every combination of
selection, hover and modifiers and compares them).

Like the selection, the context is **presentation**: it reads the world, never changes it,
and orders still reach the sim only as commands on the next tick (PLAN.md section 5, rule 1).
No sim code was touched and the goldens did not change.

Inputs:

* **hover**: the entity whose picture is under the mouse (`Instancer::pick`: the model's
  box close up, the icon quad zoomed out, the same answer the hover highlight and a click
  use, PLAN.md section 2.3, so the context is the same at every zoom), the ground point under
  the mouse in cells (`Camera::ground_point`), and the screen edges the mouse is scrolling.
  Over the UI (the top bar, the minimap, an F1 panel) there is no hover: the operating
  system's cursor shows there and a click orders nothing.
* **modifiers**: the actions `OrderQueue` (shift, held) and `OrderAttackMove` (F, held),
  never their keys.
* **selection**: the selected units that can take an order (`Selection::orderable`, the own
  mobile ones; a selected structure counts for nothing, armed or not) and whether any of them
  has a weapon (`UnitStats::weapon_range` > 0).
* **terrain**: `World::passable(class, cell)`, below.

## The decision table

*Since WP11.1 (`sidebar.md`) the sidebar keeps out of the edge-scroll band, so row 1 and the sidebar never meet on a
pixel. Row 1 still wins over a unit under the cursor inside the band (progress item 22): decided and kept, see there.*

The first row that matches wins.

| # | Situation | Cursor | A right-click gives |
|---|---|---|---|
| 1 | the mouse is at a screen edge that scrolls | the scroll cursor of that edge (8: N, NE, E, SE, S, SW, W, NW; the corners are the diagonals) | nothing |
| 2 | no selected unit can take an order (nothing, or only structures) and the mouse is over an enemy or neutral thing | **Inspect** | nothing (a left-click inspects) |
| 3 | the same, over anything else (own thing, ground, sky) | **Select** | nothing |
| 4 | over an **enemy** unit or structure and at least one selected unit has a weapon | **Attack** | `Attack(entity)` |
| 5 | over **one of ours** (unit or structure) and some selected unit other than it can take the order | **Guard** | `Guard(Entity(entity))` |
| 6 | no ground point under the mouse (the sky) | **Select** | nothing |
| 7 | the ground point is off the map, or no selected unit can enter that cell | **NoGo** | nothing |
| 8 | attack-move held and at least one selected unit has a weapon | **AttackMove** | `AttackMove(point)` |
| 9 | queue held | **Queue** | `Move(point)`, queued |
| 10 | otherwise | **Move** | `Move(point)` |

Notes on the rows:

* **Modifiers.** Queue (shift) also queues an Attack, an AttackMove and a Guard: the order is
  queued whenever the modifier is held and the order is one of those or a Move. With both
  modifiers held the attack-move wins (it is the more specific), and it is queued. Neither
  modifier changes Select, Inspect, NoGo or a scroll.
* **Row 4, no weapon.** A selection with nothing that can shoot (engineers, harvesters, medics)
  never shows Attack: the enemy is only a place on the ground and rows 6 to 10 apply (a Move
  onto it). Structures never count as the armed unit: a selected defence tower does not turn a
  right-click into an attack, because structures take no orders.
* **Row 4, neutral.** A neutral unit (the civilians, `owner` 2) is not attacked by a right-click;
  it is ground for the orders. It can be inspected by a left-click.
* **Row 5, guard needs no modifier.** Right-click on a friendly unit means "stay with it", the
  common practice of the genre, and the sim already has `Guard(entity)`. The cost is that a
  move onto a spot that a friendly icon covers (strategic zoom) becomes a guard; the
  cursor shows Guard before the click, so it is visible and not a surprise. A modifier for a
  forced move can be added later as one more action. A unit cannot guard itself (the sim
  refuses it): when the only selected unit is the one under the mouse, it is ground like any
  other and rows 6 to 10 apply. With two selected, the other one guards it.
* **Row 5, structures.** Guarding an own structure is allowed. Capture, repair, enter and the
  like (engineers on structures, repair trucks on vehicles) come in later phases and will
  take rows before this one.
* **Row 7.** A selection is blocked only when **every** orderable unit is blocked: one unit
  that can go is enough, the order goes to all and the sim sorts out who arrives (path
  finding is phase 4). NoGo gives **no order** at all, so a unit is never sent into the river
  by a click; before this work package the click went through. A right-click off the map used
  to be clamped to the edge by the sim; it is now refused.
* **Enemy in water.** The cell under an enemy does not matter: an enemy gunboat in the river
  is still attacked.
* **Stale hover.** An id whose entity is gone (or whose slot now holds another) is bare ground
  (the generation check, `selection::facts`).

### What can enter what

Since WP4.1 the cursor does not read terrain flags of its own: it asks the simulation. A
selected unit **can go** to a cell when `World::passable(class, cell)` says its movement class
may enter it, the very table the units move by (`movement.md`: foot, wheeled and tracked cross
grass, road, ore and bridge; hover also water; vessels water and bridge; aircraft anything on
the map). A unit with no class, and a class the table does not have, could go anywhere on the
map (never block what is not understood); a cell off the map is nobody's. The old `Passability`
and `Mobility` of WP3.5, which read the flags by class name, are gone, and so is the
`Passability` argument of `cursor_context` and `commands::right_click`: the world is the one
source of truth, so the picture, the click and the walk cannot disagree
(`the_cursor_asks_the_sims_cost_table_and_nothing_else` sweeps every class over land, river and rock).

## The cursors

24 pictures, `assets/ui/cursors/<name>.png`, 32 x 32 RGBA, named by `CursorKind::name`:

* in use: `select` (a pointer), `inspect` (a lens), `move` (four arrows), `queue` (the same
  with a plus), `attack` (a crosshair), `attack_move` (a crosshair that leaves to the right),
  `guard` (a shield), `no_go` (a forbidden sign);
* the 8 scroll arrows `scroll_n` to `scroll_nw`, pointing the way the screen scrolls;
* **placeholders** `capture` (flag), `repair` (spanner), `enter` (door), `deploy` (box with
  arrows out), `harvest` (drop), `infiltrate` (eye), `sell` (coin), `place` (corner marks):
  their enum variants and pictures exist so that phases 6 to 10 only have to teach
  `cursor_context` when to answer them. **Nothing returns them today and `order_for` gives no
  order for them** (`placeholders_never_come_out_of_the_context_and_never_order`).

The pictures are drawn by `tools/sf_icon_gen` (`cargo run -p sf_icon_gen -- --cursors`, the
approach of the strategic icons: signed distance shapes, integer-exact arithmetic, the same
bytes on every machine; `--check` compares with what is on disk, and a test does too). They
are own shapes, plain geometry, no third-party material. They are placeholders until an artist
delivers a set in the same format.

### The format (for artists)

* 32 x 32 texels, 8-bit RGBA, straight alpha; **coloured** (drawn as it is, not tinted).
* **A dark outline** of about 1.7 texels all the way round the silhouette, so the cursor reads
  on grass, sand, grey rock, snow and water alike (the generator test
  `every_cursor_has_a_dark_outline_all_round` and the screenshot test check it).
* Keep the picture about 26 texels across at most; nothing may touch the border.
* `assets/ui/cursors/cursors.ron` names each cursor, its file and its **hotspot** (the texel
  that is the mouse position, from the top-left: the tip of the pointer, the middle of a
  crosshair) and `size_px`, the logical size the 32 texels are drawn at (32: one texel per
  pixel at UI scale 1). The loader refuses a hotspot outside the picture, a file name that is a
  path, a non-square picture and a bad size; the game then keeps the system cursor and says so.

## Drawing

`sf_ui` draws the cursor as one textured quad appended to the UI's draw list after the
panels (`Ui::draw_cursor`), so it is above everything the UI shows, and it needs no new
allocation (the list keeps its capacity; `drawing_cursors_does_not_allocate_...` checks it).
The pictures live in the UI atlas, read as **colour** (`Texture::Sprites`): the GPU decodes
their sRGB texels through an sRGB view of the atlas texture, while the glyphs and the skin stay
coverage and shading. The quad is one draw call however many cursors a frame has (just the one
in the game).

* **Size scales with the UI scale** (`ui.ron` scale times the DPI factor): the picture is
  `size_px * scale` pixels and its corner snaps to a whole pixel, so a 1:1 picture keeps its
  texels crisp. The hotspot stays on the mouse at every scale.
* **The system cursor is hidden over the game view only.** The runner hides it while the
  software cursor is drawn: the mouse is in the window and not over the HUD, the minimap or an
  F1 panel. Over those, and outside the window, the system cursor is back, and it is restored on
  exit. If the pictures cannot be loaded the system cursor simply stays.
* **Screen edges.** `Edges` (`sf_engine`) says which edge bands the mouse is in: only edge
  *triggers* of bindings count (a pan key does not), so a rebound or unbound edge shows no scroll
  cursor, and the cursor never says "scrolling" when the camera is not.
* **Only in the sandbox.** The engine demo's crowd (`sf_demo`) and every benchmark run draw no
  game cursor, so the benchmark scene and its numbers are unchanged; `--hover x,y` (the fixed
  mouse for screenshots) draws one at that spot. A plain `--shots` run draws **no** game
  cursor, so the pictures do not depend on where the X pointer happens to be; only
  `--shots --hover x,y` shows it.

## Keys

Every key is an action in `assets/config/input.ron` (rebindable). New in this work package:

| Action | Default | |
|---|---|---|
| `OrderAttackMove` | F, held | a right-click on the ground attack-moves instead of moving (row 8) |

A is the camera's pan left, so it could not be the attack-move key; F is free and near the
camera keys. It is a **held modifier** like shift, not a key that arms the next click, so the
cursor can show the AttackMove picture while it is held and the click needs no state. The binding
test of `sf_engine` still allows only the shift of `OrderQueue` and `SelectAdd` to be shared.

## Decisions that were open

* Guard on a friendly unit needs no modifier (row 5); see the note about strategic zoom.
* A neutral unit is not attackable by a right-click (row 4).
* A click off the map or onto impassable ground is refused rather than clamped or sent (row 7).
* Hover over a selected structure with only structures selected shows Select, not Guard:
  structures take no orders.
* Attack-move and queue are held modifiers (F, shift), not click-then-click.

## Where it is tested

* `crates/sf_game/src/cursor/tests.rs`: every row above, the modifiers, each movement class
  against water and rock, mixed selections, the eight scroll cursors and their precedence, the
  placeholders never being the answer, the right-click equal to the cursor over a sweep, each
  order run through the sim, and that every kind has a picture in the shipped set.
* `crates/sf_game/tests/cursor_screenshot.rs`: all 24 cursors over a generated terrain through
  the real renderer (software adapter): drawn at the hotspot, the outline dark on any ground, one
  draw call, growing with the UI scale, clipped at the window edge, nothing drawn with no cursor.
  Frames: `docs/screenshots/wp3.5/` (`cursors-over-terrain.png`, and the move cursor at UI scale
  1 and 2: `cursor-ui-scale-1-and-2.png` real size, `-x4` enlarged).
* `crates/sf_ui/src/cursor.rs`: loading, refusing a broken set, the hotspot and the scale, one
  quad last and in its own batch, no allocation.
* `crates/sf_engine/src/input.rs`: `Edges`, and the attack-move binding.
* `tools/sf_icon_gen/src/cursors.rs`: determinism, outline, borders, hotspots and that the
  committed files are what the generator writes.
