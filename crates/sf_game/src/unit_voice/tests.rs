//! Which event each action raises and the brake (WP11.2). The selection, the pointer gestures and the
//! order commands are the production ones; the sounds go to a stand-in, or to a real `AudioSystem`
//! without a device that records what it would play. The runner's own wiring (`handle_selection`,
//! `handle_orders`) needs a window and is checked by reading, see `docs/rules/voices.md`.

use std::path::PathBuf;

use super::*;
use crate::commands;
use crate::cursor::{Hover, Modifiers};
use crate::sandbox::{MERIDIAN, NEUTRAL, VEYRAN};
use crate::selection::{Gesture, Pointer, Recall, Selection};
use crate::sim_snapshot::SimSnapshot;
use sf_audio::AudioConfig;
use sf_data::units::{Faction, UnitDefs};
use sf_math::Vec2;
use sf_sim::{Cell, GuardTarget, Kind, PlayerId, World, WorldConfig};

const ME: PlayerId = PlayerId(MERIDIAN);

struct Scene {
    world: World,
    defs: UnitDefs,
}

impl Scene {
    fn new(vision: bool) -> Self {
        let defs = UnitDefs::embedded();
        let world = World::new(WorldConfig::new(3).with_map_size(64, 64))
            .unwrap()
            .with_units(sf_bridge::unit_table(&defs).unwrap())
            .unwrap();
        let world = if vision {
            crate::playfield::with_vision(world, &defs)
        } else {
            world
        };
        Scene { world, defs }
    }

    fn put(&mut self, command: Command, owner: u8) -> EntityId {
        let before: Vec<EntityId> = self.world.snapshot().iter().map(|v| v.id).collect();
        self.world
            .schedule(self.world.tick(), PlayerId(owner), command)
            .unwrap();
        self.world.step();
        self.world
            .snapshot()
            .iter()
            .map(|v| v.id)
            .find(|id| !before.contains(id))
            .expect("the command made an entity")
    }

    fn unit(&mut self, role: &str, owner: u8, x: i32, y: i32) -> EntityId {
        let kind = Kind(self.defs.kind_of(role).unwrap());
        self.put(
            Command::SpawnUnit {
                kind,
                pos: Vec2::from_ints(x, y),
                owner: PlayerId(owner),
            },
            owner,
        )
    }

    fn structure(&mut self, role: &str, owner: u8, x: u32, y: u32) -> EntityId {
        let kind = Kind(self.defs.kind_of(role).unwrap());
        self.put(
            Command::PlaceStructure {
                kind,
                cell: Cell::new(x, y),
                owner: PlayerId(owner),
            },
            owner,
        )
    }

    fn snap(&self) -> SimSnapshot {
        SimSnapshot::capture(&self.world)
    }
}

/// One selection step as the runner does it: the orderable units before, the step, the orderable
/// units after, then [`selection_frame`].
fn step(
    scene: &Scene,
    selection: &mut Selection,
    acted: bool,
    additive: bool,
    apply: impl FnOnce(&mut Selection, &SimSnapshot),
) -> Option<UnitVoice> {
    let snap = scene.snap();
    let before = selection.orderable(&snap);
    apply(selection, &snap);
    let after = selection.orderable(&snap);
    selection_frame(acted, additive, &before, &after)
}

fn click(
    scene: &Scene,
    sel: &mut Selection,
    hit: Option<EntityId>,
    additive: bool,
) -> Option<UnitVoice> {
    step(scene, sel, true, additive, |s, w| s.click(w, hit, additive))
}

#[test]
fn clicking_an_own_unit_selects_with_a_voice() {
    let mut scene = Scene::new(false);
    let tank = scene.unit("light_tank", MERIDIAN, 10, 10);
    let mut sel = Selection::new(ME);
    assert_eq!(
        click(&scene, &mut sel, Some(tank), false),
        Some(UnitVoice::Select)
    );
    // Clicking the same unit again is a selection again: the brake, not this rule, thins it out.
    assert_eq!(
        click(&scene, &mut sel, Some(tank), false),
        Some(UnitVoice::Select)
    );
}

