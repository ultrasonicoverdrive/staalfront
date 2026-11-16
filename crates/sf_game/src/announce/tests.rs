//! The moments, the brake and the priorities of the announcements (WP11.2c). `changes` and `deaths` are
//! the production functions; the facts are stated by hand here and read from real worlds in
//! `crates/sf_game/tests/announcements.rs`. The runner's call (`Facts::read` per new tick, `deaths` of the
//! shown events, `speak` once a frame) needs a window and is checked by reading, see
//! `docs/rules/announcements.md`.

use std::path::{Path, PathBuf};

use super::*;
use sf_audio::{AudioConfig, AudioSystem};

const ME: PlayerId = PlayerId(0);

fn id(n: u32) -> EntityId {
    EntityId::new(n, 0)
}

fn config() -> AnnounceConfig {
    AudioConfig::embedded().announce
}

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

/// A stand-in for the audio: which events have a sound, whether a voice is sounding, what was played.
struct Out {
    silent: Vec<&'static str>,
    busy: bool,
    played: Vec<String>,
}

impl Out {
    fn new() -> Self {
        Out {
            silent: Vec::new(),
            busy: false,
            played: Vec::new(),
        }
    }
}

impl AnnounceOut for Out {
    fn has_sound(&self, event: &str) -> bool {
        !self.silent.contains(&event)
    }
    fn busy(&self) -> bool {
        self.busy
    }
    fn play(&mut self, event: &str) {
        self.played.push(event.to_string());
    }
}

fn unit(n: u32, hp: u32) -> Health {
    Health {
        id: id(n),
        hp,
        structure: false,
        harvester: false,
    }
}

fn structure(n: u32, hp: u32) -> Health {
    Health {
        id: id(n),
        hp,
        structure: true,
        harvester: false,
    }
}

fn harvester(n: u32, hp: u32) -> Health {
    Health {
        id: id(n),
        hp,
        structure: false,
        harvester: true,
    }
}

fn factory(n: u32, items: usize) -> FactoryFact {
    FactoryFact {
        id: id(n),
        items,
        holding: false,
        waiting_for_money: false,
    }
}

/// The announcements of the change from `prev` to `now`.
fn said(prev: &Facts, now: &Facts) -> Vec<Announcement> {
    changes(prev, now)
}

/// The events are the sound events of the bank, as literals, in this order: a renumbering or a rename fails.
#[test]
fn the_19_events_are_pinned() {
    let names: Vec<&str> = Announcement::ALL.iter().map(|a| a.event()).collect();
    assert_eq!(
        names,
        [
            "announce.structure_ready",
            "announce.unit_ready",
            "announce.building",
            "announce.training",
            "announce.on_hold",
            "announce.cancelled",
            "announce.base_under_attack",
            "announce.harvester_under_attack",
            "announce.low_power",
            "announce.insufficient_funds",
            "announce.storage_full",
            "announce.unit_lost",
            "announce.structure_lost",
            "announce.new_options",
            "announce.cannot_deploy",
            "announce.victory",
            "announce.defeat",
            "announce.structure_sold",
            "announce.repairing",
        ]
    );
}

/// One voice at a time covers the unit voices and the announcements: the 22 names `busy` asks about.
#[test]
fn the_voice_channel_holds_the_unit_voices_and_every_announcement() {
    let c = voice_channel();
    assert_eq!(c.len(), 22);
    assert_eq!(&c[..3], ["unit.select", "unit.move", "unit.attack"]);
    for a in Announcement::ALL {
        assert!(c.contains(&a.event()), "{}", a.event());
    }
}

/// Every event of the enum has a rule in the shipped config, a sound in the shipped bank and a line on disk.
#[test]
fn the_shipped_config_and_bank_know_every_event() {
    let c = config();
    let audio = AudioSystem::silent(assets());
    for a in Announcement::ALL {
        assert!(c.events.contains_key(a.event()), "{}: no rule", a.event());
        assert!(audio.has_sound(a.event()), "{}: no sound", a.event());
    }
    assert_eq!(c.events.len(), 19);
}

// ---- the moments -------------------------------------------------------------------------------------

