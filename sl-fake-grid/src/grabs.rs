//! What a drag and a spin do to a prim: the half of a held mouse button that
//! needs no script to answer.
//!
//! A viewer holding a button down on a prim says so with `ObjectGrab`, a
//! stream of `ObjectGrabUpdate`s and an `ObjectDeGrab`, and turns what it
//! holds with `ObjectSpinStart`, `ObjectSpinUpdate` and `ObjectSpinStop`.
//! What a *script* in the prim is told of them is the script engine's
//! (`server-world-touch-and-grab`). This module is the rest, as both live
//! grids were measured to do it (`object-touch-grab`, 2026-10-12, written up
//! in `book/src/gridspec/building.md` § Touch and grab):
//!
//! - **A drag moves a prim that is not physical**: every `ObjectGrabUpdate`
//!   puts the linkset's root at the grab position it states, and every
//!   `ObjectSpinUpdate` gives it the rotation it states — with or without
//!   an `ObjectGrab` before it. Everybody in the region is sent a terse
//!   update.
//! - **Unless a script in it takes touches**
//!   ([`prim_flags::HANDLE_TOUCH`] on the prim or on its root): then the held
//!   button is the script's, and the prim stays.
//! - **And only for somebody who may move it**: its owner, or anybody where
//!   its record lets everyone move it.
//! - **A physical prim is pulled, not placed** — by a force, which this grid
//!   has no physics to apply. It stays.
//!
//! The grids differ in one thing: OpenSim sends its terse update for every
//! `ObjectGrabUpdate` whether the prim moved or not, and Second Life only
//! when it did
//! ([`EditPolicy::refused_drag`](crate::imitates::EditPolicy::refused_drag)).

use std::time::Instant;

use sl_proto::{
    AgentKey, Object, Permissions, RegionLocalObjectId, ServerEvent, SimSession, prim_flags,
};
use sl_types::key::OwnerKey;
use sl_types::lsl::{Rotation, Vector};

use crate::object_edits::push_terse;
use crate::world::{RegionChange, SceneFixtures};

/// What a held button asks of the prim it holds.
enum Pull<'a> {
    /// To be at a place.
    To(&'a Vector),
    /// To stand a way.
    Turned(&'a Rotation),
}

/// Answers one drained [`ServerEvent`] that is an update of a held button,
/// returning the [`RegionChange`]s the region's other sessions have to be
/// told about — or [`None`] when the event is not one.
pub(crate) fn answer_grab(
    world: &mut SceneFixtures,
    agent: AgentKey,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    let (object, pull) = match event {
        ServerEvent::ObjectGrabMoved(update) => (update.object_id, Pull::To(&update.grab_position)),
        ServerEvent::SpinObjectUpdate {
            object_id,
            rotation,
        } => (*object_id, Pull::Turned(rotation)),
        _ => return None,
    };
    let Some(local_id) = world.local_id_of(object) else {
        tracing::debug!("a held button named {object}, which this region does not have");
        return Some(Vec::new());
    };
    let root = world.root_of(local_id);
    if follows(world, agent, local_id, root)
        && let Some(object) = world.object_mut(root)
    {
        match pull {
            Pull::To(position) => object.motion.position.clone_from(position),
            Pull::Turned(rotation) => object.motion.rotation.clone_from(rotation),
        }
        let moved = object.clone();
        push_terse(sim, &moved, now);
        return Some(vec![RegionChange::Moved(Box::new(moved))]);
    }
    if world.edit.refused_drag == crate::imitates::RefusedDrag::Resent
        && let Some(object) = world.object_by_local_id(root)
    {
        push_terse(sim, &object, now);
        return Some(vec![RegionChange::Moved(Box::new(object))]);
    }
    Some(Vec::new())
}

/// Whether the linkset `root`, held by its prim `local_id`, goes where
/// `agent`'s held button asks it to.
fn follows(
    world: &SceneFixtures,
    agent: AgentKey,
    local_id: RegionLocalObjectId,
    root: RegionLocalObjectId,
) -> bool {
    let flags_of = |id| {
        world
            .object_by_local_id(id)
            .map_or(0, |object: Object| object.update_flags)
    };
    let flags = flags_of(local_id) | flags_of(root);
    if flags & (prim_flags::HANDLE_TOUCH | prim_flags::USE_PHYSICS) != 0 {
        return false;
    }
    world.record_of(root).is_some_and(|record| {
        record.owner == OwnerKey::Agent(agent)
            || record.permissions.everyone.contains(Permissions::MOVE)
    })
}
