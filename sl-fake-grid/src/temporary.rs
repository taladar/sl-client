//! Temporary objects: a linkset whose root carries `TEMPORARY_ON_REZ` leaves
//! the region a minute after it became temporary, unless an avatar sits on
//! it.
//!
//! OpenSim's rule (`Scene.CleanTempObjects`): the flag's arrival sets the
//! root's `Expires` 60 s ahead (`SceneObjectPart.ResetExpire`), and a sweep
//! every 180 frames deletes each expired group nobody sits on. Second Life's
//! "about a minute" is the wiki's, not measured; the fake grid deletes on
//! the first tick past the minute on both flavours, with no sweep cadence.

use std::collections::BTreeMap;
use std::time::Duration;

use sl_types::key::ObjectKey;

use crate::world::{RegionChange, SceneFixtures};

/// How long an object stays temporary before it is deleted.
pub(crate) const LIFETIME: Duration = Duration::from_secs(60);

/// When each temporary root in the region became temporary, on the physics
/// clock ([`crate::physics::Physics::clock`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct Temporaries {
    /// Each temporary root's key and when it was first seen temporary.
    since: BTreeMap<ObjectKey, Duration>,
}

/// One tick's sweep: notes the roots that became temporary, forgets those
/// that are no longer, and takes out of the region each one temporary for
/// [`LIFETIME`] that nobody sits on. What the region's sessions are to be
/// told.
pub(crate) fn sweep(world: &mut SceneFixtures) -> Vec<RegionChange> {
    let now = world.physics.clock;
    let temporary: Vec<_> = world
        .objects
        .iter()
        .filter(|object| {
            object.parent_id.0 == 0
                && object.update_flags & sl_proto::prim_flags::TEMPORARY_ON_REZ != 0
                && world.objects.attached_to(object.local_id).is_none()
        })
        .map(|object| (object.full_id, object.local_id))
        .collect();
    let since = &mut world.temporaries.since;
    since.retain(|key, _since| temporary.iter().any(|(known, _local)| known == key));
    let mut expired = Vec::new();
    for (key, local_id) in temporary {
        let became = *since.entry(key).or_insert(now);
        if now.saturating_sub(became) >= LIFETIME {
            expired.push((key, local_id));
        }
    }
    let mut changes = Vec::new();
    for (key, local_id) in expired {
        if !crate::seating::sitters_of(world, local_id).is_empty() {
            continue;
        }
        let _gone = world.temporaries.since.remove(&key);
        changes.extend(crate::scripts::remove_linkset(world, local_id));
    }
    changes
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_proto::RegionLocalObjectId;
    use sl_types::key::AgentKey;
    use sl_types::lsl::Vector;

    use super::*;

    /// A region holding two half-metre boxes, the first temporary.
    fn region() -> SceneFixtures {
        let mut world = SceneFixtures::default();
        for (n, temporary) in [(1_u32, true), (2, false)] {
            let mut prim = crate::world::box_prim(
                RegionLocalObjectId(n),
                ObjectKey::from(uuid::Uuid::from_u128(u128::from(n))),
                AgentKey::from(uuid::Uuid::from_u128(0x10)),
                Vector {
                    x: 100.0,
                    y: 100.0,
                    z: 30.0,
                },
                Vector {
                    x: 0.5,
                    y: 0.5,
                    z: 0.5,
                },
            );
            if temporary {
                prim.update_flags |= sl_proto::prim_flags::TEMPORARY_ON_REZ;
            }
            world.objects.push(prim);
        }
        world
    }

    /// The region-local ids of the objects left in `world`.
    fn left(world: &SceneFixtures) -> Vec<u32> {
        world
            .objects
            .iter()
            .map(|object| object.local_id.0)
            .collect()
    }

    #[test]
    fn a_temporary_object_goes_a_minute_after_it_was_first_seen() {
        let mut world = region();
        world.physics.clock = Duration::from_secs(5);
        assert!(sweep(&mut world).is_empty());
        world.physics.clock = Duration::from_secs(64);
        assert!(sweep(&mut world).is_empty());
        assert_eq!(left(&world), vec![1, 2]);
        world.physics.clock = Duration::from_secs(65);
        assert_eq!(
            sweep(&mut world),
            vec![RegionChange::Killed(RegionLocalObjectId(1))]
        );
        assert_eq!(left(&world), vec![2]);
    }

    #[test]
    fn an_object_no_longer_temporary_stays() {
        let mut world = region();
        assert!(sweep(&mut world).is_empty());
        if let Some(mut object) = world.object_mut(RegionLocalObjectId(1)) {
            object.update_flags &= !sl_proto::prim_flags::TEMPORARY_ON_REZ;
        }
        world.physics.clock = LIFETIME;
        assert!(sweep(&mut world).is_empty());
        assert_eq!(left(&world), vec![1, 2]);
    }
}
