//! The turret (WP4.5): a facing of its own, turning apart from the hull.
//!
//! A kind with a `turret_rate` in `unit_defs.ron` ([`UnitStats::turret_per_tick`])
//! has a turret facing next to its hull facing. It is an absolute direction in the
//! world, not an angle relative to the hull, so that a turret that is tracking a
//! target keeps tracking while the hull turns under it; the renderer subtracts
//! the hull's facing to get the angle of the glTF `turret` part's yaw joint.
//!
//! Every tick, after the units have moved ([`update_all`]), each turret turns
//! toward where it wants to point, by at most its rate, the short way round
//! ([`sf_math::Facing::turn_toward`]: the decreasing way when the target is
//! exactly behind, the fixed tie-break at 128):
//!
//! * at the unit named by the **turret target** column, if that unit is alive
//!   (set by `Command::AimTurret`; phase 5 target selection writes the same
//!   column, see [`would_acquire_turret_target_here`]); a target that is gone
//!   clears the column;
//! * else at the victim of an `Attack` order, from the moment the order is given,
//!   also while the hull is still on its way;
//! * else along the hull: the turret returns to hull-forward and then follows the
//!   hull as it turns, with the lag of its own rate.
//!
//! A unit whose turret is at its target does not turn its hull toward the target
//! when it is in reach (`orders::run`, `Order::Attack`): the turret does that.
//!
//! The barrel's pitch is presentation only for now. The renderer's hook is
//! `Motion::with_barrel_pitch` in `sf_render`; nothing in the simulation decides
//! it.

use sf_math::Facing;

use crate::entities::{Entities, NO_DEF};
use crate::orders::{Order, live_slot};
use crate::units::UnitTable;

/// Whether entity `i` has a turret.
pub(crate) fn has_turret(e: &Entities, i: usize, units: &UnitTable) -> bool {
    e.def[i] != NO_DEF && units.at(e.def[i] as usize).has_turret()
}

/// The hook for phase 5: target selection puts the unit a turret should aim at
/// into the `turret_target` column from here. Called every tick for a turret
/// that has nothing to aim at; it does nothing yet.
pub(crate) fn would_acquire_turret_target_here(_e: &Entities, _i: usize) {}

/// The direction entity `i`'s turret should point in: at its target, at the
/// victim of its attack order, or along the hull.
fn wanted(e: &mut Entities, i: usize) -> Facing {
    if let Some(id) = e.turret_target[i] {
        match live_slot(e, id) {
            Some(j) if j != i => {
                if let Some(at) = (e.pos[j] - e.pos[i]).facing() {
                    return at;
                }
                return e.turret[i];
            }
            _ => e.turret_target[i] = None,
        }
    }
    if let Order::Attack(victim) = e.order[i] {
        if let Some(j) = live_slot(e, victim)
            && let Some(at) = (e.pos[j] - e.pos[i]).facing()
        {
            return at;
        }
    } else {
        would_acquire_turret_target_here(e, i);
    }
    e.facing[i]
}

/// Runs one tick for every turret, in ascending slot order, after the hulls have
/// moved. Also records the previous turret facing for interpolation.
pub(crate) fn update_all(e: &mut Entities, units: &UnitTable) {
    for i in 0..e.len() {
        if !e.alive[i] || !has_turret(e, i, units) {
            continue;
        }
        e.prev_turret[i] = e.turret[i];
        let rate = units.at(e.def[i] as usize).turret_per_tick;
        let aim = wanted(e, i);
        e.turret[i] = e.turret[i].turn_toward(aim, rate);
    }
}
