//! The update scheduler: what a heartbeat tick changed in the region's
//! prims, told to its viewers as one burst.
//!
//! A change a **session** makes — an edit with the build floater, a drag, a
//! rez — is published by that session the moment it is made, as a
//! [`RegionChange`] per change. That is right at the speed a person edits, and
//! it stays so. A change the **region** makes on its own clock is different:
//! a script can set a prim's position, then its rotation, then its colour in
//! one event handler, and move it again every tick after. Told one message
//! per change, that is three full `ObjectUpdate`s where a simulator sends one,
//! and a full update a tick where it sends a terse one.
//!
//! So the region's tick is bracketed:
//!
//! 1. `begin` records every prim written since the last tick as shown —
//!    whoever wrote it outside a tick was a session, and it told the region
//!    itself;
//! 2. the tick runs (the scripts' slice and what it applies to the world);
//! 3. `finish` takes the object changes the tick announced and the prims
//!    the store says were written during it (`ObjectStore::take_written`),
//!    coalesces them to one entry per prim, and replaces them with a single
//!    [`RegionChange::Burst`].
//!
//! # Terse or full
//!
//! Each prim in a burst is compared with what the region's viewers were last
//! shown of it (`ObjectStore::shown`). A prim that differs only in its
//! placement — position, velocity, acceleration, rotation, spin, and the
//! state byte a terse update also carries — goes out **terse**, the form a
//! viewer's dead reckoning is built for. A prim seen for the first time, or
//! changed in anything a terse update cannot carry (its faces, shape, flags,
//! text, particles, name-values), goes out **full**. A prim written and left
//! as it was shown is not sent at all.
//!
//! What the tick *announced* still counts, because the grid sometimes tells
//! of a prim that did not change: a link re-sends every prim of the set, as
//! both live grids do. An announced full update stays full, an announced move
//! is sent terse at least, and a removal is a kill — and a prim removed in the
//! tick is not updated in it. A write the grid deliberately keeps quiet
//! amends what was shown instead (`ObjectStore::amend_shown`), so the
//! burst does not announce it either; objects put on an avatar travel in their
//! own [`RegionChange::Worn`] and are left out.
//!
//! # Order
//!
//! A burst names prims in the order the tick announced them, then the ones
//! only the store knew were written, in the store's order
//! ([`crate::scene_store`]), so two runs of a seed send the same bytes. Each
//! session's watcher sends the full updates as one `ObjectUpdate`, the terse
//! ones as one `ImprovedTerseObjectUpdate` and the kills as one
//! `KillObject`, in that order.
//!
//! # Per session
//!
//! A burst is published once to the region and filtered per session by its
//! watcher (`driver.rs`): another avatar's worn object is not shown. That
//! filter is the seam for what a simulator also does per viewer and the fake
//! grid does not yet — culling by distance, prioritising by size on screen,
//! and holding to the viewer's `AgentThrottle`.

use std::collections::BTreeSet;

use sl_proto::{Object, RegionLocalObjectId};

use crate::scene_store::ObjectStore;
use crate::world::{RegionChange, SceneFixtures};

/// What one tick changed in the region's prims, coalesced to one entry per
/// prim ([`RegionChange::Burst`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpdateBurst {
    /// The prims to send in full, as they stand at the end of the tick.
    pub full: Vec<Object>,
    /// The prims to send terse: only their placement changed.
    pub terse: Vec<Object>,
    /// The prims that left the region.
    pub killed: Vec<RegionLocalObjectId>,
}

impl UpdateBurst {
    /// Whether the burst names no prim.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.full.is_empty() && self.terse.is_empty() && self.killed.is_empty()
    }
}

/// How a prim goes out in a burst. A full update says everything a terse one
/// does, so the stronger form wins where a prim is named twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Form {
    /// An `ImprovedTerseObjectUpdate` entry.
    Terse,
    /// An `ObjectUpdate` entry.
    Full,
}

/// A prim the tick announced: the strongest form it was announced in, and the
/// record it was announced with, for a prim the store does not hold (an NPC's
/// body or attachment).
#[derive(Debug)]
struct Announced {
    /// The prim.
    local_id: RegionLocalObjectId,
    /// The strongest form it was announced in.
    form: Form,
    /// The record it was last announced with.
    record: Object,
}

/// Opens a tick: every prim written since the last one was written by a
/// session, which told the region of it itself, so it is recorded as shown.
pub(crate) fn begin(world: &mut SceneFixtures) {
    for local_id in world.objects.take_written() {
        world.objects.mark_shown(local_id);
    }
}