#[test]
fn clicking_bare_ground_or_an_enemy_or_a_neutral_says_nothing() {
    let mut scene = Scene::new(false);
    let tank = scene.unit("light_tank", MERIDIAN, 10, 10);
    let foe = scene.unit("rifleman", VEYRAN, 14, 10);
    let civ = scene.unit("civilian", NEUTRAL, 16, 10);
    let mut sel = Selection::new(ME);
    // An enemy and a neutral are only inspected: no own unit is selected, so nothing speaks.
    assert_eq!(click(&scene, &mut sel, Some(foe), false), None);
    assert_eq!(sel.inspected(), Some(foe));
    assert_eq!(click(&scene, &mut sel, Some(civ), false), None);
    // Bare ground empties the selection: silence too.
    click(&scene, &mut sel, Some(tank), false);
    assert_eq!(click(&scene, &mut sel, None, false), None);
    assert!(sel.is_empty());
}

#[test]
fn an_enemy_out_of_sight_is_not_found_and_says_nothing() {
    let mut scene = Scene::new(true);
    let _tank = scene.unit("light_tank", MERIDIAN, 4, 4);
    let far = scene.unit("rifleman", VEYRAN, 58, 58);
    scene.world.step();
    assert!(!scene.snap().seen(far), "the enemy far away is in the fog");
    let mut sel = Selection::new(ME);
    assert_eq!(click(&scene, &mut sel, Some(far), false), None);
    assert!(sel.inspected().is_none() && sel.is_empty());
}

#[test]
fn a_structure_alone_says_nothing_and_a_drag_never_takes_it() {
    let mut scene = Scene::new(false);
    let yard = scene.structure("barracks", MERIDIAN, 20, 20);
    let tank = scene.unit("light_tank", MERIDIAN, 10, 10);
    let mut sel = Selection::new(ME);
    assert_eq!(click(&scene, &mut sel, Some(yard), false), None);
    assert_eq!(sel.ids(), &[yard]);
    // A drag over both takes the unit only, and that speaks.
    let v = step(&scene, &mut sel, true, false, |s, w| {
        s.drag(w, &[yard, tank], false)
    });
    assert_eq!(v, Some(UnitVoice::Select));
    assert_eq!(sel.ids(), &[tank]);
}

#[test]
fn shift_adding_a_unit_speaks_and_shift_removing_one_does_not() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let b = scene.unit("rifleman", MERIDIAN, 12, 10);
    let mut sel = Selection::new(ME);
    click(&scene, &mut sel, Some(a), false);
    assert_eq!(
        click(&scene, &mut sel, Some(b), true),
        Some(UnitVoice::Select),
        "adds b"
    );
    assert_eq!(click(&scene, &mut sel, Some(b), true), None, "removes b");
    assert_eq!(sel.ids(), &[a]);
    // Shift on bare ground changes nothing and says nothing.
    assert_eq!(click(&scene, &mut sel, None, true), None);
}

/// A box drag takes many frames; the voice comes once, when the button is released.
#[test]
fn a_box_drag_speaks_once_at_the_release_and_not_on_its_frames() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let b = scene.unit("rifleman", MERIDIAN, 12, 10);
    let mut sel = Selection::new(ME);
    let mut pointer = Pointer::default();
    let mut said = Vec::new();
    pointer.press([100.0, 100.0]);
    for frame in 0..30 {
        // The runner's frame: only a release gives a gesture; `acted` follows it.
        pointer.moved(
            [100.0 + 10.0 * frame as f32, 100.0 + 5.0 * frame as f32],
            1.0,
        );
        said.push(step(&scene, &mut sel, false, false, |_, _| {}));
    }
    assert!(
        said.iter().all(Option::is_none),
        "no voice while dragging: {said:?}"
    );
    let gesture = pointer.release([390.0, 245.0], 1000, 1.0);
    let Some(Gesture::Drag(_)) = gesture else {
        panic!("{gesture:?}");
    };
    let v = step(&scene, &mut sel, gesture.is_some(), false, |s, w| {
        s.drag(w, &[a, b], false);
    });
    assert_eq!(v, Some(UnitVoice::Select));
    // The frames after the release: the selection stays, the voice does not repeat.
    for _ in 0..30 {
        assert_eq!(step(&scene, &mut sel, false, false, |_, _| {}), None);
    }
}

