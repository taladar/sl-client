//! Resolving a [`WorldLocator`] against a world snapshot, and [`WorldQuery`]:
//! a resolution that waits, a frame at a time, for the names and owners the
//! simulator has not sent yet.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sl_automation_proto::{Anchor, AutomationError, Deadline, WorldKind, WorldLocator, WorldNode};
use sl_client_bevy::{Command, ObjectKey, SlCommand};

use crate::pursuit::{DEFAULT_DEADLINE, DEFAULT_DEADLINE_FRAMES, PursuitError};
use crate::world_model::WorldModel;

/// Every node of `nodes` that `locator` matches, in the snapshot's order — or,
/// under [`WorldLocator::near`], nearest the anchor first and only those
/// within its radius. [`WorldLocator::nth`] then picks one.
///
/// A proximity criterion needs a position on both ends: a thing drawn on the
/// HUD is never near anything, and while the own avatar is not in the
/// snapshot nothing is near it.
#[must_use]
pub fn find_world<'a>(nodes: &'a [WorldNode], locator: &WorldLocator) -> Vec<&'a WorldNode> {
    let mut matches: Vec<&WorldNode> = nodes
        .iter()
        .filter(|node| locator.matches_node(node))
        .collect();
    if let Some(near) = locator.near {
        let anchor = match near.to {
            Anchor::Point(point) => Some(point),
            Anchor::OwnAvatar => nodes
                .iter()
                .find(|node| node.own && node.kind == WorldKind::Avatar)
                .and_then(|node| node.position),
        };
        let Some(anchor) = anchor else {
            return Vec::new();
        };
        let mut placed: Vec<(f32, &WorldNode)> = matches
            .into_iter()
            .filter_map(|node| Some((distance(anchor, node.position?), node)))
            .filter(|(distance, _node)| near.radius.is_none_or(|radius| *distance <= radius))
            .collect();
        placed.sort_by(|(left, _), (right, _)| left.total_cmp(right));
        matches = placed.into_iter().map(|(_distance, node)| node).collect();
    }
    match locator.nth {
        Some(index) => usize::try_from(index)
            .ok()
            .and_then(|index| matches.get(index).copied())
            .into_iter()
            .collect(),
        None => matches,
    }
}

/// The straight-line distance between two region-local points, metres.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Vec3::new(ax - bx, ay - by, az - bz).length()
}

/// How many things a [`WorldQuery`] wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldWant {
    /// Every match, possibly none — a lookup. Ready as soon as every thing
    /// that might match is known well enough to say.
    All,
    /// Exactly one match — what an action on the thing needs. Waits while
    /// nothing matches; several matches fail at once.
    One,
}

/// Where a [`WorldQuery`] stands after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum WorldProgress {
    /// The answer: every match for [`WorldWant::All`], the one match for
    /// [`WorldWant::One`].
    Ready(Vec<WorldNode>),
    /// Not yet: poll again next frame.
    Waiting {
        /// How many things might match but still lack the name or owner the
        /// locator compares.
        unresolved: usize,
    },
}

/// A world locator's resolution, polled once a frame until it can answer.
///
/// An object's name and owner are not part of the stream that places it, so
/// a locator comparing either cannot judge an object whose property reply
/// has not arrived. Each poll finds the things that match every *other*
/// criterion, asks the simulator — once per query — for the properties of
/// any such object still unnamed (the `ObjectPropertiesFamily` request a
/// hover makes, which selects nothing), and answers only once none is left
/// unresolved. An avatar's name is the avatar layer's to fetch; the query
/// only waits for it.
#[derive(Debug)]
pub struct WorldQuery {
    /// What is wanted.
    locator: WorldLocator,
    /// How many of it.
    want: WorldWant,
    /// The most frames to wait.
    max_frames: u32,
    /// The most wall-clock time to wait.
    max_time: Duration,
    /// When the first poll ran.
    started: Option<Instant>,
    /// The polls so far.
    frames: u32,
    /// The objects whose properties this query has asked for.
    requested: HashSet<ObjectKey>,
}