#[test]
fn a_structure_that_goes_up_says_building_and_then_structure_ready() {
    let before = Facts {
        health: vec![structure(1, 100)],
        ..Facts::default()
    };
    let going_up = Facts {
        health: vec![structure(1, 100), structure(2, 10)],
        constructions: vec![id(2)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &going_up), [Announcement::Building]);
    // Still going up: nothing more.
    assert_eq!(said(&going_up, &going_up), []);
    let standing = Facts {
        health: vec![structure(1, 100), structure(2, 100)],
        ..Facts::default()
    };
    assert_eq!(said(&going_up, &standing), [Announcement::StructureReady]);
    // It vanished instead (destroyed while going up): not ready.
    assert_eq!(said(&going_up, &before), []);
    // It is being sold the moment it completes: not ready.
    let selling = Facts {
        sales: vec![id(2)],
        ..standing.clone()
    };
    assert_eq!(
        said(&going_up, &selling),
        [Announcement::StructureSold],
        "a sale is not a completion"
    );
}

#[test]
fn a_unit_that_leaves_a_factory_says_unit_ready() {
    let before = Facts {
        units: vec![id(5)],
        factories: vec![factory(1, 1)],
        ..Facts::default()
    };
    let after = Facts {
        units: vec![id(5), id(6)],
        factories: vec![factory(1, 0)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &after), [Announcement::UnitReady]);
    // A repeating factory refills at once: the unit is still announced.
    let refilled = Facts {
        factories: vec![factory(1, 1)],
        ..after.clone()
    };
    assert_eq!(said(&before, &refilled), [Announcement::UnitReady]);
    assert_eq!(said(&after, &after), []);
}

#[test]
fn queueing_says_training_and_taking_the_unit_out_says_cancelled() {
    let empty = Facts {
        factories: vec![factory(1, 0)],
        ..Facts::default()
    };
    let one = Facts {
        factories: vec![factory(1, 1)],
        ..Facts::default()
    };
    assert_eq!(said(&empty, &one), [Announcement::Training]);
    assert_eq!(said(&one, &empty), [Announcement::Cancelled]);
    // The same count: nothing.
    assert_eq!(said(&one, &one), []);
    // A factory that is gone (not in `now`) is not a cancel: its structure was destroyed or sold.
    assert_eq!(said(&one, &Facts::default()), []);
    // A new factory with a queue has nothing before it: that is training too (the queue grew from nothing).
    assert_eq!(said(&Facts::default(), &one), [Announcement::Training]);
}

#[test]
fn a_free_item_is_not_training() {
    // `Facts::read` counts paid items only (the harvester a refinery brings is free), so the free item
    // leaving the queue with a new unit is a unit ready and no cancel.
    let before = Facts {
        factories: vec![factory(1, 0)],
        ..Facts::default()
    };
    let after = Facts {
        units: vec![id(9)],
        factories: vec![factory(1, 0)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &after), [Announcement::UnitReady]);
}

#[test]
fn a_factory_that_starts_to_hold_says_on_hold_once() {
    let waiting = FactoryFact {
        holding: true,
        ..factory(1, 1)
    };
    let before = Facts {
        factories: vec![factory(1, 1)],
        ..Facts::default()
    };
    let now = Facts {
        factories: vec![waiting],
        ..Facts::default()
    };
    assert_eq!(said(&before, &now), [Announcement::OnHold]);
    assert_eq!(said(&now, &now), [], "still holding: said already");
}

#[test]
fn running_out_of_money_says_insufficient_funds_when_it_starts() {
    let poor = FactoryFact {
        waiting_for_money: true,
        ..factory(1, 1)
    };
    let ok = Facts {
        factories: vec![factory(1, 1)],
        ..Facts::default()
    };
    let short = Facts {
        factories: vec![poor],
        ..Facts::default()
    };
    assert_eq!(said(&ok, &short), [Announcement::InsufficientFunds]);
    assert_eq!(said(&short, &short), []);
    // A structure going up that waits for money counts too.
    let site = Facts {
        construction_waits_for_money: true,
        ..Facts::default()
    };
    assert_eq!(said(&ok, &site), [Announcement::InsufficientFunds]);
    // A second one while the first still waits is not news.
    let both = Facts {
        construction_waits_for_money: true,
        ..short.clone()
    };
    assert_eq!(said(&short, &both), []);
}