#[test]
fn a_double_click_selects_the_kind_with_a_voice() {
    let mut scene = Scene::new(false);
    let a = scene.unit("rifleman", MERIDIAN, 10, 10);
    let b = scene.unit("rifleman", MERIDIAN, 12, 10);
    let mut sel = Selection::new(ME);
    let mut pointer = Pointer::default();
    pointer.press([50.0, 50.0]);
    let first = pointer.release([50.0, 50.0], 1000, 1.0);
    assert_eq!(first, Some(Gesture::Click { double: false }));
    assert_eq!(
        click(&scene, &mut sel, Some(a), false),
        Some(UnitVoice::Select)
    );
    pointer.press([50.0, 50.0]);
    let second = pointer.release([50.0, 50.0], 1200, 1.0);
    assert_eq!(second, Some(Gesture::Click { double: true }));
    let v = step(&scene, &mut sel, true, false, |s, w| {
        s.select_same_kind(w, Some(a), &[a, b], false);
    });
    assert_eq!(v, Some(UnitVoice::Select));
    assert_eq!(sel.ids().len(), 2);
}

#[test]
fn recalling_a_group_speaks_and_an_empty_group_does_not() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let mut sel = Selection::new(ME);
    click(&scene, &mut sel, Some(a), false);
    sel.assign_group(0);
    click(&scene, &mut sel, None, false);
    // The runner: acted only when the recall did something.
    let snap = scene.snap();
    let before = sel.orderable(&snap);
    let recall = sel.recall_group(&snap, 0, 5000);
    let acted = recall != Recall::Empty;
    let v = selection_frame(acted, false, &before, &sel.orderable(&snap));
    assert_eq!(v, Some(UnitVoice::Select));
    // Group 2 is empty: nothing recalled, nothing said, the selection stays.
    let before = sel.orderable(&snap);
    let recall = sel.recall_group(&snap, 1, 6000);
    assert_eq!(recall, Recall::Empty);
    assert_eq!(
        selection_frame(
            recall != Recall::Empty,
            false,
            &before,
            &sel.orderable(&snap)
        ),
        None
    );
}

#[test]
fn a_move_order_raises_the_move_voice() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let mut sel = Selection::new(ME);
    let snap = scene.snap();
    sel.click(&snap, Some(a), false);
    let hover = Hover::ground(Cell::new(30, 30));
    let command = commands::right_click(&snap, &sel, &hover, Modifiers::default()).unwrap();
    assert!(matches!(
        command,
        Command::Order {
            order: Order::Move(_),
            ..
        }
    ));
    assert_eq!(order_voice(&command), Some(UnitVoice::Move));
    // Queued is still a move.
    let queued = commands::right_click(
        &snap,
        &sel,
        &hover,
        Modifiers {
            queue: true,
            ..Modifiers::default()
        },
    )
    .unwrap();
    assert_eq!(order_voice(&queued), Some(UnitVoice::Move));
}

#[test]
fn an_order_on_an_enemy_raises_the_attack_voice_and_never_the_move_voice() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let foe = scene.unit("rifleman", VEYRAN, 14, 10);
    let mut sel = Selection::new(ME);
    let snap = scene.snap();
    sel.click(&snap, Some(a), false);
    let hover = Hover::over(foe, Cell::new(14, 10));
    let command = commands::right_click(&snap, &sel, &hover, Modifiers::default()).unwrap();
    assert!(matches!(
        command,
        Command::Order {
            order: Order::Attack(_),
            ..
        }
    ));
    assert_eq!(order_voice(&command), Some(UnitVoice::Attack));
    // The attack-move modifier on the ground also attacks.
    let ground = Hover::ground(Cell::new(30, 30));
    let am = commands::right_click(
        &snap,
        &sel,
        &ground,
        Modifiers {
            attack_move: true,
            ..Modifiers::default()
        },
    )
    .unwrap();
    assert!(matches!(
        am,
        Command::Order {
            order: Order::AttackMove(_),
            ..
        }
    ));
    assert_eq!(order_voice(&am), Some(UnitVoice::Attack));
}

