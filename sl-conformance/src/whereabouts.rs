//! Where a session's own avatar last stood, kept by the session's event
//! forwarder rather than by whichever case body happens to read its events.
//!
//! A case that moves an avatar (a sit and a stand, a script's teleport, held
//! movement controls) and then hands the scene to a later step needs to know
//! where the avatar ended up, to put it back. The answer is the newest object
//! update of the avatar, but a case body reads only the events it waits for,
//! and a wait discards the rest: the update that says where a stand left the
//! avatar is as likely as not read and dropped by some other wait, or still
//! unread when the step ends. The forwarder that drains the run loop's events
//! sees every one of them (see
//! [`RezLedger`](crate::rez_ledger::RezLedger), kept the same way), so it keeps
//! the newest update of the session's own avatar in a [`Whereabouts`], and
//! [`Session::whereabouts`](crate::context::Session::whereabouts) reads it.

use std::sync::{Arc, Mutex, PoisonError};

use sl_client_tokio::{AgentKey, Event, RegionHandle, RegionLocalObjectId, Vector};

/// Where the avatar was last seen.
#[derive(Debug, Clone, PartialEq)]
pub enum Sighting {
    /// Standing (or flying) on its own in `region`, at the region-local
    /// `position`.
    Standing {
        /// The region it stands in.
        region: RegionHandle,
        /// Where, in that region's coordinates.
        position: Vector,
    },
    /// Seated on an object: its update's position is then an offset from the
    /// seat, not a place in the region.
    Seated,
}

/// The newest [`Sighting`] of one agent's avatar, shared between the
/// forwarder that writes it and the session that reads it.
#[derive(Debug, Clone, Default)]
pub struct Whereabouts {
    /// The agent whose avatar this follows; `None` when the login named none,
    /// and then nothing is ever seen.
    agent: Option<AgentKey>,
    /// The newest sighting, if any.
    last: Arc<Mutex<Option<Sighting>>>,
}

impl Whereabouts {
    /// Whereabouts that follow the avatar of `agent`.
    #[must_use]
    pub fn of(agent: Option<AgentKey>) -> Self {
        Self {
            agent,
            last: Arc::default(),
        }
    }

    /// Learns from one event of the session: an update of the agent's own
    /// avatar. (A teleport inside the region is followed by one, which says
    /// where it landed in the region the update names; the `TeleportLocal`
    /// itself names no region.)
    pub fn observe(&self, event: &Event) {
        let Some(agent) = self.agent else {
            return;
        };
        let sighting = match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id.uuid() == agent.uuid() =>
            {
                if object.parent_id == RegionLocalObjectId(0) {
                    Sighting::Standing {
                        region: object.region_handle,
                        position: object.motion.position.clone(),
                    }
                } else {
                    Sighting::Seated
                }
            }
            _other => return,
        };
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(sighting);
    }

    /// The newest sighting, or `None` when the avatar has not been seen yet.
    #[must_use]
    pub fn last(&self) -> Option<Sighting> {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        AgentKey, Event, Object, ObjectKey, RegionHandle, RegionLocalObjectId, Uuid, Vector,
    };

    use super::{Sighting, Whereabouts};

    /// The agent the tests follow.
    fn agent() -> AgentKey {
        AgentKey::from(Uuid::from_u128(0xA1))
    }

    /// A metre-sized box with the key `key`, at `position` in region 0,
    /// linked to the object `parent` (0 for none).
    fn object(key: Uuid, position: Vector, parent: u32) -> Box<Object> {
        let mut object = sl_fake_grid::world::box_prim(
            RegionLocalObjectId(5),
            ObjectKey::from(key),
            agent(),
            position,
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        );
        object.region_handle = RegionHandle(0);
        object.parent_id = RegionLocalObjectId(parent);
        Box::new(object)
    }

    /// A spot in region 0.
    fn at(x: f32) -> Vector {
        Vector {
            x,
            y: 20.0,
            z: 30.0,
        }
    }

    /// The newest update of the agent's own avatar says where it is; a
    /// seated update says only that it sits, and a stand's update says
    /// where the stand left it.
    #[test]
    fn the_newest_update_of_the_own_avatar_is_its_whereabouts() {
        let whereabouts = Whereabouts::of(Some(agent()));
        assert_eq!(whereabouts.last(), None);
        whereabouts.observe(&Event::ObjectAdded(object(agent().uuid(), at(10.0), 0)));
        assert_eq!(
            whereabouts.last(),
            Some(Sighting::Standing {
                region: RegionHandle(0),
                position: at(10.0),
            })
        );
        whereabouts.observe(&Event::ObjectUpdated(object(agent().uuid(), at(0.5), 9)));
        assert_eq!(whereabouts.last(), Some(Sighting::Seated));
        whereabouts.observe(&Event::ObjectUpdated(object(agent().uuid(), at(12.0), 0)));
        assert_eq!(
            whereabouts.last(),
            Some(Sighting::Standing {
                region: RegionHandle(0),
                position: at(12.0),
            })
        );
    }

    /// Updates of other objects and other avatars, and whereabouts that
    /// follow no agent, leave the sighting as it was.
    #[test]
    fn other_objects_leave_the_whereabouts_alone() {
        let whereabouts = Whereabouts::of(Some(agent()));
        whereabouts.observe(&Event::ObjectAdded(object(agent().uuid(), at(10.0), 0)));
        whereabouts.observe(&Event::ObjectUpdated(object(
            Uuid::from_u128(0xB2),
            at(50.0),
            0,
        )));
        assert_eq!(
            whereabouts.last(),
            Some(Sighting::Standing {
                region: RegionHandle(0),
                position: at(10.0),
            })
        );

        let nobody = Whereabouts::of(None);
        nobody.observe(&Event::ObjectAdded(object(agent().uuid(), at(10.0), 0)));
        assert_eq!(nobody.last(), None);
    }
}