/// Closes a tick: the object changes among `changes` and the prims written
/// since [`begin`], as one [`RegionChange::Burst`] after every other change,
/// which keep their order.
pub(crate) fn finish(world: &mut SceneFixtures, changes: Vec<RegionChange>) -> Vec<RegionChange> {
    let mut announced: Vec<Announced> = Vec::new();
    let mut killed: Vec<RegionLocalObjectId> = Vec::new();
    let mut worn: BTreeSet<RegionLocalObjectId> = BTreeSet::new();
    let mut others = Vec::with_capacity(changes.len());
    for change in changes {
        match change {
            RegionChange::Rezzed(object) | RegionChange::Updated(object) => {
                announce(&mut announced, *object, Form::Full);
            }
            RegionChange::Moved(object) => announce(&mut announced, *object, Form::Terse),
            RegionChange::Killed(local_id) => {
                if !killed.contains(&local_id) {
                    killed.push(local_id);
                }
            }
            RegionChange::Worn { agent, objects } => {
                worn.extend(objects.iter().map(|object| object.local_id));
                others.push(RegionChange::Worn { agent, objects });
            }
            other => others.push(other),
        }
    }
    let written = world.objects.take_written();
    let mut order: Vec<RegionLocalObjectId> =
        announced.iter().map(|entry| entry.local_id).collect();
    order.extend(
        written
            .into_iter()
            .filter(|local_id| !announced.iter().any(|entry| entry.local_id == *local_id)),
    );
    let mut burst = UpdateBurst {
        killed,
        ..UpdateBurst::default()
    };
    for local_id in order {
        if burst.killed.contains(&local_id) {
            continue;
        }
        if worn.contains(&local_id) {
            world.objects.mark_shown(local_id);
            continue;
        }
        let hint = announced.iter().find(|entry| entry.local_id == local_id);
        let sent = match world.objects.get(local_id).cloned() {
            Some(now) => {
                let form = form_of(&world.objects, &now, hint.map(|entry| entry.form));
                world.objects.mark_shown(local_id);
                form.map(|form| (form, now))
            }
            None => hint.map(|entry| (entry.form, entry.record.clone())),
        };
        match sent {
            Some((Form::Full, object)) => burst.full.push(object),
            Some((Form::Terse, object)) => burst.terse.push(object),
            None => {}
        }
    }
    if !burst.is_empty() {
        others.push(RegionChange::Burst(Box::new(burst)));
    }
    others
}

/// Notes `object` as announced in `form`, keeping the first place it was
/// announced in and the strongest form.
fn announce(announced: &mut Vec<Announced>, object: Object, form: Form) {
    if let Some(entry) = announced
        .iter_mut()
        .find(|entry| entry.local_id == object.local_id)
    {
        entry.form = entry.form.max(form);
        entry.record = object;
        return;
    }
    announced.push(Announced {
        local_id: object.local_id,
        form,
        record: object,
    });
}

/// How the prim `now` goes out, given what the region's viewers were shown of
/// it and how the tick announced it, or `None` where it need not be sent.
fn form_of(objects: &ObjectStore, now: &Object, announced: Option<Form>) -> Option<Form> {
    if announced == Some(Form::Full) {
        return Some(Form::Full);
    }
    let Some(shown) = objects.shown(now.local_id) else {
        return Some(Form::Full);
    };
    if shown == now {
        return announced;
    }
    if placement_only(shown, now) {
        Some(Form::Terse)
    } else {
        Some(Form::Full)
    }
}