#[test]
fn the_other_orders_say_nothing() {
    let mut scene = Scene::new(false);
    let a = scene.unit("light_tank", MERIDIAN, 10, 10);
    let ids = [a];
    assert_eq!(order_voice(&commands::stop_command(&ids).unwrap()), None);
    assert_eq!(
        order_voice(&commands::scatter_command(&ids, false).unwrap()),
        None
    );
    assert_eq!(
        order_voice(&commands::hold_command(&ids, false).unwrap()),
        None
    );
    assert_eq!(
        order_voice(&commands::move_command(&ids, Vec2::from_ints(1, 1), false).unwrap()),
        Some(UnitVoice::Move)
    );
    let guard = Command::Order {
        ids: ids.to_vec(),
        order: Order::Guard(GuardTarget::Point(Vec2::from_ints(2, 2))),
        queued: false,
    };
    assert_eq!(order_voice(&guard), Some(UnitVoice::Move));
    // Not an order at all.
    assert_eq!(order_voice(&Command::Stop { ids: ids.to_vec() }), None);
    // An order without units.
    let none = Command::Order {
        ids: Vec::new(),
        order: Order::Move(Vec2::from_ints(1, 1)),
        queued: false,
    };
    assert_eq!(order_voice(&none), None);
}

#[test]
fn the_events_are_the_bank_names() {
    let names: Vec<&str> = UnitVoice::ALL.iter().map(|v| v.event()).collect();
    assert_eq!(names, ["unit.select", "unit.move", "unit.attack"]);
}

// ---- the brake through `UnitVoices` -----------------------------------------------------------

#[derive(Default)]
struct Out {
    has: Vec<&'static str>,
    /// Events that play a mapped Red Alert original.
    original: Vec<&'static str>,
    busy: bool,
    played: Vec<String>,
}

impl VoiceOut for Out {
    fn has_sound(&self, event: &str) -> bool {
        self.has.contains(&event)
    }
    fn original(&self, event: &str) -> bool {
        self.original.contains(&event)
    }
    fn busy(&self) -> bool {
        self.busy
    }
    fn play(&mut self, event: &str) {
        self.played.push(event.to_string());
    }
}

fn voices() -> UnitVoices {
    UnitVoices::new(AudioConfig::embedded().voices)
}

fn all_sounds() -> Out {
    Out {
        has: vec!["unit.select", "unit.move", "unit.attack"],
        ..Out::default()
    }
}

#[test]
fn the_shipped_gaps_are_400_and_1200_ms() {
    let c = AudioConfig::embedded().voices;
    assert_eq!((c.min_gap_ms, c.repeat_gap_ms), (400, 1200));
}

#[test]
fn each_voice_plays_its_own_event() {
    for voice in UnitVoice::ALL {
        let mut v = voices();
        let mut out = all_sounds();
        assert!(v.speak(voice, Faction::Meridian, 10_000, &mut out));
        assert_eq!(out.played, [voice.event()]);
    }
}

#[test]
fn a_spammed_click_is_thinned_out_by_the_gaps() {
    let mut v = voices();
    let mut out = all_sounds();
    // Ten selections 100 ms apart, from t = 0 to 900: one voice (the repeat gap is 1200).
    for i in 0..10 {
        v.speak(UnitVoice::Select, Faction::Meridian, i * 100, &mut out);
    }
    assert_eq!(out.played, ["unit.select"]);
    // At 1200 ms the same event may speak again; at 1199 it could not.
    assert!(!v.speak(UnitVoice::Select, Faction::Meridian, 1199, &mut out));
    assert!(v.speak(UnitVoice::Select, Faction::Meridian, 1200, &mut out));
    // Another event only needs the shorter gap.
    assert!(!v.speak(UnitVoice::Move, Faction::Meridian, 1500, &mut out));
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 1600, &mut out));
    assert_eq!(out.played, ["unit.select", "unit.select", "unit.move"]);
}