#[test]
fn low_power_and_storage_full_are_said_when_the_state_starts() {
    let ok = Facts::default();
    let low = Facts {
        low_power: true,
        ..Facts::default()
    };
    assert_eq!(said(&ok, &low), [Announcement::LowPower]);
    assert_eq!(said(&low, &low), []);
    assert_eq!(said(&low, &ok), []);
    let full = Facts {
        storage_full: true,
        ..Facts::default()
    };
    assert_eq!(said(&ok, &full), [Announcement::StorageFull]);
    assert_eq!(said(&full, &full), []);
}

#[test]
fn losing_hit_points_says_base_or_harvester_under_attack() {
    let before = Facts {
        health: vec![structure(1, 400), harvester(2, 200), unit(3, 50)],
        ..Facts::default()
    };
    let base_hit = Facts {
        health: vec![structure(1, 390), harvester(2, 200), unit(3, 50)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &base_hit), [Announcement::BaseUnderAttack]);
    let harvester_hit = Facts {
        health: vec![structure(1, 400), harvester(2, 150), unit(3, 50)],
        ..Facts::default()
    };
    assert_eq!(
        said(&before, &harvester_hit),
        [Announcement::HarvesterUnderAttack]
    );
    // A plain unit that is hit says nothing (it is a fight, not the base), and more hit points is not an attack
    // (a repair).
    let unit_hit = Facts {
        health: vec![structure(1, 400), harvester(2, 200), unit(3, 10)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &unit_hit), []);
    let repaired = Facts {
        health: vec![structure(1, 450), harvester(2, 200), unit(3, 50)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &repaired), []);
    // A structure that is new (not in `before`) has nothing to compare with.
    let new = Facts {
        health: vec![structure(1, 400), structure(4, 1)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &new), []);
    // Hit points that go while it is being sold are not an attack.
    let selling = Facts {
        health: vec![structure(1, 100)],
        sales: vec![id(1)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &selling), [Announcement::StructureSold]);
}

#[test]
fn a_sale_and_a_repair_that_start_are_said() {
    let before = Facts {
        health: vec![structure(1, 400)],
        ..Facts::default()
    };
    let sale = Facts {
        sales: vec![id(1)],
        ..before.clone()
    };
    assert_eq!(said(&before, &sale), [Announcement::StructureSold]);
    assert_eq!(said(&sale, &sale), []);
    let repair = Facts {
        repairs: vec![id(1)],
        ..before.clone()
    };
    assert_eq!(said(&before, &repair), [Announcement::Repairing]);
    assert_eq!(said(&repair, &repair), []);
}

#[test]
fn the_end_of_the_match_says_victory_or_defeat_once_and_a_draw_nothing() {
    let on = Facts::default();
    let won = Facts {
        standing: Some(Standing::Won),
        ..Facts::default()
    };
    let lost = Facts {
        standing: Some(Standing::Lost),
        ..Facts::default()
    };
    let draw = Facts {
        standing: Some(Standing::Draw),
        ..Facts::default()
    };
    assert_eq!(said(&on, &won), [Announcement::Victory]);
    assert_eq!(said(&on, &lost), [Announcement::Defeat]);
    assert_eq!(said(&on, &draw), []);
    assert_eq!(said(&won, &won), [], "once");
}

#[test]
fn only_the_own_deaths_count() {
    let death = |player: u8, structure: bool| FxEvent {
        player,
        kind: FxKind::Death {
            wreck: false,
            structure,
        },
        ..FxEvent::default()
    };
    let shot = FxEvent {
        player: 0,
        ..FxEvent::default()
    };
    assert_eq!(deaths(&[death(0, false)], ME), [Announcement::UnitLost]);
    assert_eq!(deaths(&[death(0, true)], ME), [Announcement::StructureLost]);
    assert_eq!(
        deaths(&[death(0, true), death(0, false), death(0, false)], ME),
        [Announcement::UnitLost, Announcement::StructureLost]
    );
    // The opponent's, a neutral's and a shot of ours are not losses.
    assert_eq!(deaths(&[death(1, false), death(1, true)], ME), []);
    assert_eq!(deaths(&[death(255, false), shot], ME), []);
}

// ---- the brake ---------------------------------------------------------------------------------------

/// The announcer with the shipped numbers, settled (nothing to settle: tests call `raise`).
fn announcer() -> Announcer {
    Announcer::new(config())
}