impl WorldQuery {
    /// Resolve `locator` to `want`, under the default deadline.
    #[must_use]
    pub fn new(locator: WorldLocator, want: WorldWant) -> Self {
        Self {
            locator,
            want,
            max_frames: DEFAULT_DEADLINE_FRAMES,
            max_time: DEFAULT_DEADLINE,
            started: None,
            frames: 0,
            requested: HashSet::new(),
        }
    }

    /// Give up after `deadline`; a limit it leaves unset keeps the default.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Deadline) -> Self {
        if let Some(frames) = deadline.frames {
            self.max_frames = frames;
        }
        if let Some(millis) = deadline.millis {
            self.max_time = Duration::from_millis(millis);
        }
        self
    }

    /// The locator being resolved.
    #[must_use]
    pub const fn locator(&self) -> &WorldLocator {
        &self.locator
    }

    /// Look at the world once — a frame's worth of the wait. May ask the
    /// simulator for object properties on the way.
    ///
    /// # Errors
    ///
    /// [`AutomationError::WorldAmbiguous`] as soon as a [`WorldWant::One`]
    /// query matches several things; [`AutomationError::WorldTimedOut`] once
    /// the deadline passes with properties still missing, or with nothing
    /// matching a [`WorldWant::One`] query; and [`PursuitError::Model`] when
    /// the model cannot be read at all.
    pub fn poll(&mut self, world: &mut World) -> Result<WorldProgress, PursuitError> {
        let started = *self.started.get_or_insert_with(Instant::now);
        self.frames = self.frames.saturating_add(1);
        let nodes = {
            let mut state = SystemState::<WorldModel<'_, '_>>::new(world);
            state.get(world)?.snapshot()
        };
        let unresolved = self.unresolved(&nodes);
        let to_ask: Vec<ObjectKey> = unresolved
            .iter()
            .filter(|node| node.kind != WorldKind::Avatar)
            .map(|node| ObjectKey::from(node.full_id))
            .filter(|object| self.requested.insert(*object))
            .collect();
        for object_id in to_ask {
            world.write_message(SlCommand(Command::RequestObjectPropertiesFamily {
                request_flags: 0,
                object_id,
            }));
        }
        let matches: Vec<WorldNode> = find_world(&nodes, &self.locator)
            .into_iter()
            .cloned()
            .collect();
        if unresolved.is_empty() {
            match (self.want, matches.len()) {
                (WorldWant::All, _) | (WorldWant::One, 1) => {
                    return Ok(WorldProgress::Ready(matches));
                }
                (WorldWant::One, 0) => {}
                (WorldWant::One, _) => {
                    return Err(AutomationError::WorldAmbiguous {
                        locator: self.locator.clone(),
                        candidates: matches,
                    }
                    .into());
                }
            }
        }
        let waited = started.elapsed();
        if self.frames >= self.max_frames || waited >= self.max_time {
            return Err(AutomationError::WorldTimedOut {
                locator: self.locator.clone(),
                failed_check: None,
                unresolved,
                last_observed: matches,
                frames: self.frames,
                millis: u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            }
            .into());
        }
        Ok(WorldProgress::Waiting {
            unresolved: unresolved.len(),
        })
    }

    /// The things that match every criterion but the name and the owner, and
    /// lack whichever of the two the locator compares.
    ///
    /// Only an avatar or a linkset's root is waited for: a grid answers a
    /// child prim's family request with its root's record (OpenSim's
    /// `ServiceObjectPropertiesFamilyRequest` sends the root part), so a
    /// child's own name arrives only with a selection's full properties, and
    /// waiting on it would wait for good on any region with a linkset.
    fn unresolved(&self, nodes: &[WorldNode]) -> Vec<WorldNode> {
        let wants_name = self.locator.name.is_some();
        let wants_owner = self.locator.owner.is_some();
        if !wants_name && !wants_owner {
            return Vec::new();
        }
        let loosened = WorldLocator {
            name: None,
            owner: None,
            nth: None,
            ..self.locator.clone()
        };
        find_world(nodes, &loosened)
            .into_iter()
            .filter(|node| node.kind == WorldKind::Avatar || node.parent.is_none())
            .filter(|node| {
                (wants_name && node.name.is_none())
                    || (wants_owner && node.owner.is_none() && node.kind != WorldKind::Avatar)
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_automation_proto::{Anchor, WorldKind, WorldLocator, WorldNode};
    use sl_client_bevy::Uuid;

    use super::find_world;

    /// A thing of `kind` at `position`, named `name`.
    fn thing(id: u128, kind: WorldKind, name: &str, position: Option<[f32; 3]>) -> WorldNode {
        WorldNode {
            kind,
            own: false,
            full_id: Uuid::from_u128(id),
            local_id: u32::try_from(id).ok(),
            pcode: if kind == WorldKind::Avatar { 47 } else { 9 },
            name: Some(name.to_owned()),
            description: None,
            owner: None,
            position,
            rotation: None,
            scale: None,
            parent: None,
            children: Vec::new(),
            attachment_point: None,
            worn_by: None,
            sitting_on: None,
            selected: false,
            hover_text: None,
            name_tag: None,
            bakes: Vec::new(),
        }
    }

    /// The own avatar at the origin corner, a far and a near tree, and a HUD
    /// attachment with no position.
    fn scene() -> Vec<WorldNode> {
        let mut own = thing(1, WorldKind::Avatar, "Me", Some([10.0, 10.0, 20.0]));
        own.own = true;
        vec![
            own,
            thing(2, WorldKind::Object, "Tree", Some([50.0, 10.0, 20.0])),
            thing(3, WorldKind::Object, "Tree", Some([12.0, 10.0, 20.0])),
            thing(4, WorldKind::Attachment, "Tree", None),
        ]
    }

    /// The full ids of `nodes`, as their low numbers.
    fn ids(nodes: &[&WorldNode]) -> Vec<u128> {
        nodes.iter().map(|node| node.full_id.as_u128()).collect()
    }

    /// A query for a name waits on an unnamed linkset root, and not on an
    /// unnamed child prim, whose family request a grid answers with the
    /// root's record.
    #[test]
    fn a_name_query_waits_on_unnamed_roots_not_on_child_prims() {
        let mut root = thing(5, WorldKind::Object, "", Some([20.0, 20.0, 20.0]));
        root.name = None;
        let mut child = thing(6, WorldKind::Object, "", Some([20.0, 21.0, 20.0]));
        child.name = None;
        child.parent = Some(5);
        let query =
            super::WorldQuery::new(WorldLocator::default().named("Door"), super::WorldWant::One);
        let waiting: Vec<u128> = query
            .unresolved(&[root, child])
            .iter()
            .map(|node| node.full_id.as_u128())
            .collect();
        assert_eq!(waiting, vec![5]);
    }

    #[test]
    fn without_proximity_matches_keep_the_snapshot_order() {
        let scene = scene();
        let trees = WorldLocator::default().named("Tree");
        assert_eq!(ids(&find_world(&scene, &trees)), vec![2, 3, 4]);
        assert_eq!(ids(&find_world(&scene, &trees.nth(1))), vec![3]);
    }

    #[test]
    fn proximity_orders_outwards_and_drops_the_unplaced() {
        let scene = scene();
        let trees = WorldLocator::default().named("Tree");
        assert_eq!(
            ids(&find_world(
                &scene,
                &trees.clone().near(Anchor::OwnAvatar, None)
            )),
            vec![3, 2],
            "nearest first; the HUD attachment is nowhere"
        );
        assert_eq!(
            ids(&find_world(
                &scene,
                &trees.clone().nearest_to(Anchor::OwnAvatar)
            )),
            vec![3]
        );
        assert_eq!(
            ids(&find_world(
                &scene,
                &trees
                    .clone()
                    .near(Anchor::Point([50.0, 10.0, 21.0]), Some(5.0))
            )),
            vec![2],
            "only within the radius of the point"
        );
        let alone = vec![thing(2, WorldKind::Object, "Tree", Some([0.0; 3]))];
        assert!(
            find_world(&alone, &trees.near(Anchor::OwnAvatar, None)).is_empty(),
            "no own avatar, nothing near it"
        );
    }
}