#[test]
fn a_voice_that_is_still_sounding_blocks_a_new_one() {
    let mut v = voices();
    let mut out = all_sounds();
    assert!(v.speak(UnitVoice::Select, Faction::Meridian, 0, &mut out));
    out.busy = true;
    // Far past both gaps, but the first is still sounding: skipped, not cut off.
    assert!(!v.speak(UnitVoice::Move, Faction::Meridian, 60_000, &mut out));
    out.busy = false;
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 60_001, &mut out));
    assert_eq!(out.played, ["unit.select", "unit.move"]);
}

#[test]
fn without_a_sound_the_event_is_silent_and_uses_up_no_gap() {
    let mut v = voices();
    // Only the move event has a sound.
    let mut out = Out {
        has: vec!["unit.move"],
        ..Out::default()
    };
    assert!(!v.speak(UnitVoice::Select, Faction::Meridian, 1000, &mut out));
    assert!(!v.speak(UnitVoice::Attack, Faction::Meridian, 1001, &mut out));
    // The silent ones did not open a gap: the move voice speaks at once.
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 1002, &mut out));
    assert_eq!(out.played, ["unit.move"]);
}

#[test]
fn a_reset_forgets_the_last_voice() {
    let mut v = voices();
    let mut out = all_sounds();
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 5000, &mut out));
    v.reset();
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 5001, &mut out));
}

// ---- the real AudioSystem ---------------------------------------------------------------------

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// Since WP11.2b the shipped bank has the walkie-talkie lines for the three events: without the player's Red Alert
/// files each one speaks through `UnitVoices::speak` (the game's call), a line of its own event, in the
/// voice volume, with no unknown-sound count.
#[test]
fn the_shipped_bank_speaks_its_lines_without_red_alert_files() {
    let mut audio = AudioSystem::silent(assets());
    audio.set_trace(true);
    let mut v = voices();
    for (i, voice) in UnitVoice::ALL.into_iter().enumerate() {
        assert!(audio.has_sound(voice.event()));
        assert!(v.speak(
            voice,
            Faction::Meridian,
            10_000 * (i as u64 + 1),
            &mut audio
        ));
    }
    let trace = audio.take_trace();
    let got: Vec<(&str, &str)> = trace
        .iter()
        .map(|t| (t.name.as_str(), t.file.as_str()))
        .collect();
    assert_eq!(got.len(), 3);
    for ((name, file), prefix) in got.iter().zip([
        ("unit.select", "audio/voice/unit_select_"),
        ("unit.move", "audio/voice/unit_move_"),
        ("unit.attack", "audio/voice/unit_attack_"),
    ]) {
        assert_eq!(*name, prefix.0);
        assert!(file.starts_with(prefix.1), "{name}: {file}");
    }
    assert_eq!(audio.stats().unknown, 0);
}

// ---- a voice per side (WP11.2d) ----------------------------------------------------------------

/// The names the two sides speak with, as literals (the bank events of `assets/audio/bank.ron`).
#[test]
fn each_side_has_its_bank_events_and_neutral_has_none() {
    let veyran: Vec<&str> = UnitVoice::ALL.iter().map(|v| v.veyran_event()).collect();
    assert_eq!(
        veyran,
        [
            "unit.select.veyran",
            "unit.move.veyran",
            "unit.attack.veyran"
        ]
    );
    for v in UnitVoice::ALL {
        assert_eq!(v.event_for(Faction::Meridian), Some(v.event()));
        assert_eq!(v.event_for(Faction::Veyran), Some(v.veyran_event()));
        assert_eq!(v.event_for(Faction::Neutral), None);
    }
}