#[test]
fn an_announcement_plays_once_the_voices_are_quiet() {
    let mut a = announcer();
    let mut out = Out::new();
    assert!(a.raise(Announcement::UnitReady, 0));
    out.busy = true;
    assert_eq!(a.speak(10, &mut out), None, "a voice is sounding");
    assert_eq!(a.pending(), [Announcement::UnitReady], "it waits");
    out.busy = false;
    assert_eq!(a.speak(20, &mut out), Some(Announcement::UnitReady));
    assert_eq!(out.played, ["announce.unit_ready"]);
    assert!(a.pending().is_empty());
}

#[test]
fn only_one_announcement_starts_at_a_time_and_the_min_gap_holds() {
    let mut a = announcer();
    let mut out = Out::new();
    a.raise(Announcement::UnitReady, 0);
    a.raise(Announcement::Training, 0);
    assert!(a.speak(0, &mut out).is_some());
    assert_eq!(a.speak(1, &mut out), None, "one per call");
    assert_eq!(a.speak(1999, &mut out), None, "min_gap_ms is 2000");
    assert!(a.speak(2000, &mut out).is_some());
    assert_eq!(out.played.len(), 2);
}

#[test]
fn the_same_announcement_is_not_queued_twice() {
    let mut a = announcer();
    assert!(a.raise(Announcement::UnitReady, 0));
    assert!(!a.raise(Announcement::UnitReady, 5));
    assert!(!a.raise(Announcement::UnitReady, 10));
    assert_eq!(a.pending(), [Announcement::UnitReady]);
    assert_eq!(a.stats().suppressed, 2);
}

#[test]
fn an_event_is_not_raised_again_inside_its_repeat_gap_and_attacks_have_a_longer_one() {
    let mut a = announcer();
    let mut out = Out::new();
    a.raise(Announcement::BaseUnderAttack, 0);
    assert!(a.speak(0, &mut out).is_some());
    // 19.999 s later it is still the same attack: not raised.
    assert!(!a.raise(Announcement::BaseUnderAttack, 19_999));
    assert!(a.raise(Announcement::BaseUnderAttack, 20_000), "20 s gap");
    // The ordinary line has the short gap (2 s).
    a.raise(Announcement::UnitReady, 30_000);
    assert!(a.speak(30_000, &mut out).is_some());
    assert!(!a.raise(Announcement::UnitReady, 31_999));
    assert!(a.raise(Announcement::UnitReady, 32_000));
}

#[test]
fn the_most_important_goes_first_and_equals_go_in_the_order_raised() {
    let mut a = announcer();
    let mut out = Out::new();
    a.raise(Announcement::Training, 0);
    a.raise(Announcement::UnitReady, 1);
    a.raise(Announcement::BaseUnderAttack, 2);
    a.raise(Announcement::Building, 3);
    a.speak(10, &mut out);
    a.speak(2010, &mut out);
    // Base under attack (8) before unit ready (3); training and building (both 2) wait, in the order raised.
    assert_eq!(
        out.played,
        ["announce.base_under_attack", "announce.unit_ready"]
    );
    assert_eq!(
        a.pending(),
        [Announcement::Training, Announcement::Building]
    );
}

#[test]
fn a_full_queue_drops_the_lowest_priority_and_the_oldest_of_those() {
    let mut a = announcer();
    for (i, x) in [
        Announcement::Training,
        Announcement::Building,
        Announcement::Cancelled,
        Announcement::UnitReady,
    ]
    .into_iter()
    .enumerate()
    {
        a.raise(x, i as u64);
    }
    assert_eq!(a.pending().len(), 4);
    // A fifth, more important one: the oldest of the lowest priority (training, 2) goes.
    a.raise(Announcement::BaseUnderAttack, 10);
    assert_eq!(
        a.pending(),
        [
            Announcement::Building,
            Announcement::Cancelled,
            Announcement::UnitReady,
            Announcement::BaseUnderAttack
        ]
    );
    assert_eq!(a.stats().dropped, 1);
    // Another of the lowest priority: again the oldest of them (building) goes.
    a.raise(Announcement::Repairing, 11);
    assert_eq!(
        a.pending(),
        [
            Announcement::Cancelled,
            Announcement::UnitReady,
            Announcement::BaseUnderAttack,
            Announcement::Repairing
        ]
    );
    assert_eq!(a.stats().dropped, 2);
}

