# Selection

Work package WP3.3 (PLAN.md phase 3). Code: `crates/sf_game/src/selection.rs` (the
logic), `selection_view.rs` (the feedback), `runner.rs` (the glue), and in `sf_render`
`pick.rs` / `instancing.rs` (`Instancer::pick`, `Instancer::pick_rect`,
`Instancer::mark_selected`). Tests: `selection/tests.rs`, `selection_view/tests.rs`,
`crates/sf_render/src/instancing/frame_tests.rs`, `crates/sf_game/tests/selection_screenshot.rs`.

## What selection is

**Presentation state, not sim state** (PLAN.md section 1.2, quality rule 1). The
selection lives in `sf_game`. It is not hashed, not saved in the world, and nothing in
it can change the world. All it produces is the list of ids that `commands.rs` puts
into a `Command::Order` or `Command::Stop` (`Selection::orderable`). The headless
goldens did not change and no sim code was touched.

Three things are kept:

* the **selection**: the local player's own entities, ascending by id;
* the **inspected** entity: at most one entity of someone else (an enemy, a neutral
  unit or structure), for looking at only;
* nine **control groups**.

## What can be selected

| Thing | Click | Drag rectangle | Double-click | Group | Gets orders |
|---|---|---|---|---|---|
| own unit (mobile) | yes | yes | yes | yes | yes |
| own structure | yes | **no** | as a single click | yes | no (the sim refuses them anyway) |
| enemy or neutral thing | **inspect** | no | as a single click | no | **never** |
| entity without a unit-table entry (the crowd's placeholder wanderers) | no | no | no | no | no |

"Own" is `owner == LOCAL_PLAYER` (Meridian, seat 0). Orders are made from
`Selection::orderable`, which keeps only the own *mobile* entities, so an inspected enemy
and a selected structure are never in a command. The rule is stated once, in that
function, and tested in `selection/tests.rs` and `commands.rs`.

## Gestures

Every key and button is an action in `assets/config/input.ron` (rebindable; the runner
asks the action, never a key):

| Action | Default | |
|---|---|---|
| `SelectClick` | left button | press, release; the gesture is decided by `Pointer` |
| `SelectAdd` | shift, held | read when the button is **released**: add or toggle instead of replace |
| `GroupAssign` | ctrl, held | with a group key: assign instead of recall |
| `Group1` .. `Group9` | 1 .. 9 (and numpad 1 .. 9) | recall; with `GroupAssign` assign |

`SelectAdd` shares shift with `OrderQueue` on purpose (one is read with a click, the
other with an order); the binding test in `sf_engine` allows exactly that pair and no
other shared binding. The debug select-all on F2 of WP3.4 is gone.

* **Click.** A press and release that moved less than `DRAG_MIN_PX` (6 px). The unit
  under the cursor is the one `Instancer::pick` found in the frame that was drawn, so a
  click uses the same volume as the hover highlight: the model's oriented box through
  the cursor ray close up, the icon quad when zoomed out, the more visible of the two in
  the cross-fade band (see `pick.rs`). An own unit or structure replaces the selection.
  An enemy or neutral entity is inspected (the selection is emptied; the inspected
  entity gets a ring and a bar, and the top bar says "inspecting"). Bare ground empties
  the selection and the inspection.
* **Drag.** A press that moved `DRAG_MIN_PX` or more, even if it came back. The
  rectangle is in **screen pixels**, so it works the same at every zoom. A unit is taken
  when its picture touches the rectangle (`Instancer::pick_rect`): its model's box
  projected to the screen (grown to at least `icons.click_min_px`, the same minimum the
  click uses) while models are at least half of the picture, its icon quad beyond. Only
  own mobile units are taken. A drag that finds none empties the selection.
* **Double-click.** Two clicks within `DOUBLE_CLICK_MS` (400 ms) and `DOUBLE_CLICK_PX`
  (8 px). Takes every own mobile unit of the **same kind** whose picture touches the
  screen (`pick_rect` over the whole viewport), plus the clicked one. On a structure, an
  enemy or bare ground it is the single click. A third quick click starts over, and a
  drag in between breaks the pair.
* **Shift** (`SelectAdd`). Click: toggles the unit in the selection (a miss on bare ground
  with shift keeps the selection, so a slipped click does not lose it; shift on an enemy
  does nothing). Drag and double-click: add, never remove.
* **Groups.** `GroupAssign` + a number makes the selection that group (an empty
  selection empties it). **A unit is in one group at a time:** assigning moves its units
  out of the other groups. The number alone replaces the selection with the group's
  living members (an empty group changes nothing). Two taps of the same number within
  `DOUBLE_TAP_MS` (500 ms) also centre the camera on the middle of the group. Groups hold
  ids, so they are cleaned like the selection.
* **Dead units.** `Selection::forget_the_dead` runs once a frame and drops every id for
  which `World::is_alive` is false, which is the generation check: a slot that now holds
  another entity is not the unit that was selected. The same check runs when a group is
  recalled.
* **Over the UI.** A press on a panel, the minimap or an F1 panel does not start a
  gesture, and the hover (and so the click) is not computed under them.

## Feedback

Everything is drawn on the **world layer** of `sf_ui` (`Ui::world_layer`): a second draw
list that `UiRenderer` uploads in the same buffers and draws first, under the panels.
All rings, bars and the drag rectangle are **one UI draw call**, however many units are
selected (`selection_screenshot.rs` checks that with 1 and with every own unit). The
scratch buffers are reused, and with nothing selected and no drag the whole pass is
skipped (no pass over the units, no quads): the benchmark scene, which has no selection,
is unchanged.

| Zoom | Own selected | Inspected |
|---|---|---|
| models (close, middle) | green ring on the ground around the unit, sized from its model's box (longer side times `RING_GROW`); a box outline around a structure; health bar above the model | the same in red (enemy) or amber (neutral) |
| icons (strategic) | the icon gets `ICON_SELECTED` (a white outline thicker than the hover highlight, set by the icon shader) and a health bar above it | same, ring colours do not apply |
| cross-fade band | the ring is drawn as long as the model is; the bar follows the more visible representation | |

The ring is the projection of a circle (28 segments) or a rectangle on the ground at the
unit's footprint centre, so it has the perspective of the terrain it lies on. It is not
depth tested (it shows through hills), like the icons. The health bar is green above 60 %
of full hit points, amber above 30 %, red below; an entity with no hit points to lose
(`hp` 0, such as an ore field) has no bar. The bar's width follows the ring's width (close)
or the icon (far), between 26 and 120 logical pixels. The drag rectangle is a translucent
green fill with a bright outline.

Line widths and bars scale with the UI scale. The colours are constants in
`selection_view.rs`; they are placeholders until the art pass (phase 11).

Frames of this (software renderer, from `selection_screenshot.rs`): `docs/screenshots/wp3.3/`:
rings and bars close up, highlighted icons zoomed out, an inspected enemy, the drag rectangle.

## Tuning constants

`DRAG_MIN_PX` 6, `DOUBLE_CLICK_MS` 400, `DOUBLE_CLICK_PX` 8, `DOUBLE_TAP_MS` 500
(`selection.rs`); `RING_GROW` 1.25, `RING_PX` 2.5, `BAR_*` (`selection_view.rs`);
`SELECTED_EXTRA_PX` (`assets/shaders/icon.wgsl`). Placeholders for the feel rounds.

## Decisions that were open

* A double-click on a structure selects only that structure: the plan says structures are
  only selected by a single click.
* Shift + click on an enemy is ignored rather than inspecting it: it cannot join a
  selection, and the existing selection should not be lost by a slip.
* Control + number with nothing selected empties the group; the alternative (ignore) leaves
  no way to clear a group.
* A unit is in one group at a time. Say so if overlapping groups are wanted.