/// The shipped bank, through `UnitVoices::speak` (the game's call), without Red Alert files: each side speaks a
/// line of its own voice for every event (joe for Meridian, `cori_` for Veyran), in the voice volume and with
/// no unknown sound; Neutral says nothing and uses up no gap.
#[test]
fn the_shipped_bank_speaks_both_sides_each_in_its_own_voice() {
    let mut audio = AudioSystem::silent(assets());
    audio.set_trace(true);
    let mut v = voices();
    let mut t = 0;
    for voice in UnitVoice::ALL {
        t += 10_000;
        assert!(!v.speak(voice, Faction::Neutral, t, &mut audio), "neutral");
        assert!(audio.take_trace().is_empty(), "neutral said something");
        for faction in [Faction::Meridian, Faction::Veyran] {
            t += 10_000;
            assert!(
                v.speak(voice, faction, t, &mut audio),
                "{voice:?} {faction:?}"
            );
            let trace = audio.take_trace().pop().unwrap();
            let kind = voice.event().trim_start_matches("unit.");
            match faction {
                Faction::Meridian => {
                    assert_eq!(trace.name, voice.event());
                    assert!(
                        trace.file.starts_with(&format!("audio/voice/unit_{kind}_")),
                        "{}",
                        trace.file
                    );
                }
                _ => {
                    assert_eq!(trace.name, voice.veyran_event());
                    assert!(
                        trace
                            .file
                            .starts_with(&format!("audio/voice/cori_unit_{kind}_")),
                        "{}",
                        trace.file
                    );
                }
            }
            assert_eq!(trace.gain, 0.95, "{}", trace.file);
        }
    }
    assert_eq!(audio.stats().unknown, 0);
}

/// Nine select lines per side and three per move and attack; over many plays every one is heard and a line is
/// never followed by itself.
#[test]
fn the_variants_of_both_sides_all_play_and_never_repeat() {
    let mut audio = AudioSystem::silent(assets());
    audio.set_trace(true);
    let mut v = voices();
    let mut t = 0;
    for faction in [Faction::Meridian, Faction::Veyran] {
        for (voice, lines) in [
            (UnitVoice::Select, 9),
            (UnitVoice::Move, 3),
            (UnitVoice::Attack, 3),
        ] {
            let mut seen = std::collections::BTreeSet::new();
            let mut last = String::new();
            for _ in 0..150 {
                t += 10_000;
                assert!(v.speak(voice, faction, t, &mut audio));
                let file = audio.take_trace().pop().unwrap().file;
                assert_ne!(file, last, "{voice:?} {faction:?} repeated {file}");
                seen.insert(file.clone());
                last = file;
            }
            assert_eq!(seen.len(), lines, "{voice:?} {faction:?}: {seen:?}");
        }
    }
}

/// Both sides share the gaps and the one channel: a Veyran select right after a Meridian select is brake-skipped
/// (same event for the brake), and a voice that is sounding blocks the other side too.
#[test]
fn the_two_sides_share_the_brake_and_the_channel() {
    let mut v = voices();
    let mut out = Out {
        has: vec![
            "unit.select",
            "unit.select.veyran",
            "unit.move",
            "unit.move.veyran",
        ],
        ..Out::default()
    };
    assert!(v.speak(UnitVoice::Select, Faction::Meridian, 0, &mut out));
    assert!(
        !v.speak(UnitVoice::Select, Faction::Veyran, 1199, &mut out),
        "repeat gap"
    );
    assert!(
        !v.speak(UnitVoice::Move, Faction::Veyran, 399, &mut out),
        "min gap"
    );
    out.busy = true;
    assert!(
        !v.speak(UnitVoice::Move, Faction::Veyran, 60_000, &mut out),
        "busy"
    );
    out.busy = false;
    assert!(v.speak(UnitVoice::Move, Faction::Veyran, 60_001, &mut out));
    assert!(v.speak(UnitVoice::Select, Faction::Veyran, 61_201, &mut out));
    assert_eq!(
        out.played,
        ["unit.select", "unit.move.veyran", "unit.select.veyran"]
    );
}

/// With a mapped Red Alert original for the plain event the original speaks for both sides (the Veyran event is
/// never played); where only one event has an original the other plays its bank voice; a bank without Veyran
/// events plays the plain one for the Veyran side, as before WP11.2d.
#[test]
fn the_original_wins_for_both_sides_and_a_bank_without_a_veyran_voice_falls_back() {
    let mut v = voices();
    let mut out = Out {
        has: vec![
            "unit.select",
            "unit.select.veyran",
            "unit.move",
            "unit.move.veyran",
        ],
        original: vec!["unit.select"],
        ..Out::default()
    };
    assert!(v.speak(UnitVoice::Select, Faction::Veyran, 0, &mut out));
    assert!(v.speak(UnitVoice::Select, Faction::Meridian, 2000, &mut out));
    assert!(v.speak(UnitVoice::Move, Faction::Veyran, 4000, &mut out));
    assert_eq!(
        out.played,
        ["unit.select", "unit.select", "unit.move.veyran"]
    );
    // No Veyran events in the bank.
    let mut out = Out {
        has: vec!["unit.attack"],
        ..Out::default()
    };
    let mut v = voices();
    assert!(v.speak(UnitVoice::Attack, Faction::Veyran, 0, &mut out));
    assert_eq!(out.played, ["unit.attack"]);
}