#[test]
fn old_news_is_dropped() {
    let mut a = announcer();
    let mut out = Out::new();
    out.busy = true;
    a.raise(Announcement::UnitReady, 0);
    assert_eq!(a.speak(4000, &mut out), None);
    assert_eq!(a.pending().len(), 1, "4000 ms is the limit, still there");
    assert_eq!(a.speak(4001, &mut out), None);
    assert!(a.pending().is_empty(), "waited longer than max_wait_ms");
    assert_eq!(a.stats().dropped, 1);
    out.busy = false;
    assert_eq!(a.speak(4002, &mut out), None);
    assert!(out.played.is_empty());
}

#[test]
fn an_event_without_a_sound_is_silent_and_uses_up_no_gap() {
    let mut a = announcer();
    let mut out = Out::new();
    out.silent.push("announce.unit_ready");
    a.raise(Announcement::UnitReady, 0);
    a.raise(Announcement::Training, 0);
    // The silent one goes without a word, the other plays in the same call.
    assert_eq!(a.speak(0, &mut out), Some(Announcement::Training));
    assert_eq!(out.played, ["announce.training"]);
    // It used no gap and no repeat gap: raised again at once it queues (and is silent again).
    assert!(a.raise(Announcement::UnitReady, 1));
    assert_eq!(a.stats().dropped, 0, "not counted as waited too long");
}

#[test]
fn an_announcement_does_not_cut_off_or_talk_over_a_voice() {
    // `busy` is what `AudioSystem::is_playing(voice_channel())` says: a unit voice or an announcement.
    let mut a = announcer();
    let mut out = Out::new();
    out.busy = true;
    a.raise(Announcement::BaseUnderAttack, 0);
    for t in [0, 500, 1000, 3000] {
        assert_eq!(a.speak(t, &mut out), None, "{t}");
    }
    assert!(out.played.is_empty());
    out.busy = false;
    assert!(a.speak(3500, &mut out).is_some());
}

#[test]
fn nothing_is_said_while_the_game_settles_and_the_baseline_is_the_state_at_its_end() {
    let mut a = announcer();
    let tick = |t: u64, f: Facts| Facts { tick: t, ..f };
    let low = Facts {
        low_power: true,
        options: vec![Kind(7)],
        ..Facts::default()
    };
    // The starting objects arrive: low power from the first facts, an option, in the settling time.
    a.observe(tick(1, Facts::default()), 50);
    a.observe(tick(50, low.clone()), 2500);
    a.observe(tick(100, low.clone()), 5000);
    assert!(a.pending().is_empty(), "settling: nothing raised");
    // After it: the same state is not news (the baseline), a new option is.
    a.observe(
        tick(
            101,
            Facts {
                options: vec![Kind(7), Kind(9)],
                ..low.clone()
            },
        ),
        5050,
    );
    // first tick 1 + 100 settle ticks = 101 is the first compared one.
    assert_eq!(a.pending(), [Announcement::NewOptions]);
}

#[test]
fn an_option_is_new_once_per_game_even_if_it_goes_and_comes_back() {
    let mut a = announcer();
    let mut out = Out::new();
    let at = |t: u64, options: Vec<Kind>| Facts {
        tick: t,
        options,
        ..Facts::default()
    };
    a.observe(at(0, vec![Kind(1)]), 0);
    a.observe(at(100, vec![Kind(1)]), 5000);
    a.observe(at(101, vec![Kind(1), Kind(2)]), 5050);
    assert_eq!(a.pending(), [Announcement::NewOptions]);
    a.speak(5100, &mut out);
    // The structure that gave option 2 is lost and rebuilt: option 2 is not new again.
    a.observe(at(200, vec![Kind(1)]), 10_000);
    a.observe(at(300, vec![Kind(1), Kind(2)]), 20_000);
    assert!(a.pending().is_empty());
    // A reset (a new game) forgets what was seen.
    a.reset();
    a.observe(at(0, vec![Kind(1)]), 0);
    a.observe(at(100, vec![Kind(1)]), 5000);
    a.observe(at(101, vec![Kind(1), Kind(2)]), 5050);
    assert_eq!(a.pending(), [Announcement::NewOptions]);
}