/// Whether `now` differs from `shown` in nothing a terse update does not
/// carry: its motion and its state byte.
fn placement_only(shown: &Object, now: &Object) -> bool {
    let mut placed = shown.clone();
    placed.motion = now.motion.clone();
    placed.state = now.state;
    placed == *now
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::RegionLocalObjectId;
    use sl_types::key::{AgentKey, ObjectKey};
    use sl_types::lsl::Vector;

    use super::{UpdateBurst, begin, finish};
    use crate::world::{RegionChange, SceneFixtures, box_prim};

    /// What a test returns when a step that should succeed does not.
    type TestError = Box<dyn core::error::Error>;

    /// A region of three boxes, every one already shown.
    fn region() -> SceneFixtures {
        let mut world = SceneFixtures::new();
        for local_id in 1..=3 {
            world.objects.push(box_prim(
                RegionLocalObjectId(local_id),
                ObjectKey::from(uuid::Uuid::from_u128(u128::from(local_id))),
                AgentKey::from(uuid::Uuid::from_u128(0xA)),
                Vector {
                    x: 10.0,
                    y: 10.0,
                    z: 25.0,
                },
                Vector {
                    x: 1.0,
                    y: 1.0,
                    z: 1.0,
                },
            ));
        }
        begin(&mut world);
        world
    }

    /// The burst among `changes`, or an empty one.
    fn burst_of(changes: &[RegionChange]) -> UpdateBurst {
        changes
            .iter()
            .find_map(|change| match change {
                RegionChange::Burst(burst) => Some(burst.as_ref().clone()),
                _other => None,
            })
            .unwrap_or_default()
    }

    /// The local ids of `objects`.
    fn ids(objects: &[sl_proto::Object]) -> Vec<u32> {
        objects.iter().map(|object| object.local_id.0).collect()
    }

    /// A prim moved every tick goes out terse once a tick, at the position
    /// it reached, and never in full.
    #[test]
    fn a_prim_moved_every_tick_goes_out_terse() -> Result<(), TestError> {
        let mut world = region();
        for tick in 1..=5_u8 {
            begin(&mut world);
            for step in 0..3_u8 {
                let mut prim = world
                    .object_mut(RegionLocalObjectId(2))
                    .ok_or("the prim is not in the region")?;
                prim.motion.position.x = f32::from(tick * 10 + step);
            }
            let burst = burst_of(&finish(&mut world, Vec::new()));
            assert_eq!(ids(&burst.terse), vec![2]);
            assert_eq!(burst.full, Vec::new());
            assert_eq!(
                burst
                    .terse
                    .first()
                    .map(|prim| prim.motion.position.x.to_string()),
                Some(f32::from(tick * 10 + 2).to_string())
            );
        }
        Ok(())
    }

    /// Three properties set in one tick, each announced, are one full update.
    #[test]
    fn three_changes_in_a_tick_are_one_full_update() -> Result<(), TestError> {
        let mut world = region();
        begin(&mut world);
        let mut changes = Vec::new();
        for change in [
            |prim: &mut sl_proto::Object| prim.motion.position.z = 30.0,
            |prim: &mut sl_proto::Object| prim.update_flags |= 0x10,
            |prim: &mut sl_proto::Object| prim.text = "hello".to_owned(),
        ] {
            let mut prim = world
                .object_mut(RegionLocalObjectId(1))
                .ok_or("the prim is not in the region")?;
            change(&mut prim);
            let announced = prim.clone();
            drop(prim);
            changes.push(RegionChange::Updated(Box::new(announced)));
        }
        let burst = burst_of(&finish(&mut world, changes));
        assert_eq!(ids(&burst.full), vec![1]);
        assert_eq!(burst.terse, Vec::new());
        let sent = burst.full.first().ok_or("nothing was sent")?;
        assert_eq!(sent.text, "hello");
        assert_eq!(sent.update_flags & 0x10, 0x10);
        Ok(())
    }

    /// A session's change between ticks is its own to tell: the next tick
    /// does not send it again, and a move after it goes out terse.
    #[test]
    fn a_sessions_change_is_not_sent_again() -> Result<(), TestError> {
        let mut world = region();
        world
            .object_mut(RegionLocalObjectId(3))
            .ok_or("the prim is not in the region")?
            .text = "edited".to_owned();
        begin(&mut world);
        assert_eq!(finish(&mut world, Vec::new()), Vec::new());
        begin(&mut world);
        world
            .object_mut(RegionLocalObjectId(3))
            .ok_or("the prim is not in the region")?
            .motion
            .position
            .y = 50.0;
        let burst = burst_of(&finish(&mut world, Vec::new()));
        assert_eq!(ids(&burst.terse), vec![3]);
        assert_eq!(burst.full, Vec::new());
        Ok(())
    }

    /// An announced prim goes out even unchanged, in the order announced and
    /// before the prims only the store knew of; a prim written but left as it
    /// was is not sent; a removed one is a kill and nothing else; and a quiet
    /// write is not announced.
    #[test]
    fn announcements_order_and_quiet_writes() -> Result<(), TestError> {
        let mut world = region();
        begin(&mut world);
        let unchanged = |world: &SceneFixtures, local_id| {
            world
                .object_by_local_id(RegionLocalObjectId(local_id))
                .map(|prim| RegionChange::Updated(Box::new(prim)))
                .ok_or("the prim is not in the region")
        };
        world
            .object_mut(RegionLocalObjectId(1))
            .ok_or("the prim is not in the region")?
            .text = "first in the store".to_owned();
        drop(world.object_mut(RegionLocalObjectId(2)));
        let changes = vec![unchanged(&world, 3)?, RegionChange::OverlayChanged];
        let out = finish(&mut world, changes);
        assert!(matches!(out.first(), Some(RegionChange::OverlayChanged)));
        assert_eq!(ids(&burst_of(&out).full), vec![3, 1]);

        begin(&mut world);
        let _gone = world.remove_object(RegionLocalObjectId(2));
        {
            let mut prim = world
                .object_mut(RegionLocalObjectId(1))
                .ok_or("the prim is not in the region")?;
            prim.update_flags |= 0x20;
        }
        world
            .objects
            .amend_shown(RegionLocalObjectId(1), |shown| shown.update_flags |= 0x20);
        let changes = vec![
            unchanged(&world, 3)?,
            RegionChange::Killed(RegionLocalObjectId(2)),
        ];
        let burst = burst_of(&finish(&mut world, changes));
        assert_eq!(ids(&burst.full), vec![3]);
        assert_eq!(burst.killed, vec![RegionLocalObjectId(2)]);
        Ok(())
    }
}