/// Which side speaks for a list of units: the owner of the lowest id, in the production snapshot. A mixed list
/// takes the leading (lowest-id) unit; a neutral and a stale id are skipped; nobody gives `None`.
#[test]
fn the_faction_of_the_leading_unit_speaks_for_a_mixed_list() {
    let mut scene = Scene::new(false);
    let veyran = scene.unit("rifleman", VEYRAN, 20, 10);
    let meridian = scene.unit("rifleman", MERIDIAN, 10, 10);
    let civ = scene.unit("civilian", NEUTRAL, 16, 10);
    assert!(
        veyran < meridian && meridian < civ,
        "spawn order is id order"
    );
    let snap = scene.snap();
    let who = |ids: &[EntityId]| speaker_faction(&snap, ids);
    assert_eq!(who(&[meridian]), Some(Faction::Meridian));
    assert_eq!(who(&[veyran]), Some(Faction::Veyran));
    // Mixed: the lowest id leads, whatever the order of the list.
    assert_eq!(who(&[meridian, veyran]), Some(Faction::Veyran));
    assert_eq!(who(&[veyran, meridian]), Some(Faction::Veyran));
    // A neutral is skipped, even with the lowest id.
    assert_eq!(who(&[civ, meridian]), Some(Faction::Meridian));
    assert_eq!(who(&[civ]), None);
    assert_eq!(who(&[]), None);
    // A stale id (never spawned) has no side.
    let stale = EntityId::new(9_999, 7);
    assert_eq!(who(&[stale]), None);
    assert_eq!(who(&[stale, meridian]), Some(Faction::Meridian));
}

/// The units an order is given to are the ones whose side speaks.
#[test]
fn an_order_names_the_units_that_speak() {
    let mut scene = Scene::new(false);
    let a = scene.unit("rifleman", VEYRAN, 20, 10);
    let command = Command::Order {
        ids: vec![a],
        order: Order::Move(Vec2::from_ints(5, 5)),
        queued: false,
    };
    assert_eq!(order_units(&command), [a]);
    let other = Command::SpawnUnit {
        kind: Kind(0),
        pos: Vec2::from_ints(1, 1),
        owner: PlayerId(0),
    };
    assert!(order_units(&other).is_empty());
}

/// With a CC0 sound in the bank the event plays through the real `AudioSystem` (recorded by its
/// trace, no device), once under the gap and again after it.
#[test]
fn a_bank_sound_plays_through_the_audio_system() {
    let dir = std::env::temp_dir().join(format!("sf_unit_voice_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("audio")).unwrap();
    std::fs::write(
        dir.join("audio/bank.ron"),
        r#"(sounds: {
            "unit.select": (file: "audio/sfx/a.wav"),
            "unit.move": (file: "audio/sfx/b.wav"),
        })"#,
    )
    .unwrap();
    let mut audio = AudioSystem::silent(&dir);
    audio.set_trace(true);
    let mut v = voices();
    assert!(v.speak(UnitVoice::Select, Faction::Meridian, 0, &mut audio));
    assert!(
        !v.speak(UnitVoice::Move, Faction::Meridian, 100, &mut audio),
        "inside the gap"
    );
    assert!(
        !v.speak(UnitVoice::Attack, Faction::Meridian, 500, &mut audio),
        "no sound for attack"
    );
    assert!(v.speak(UnitVoice::Move, Faction::Meridian, 500, &mut audio));
    let names: Vec<String> = audio.take_trace().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["unit.select", "unit.move"]);
    std::fs::remove_dir_all(dir).ok();
}