#[test]
fn observe_raises_what_changes_gives_and_a_reset_forgets_it_all() {
    let mut a = announcer();
    let t = |tick: u64, f: Facts| Facts { tick, ..f };
    a.observe(t(0, Facts::default()), 0);
    a.observe(t(100, Facts::default()), 5000);
    a.observe(
        t(
            101,
            Facts {
                low_power: true,
                ..Facts::default()
            },
        ),
        5050,
    );
    assert_eq!(a.pending(), [Announcement::LowPower]);
    assert!(!a.is_new_tick(101) && a.is_new_tick(102));
    a.reset();
    assert!(a.pending().is_empty());
    assert!(a.is_new_tick(101), "no baseline after a reset");
    assert_eq!(a.stats(), AnnounceStats::default());
}

#[test]
fn the_index_of_an_event_is_its_place_in_all() {
    for (i, a) in Announcement::ALL.iter().enumerate() {
        assert_eq!(a.index(), i, "{}", a.event());
    }
}

/// Everything raised in one `observe` has the same time: of equal priority the first raised is said first
/// (structure ready and unit ready are both priority 3).
#[test]
fn equal_priorities_in_the_same_millisecond_go_in_the_order_raised() {
    let mut a = announcer();
    let mut out = Out::new();
    a.raise(Announcement::StructureReady, 500);
    a.raise(Announcement::UnitReady, 500);
    assert_eq!(a.speak(500, &mut out), Some(Announcement::StructureReady));
    assert_eq!(a.speak(2500, &mut out), Some(Announcement::UnitReady));
    // The same through `observe`: one change gives both, in the order of `changes`.
    let mut a = announcer();
    let mut out = Out::new();
    let going_up = Facts {
        health: vec![structure(1, 100), structure(2, 10)],
        constructions: vec![id(2)],
        ..Facts::default()
    };
    let after = Facts {
        tick: 101,
        health: vec![structure(1, 100), structure(2, 100), unit(3, 50)],
        units: vec![id(3)],
        ..Facts::default()
    };
    a.observe(going_up.clone(), 0);
    a.observe(
        Facts {
            tick: 100,
            ..going_up
        },
        5000,
    );
    a.observe(after, 5050);
    assert_eq!(
        a.pending(),
        [Announcement::StructureReady, Announcement::UnitReady]
    );
    assert_eq!(a.speak(5050, &mut out), Some(Announcement::StructureReady));
}

/// A finished refinery: "Structure ready" before "Storage full" although the full store was raised first.
#[test]
fn storage_full_is_said_after_structure_ready() {
    let mut a = announcer();
    let mut out = Out::new();
    a.raise(Announcement::StorageFull, 0);
    a.raise(Announcement::StructureReady, 0);
    assert_eq!(a.speak(0, &mut out), Some(Announcement::StructureReady));
    assert_eq!(a.speak(2000, &mut out), Some(Announcement::StorageFull));
}

/// The grace covers the direct raises too (a death, a refused placement): nothing in the settling time.
#[test]
fn a_death_or_a_refusal_in_the_settling_time_says_nothing() {
    let mut a = announcer();
    // Before the first facts: the game has not started to settle.
    assert!(!a.raise_live(Announcement::CannotDeploy, 0));
    let at = |t: u64| Facts {
        tick: t,
        ..Facts::default()
    };
    a.observe(at(10), 500);
    assert!(!a.raise_live(Announcement::UnitLost, 600), "tick 10");
    a.observe(at(109), 5400);
    assert!(!a.raise_live(Announcement::CannotDeploy, 5400), "tick 109");
    assert!(a.pending().is_empty());
    // Settled at the 100th tick after the first (110): now they are heard.
    a.observe(at(110), 5450);
    assert!(a.raise_live(Announcement::CannotDeploy, 5450));
    assert_eq!(a.pending(), [Announcement::CannotDeploy]);
    // A new game settles again.
    a.reset();
    assert!(!a.raise_live(Announcement::UnitLost, 6000));
}

/// A factory that is being sold (or has no hit points) is not a standing factory: its queue going away with
/// it is not a cancel (`Facts::read` leaves it out; here the comparison of the lists without it).
#[test]
fn a_factory_that_is_gone_from_the_facts_is_not_a_cancel() {
    let before = Facts {
        factories: vec![factory(1, 2)],
        ..Facts::default()
    };
    assert_eq!(said(&before, &Facts::default()), []);
}
