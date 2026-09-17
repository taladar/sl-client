//! What the map surfaces are pointing at.
//!
//! The tracked target the minimap and (later) the world map share, and the
//! debug beacons drawn beside it, so both surfaces drive one beacon rather
//! than each keeping its own.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sl_client_bevy::{AgentKey, ObjectKey, RegionHandle, Rotation, Vector};

/// The map tracking target — a shared shape for the minimap today and the
/// world map later (`viewer-world-map-tracking-teleport`), so both surfaces
/// drive one beacon.
#[derive(Resource, Debug, Default)]
pub struct MapTracking {
    /// The current target, or `None` when not tracking.
    pub target: Option<TrackTarget>,
}

/// What the map is tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackTarget {
    /// A fixed world location (global metres).
    Location {
        /// Global metres west→east.
        east: f64,
        /// Global metres south→north.
        north: f64,
        /// Altitude in metres.
        up: f32,
    },
    /// An avatar, followed while it is known.
    Avatar(AgentKey),
}

/// One **debug beacon**: a coloured marker drawn at a world position for as long
/// as the feature that asked for it keeps asking — the reference viewer's
/// `LLViewerObjectList::addDebugBeacon`, whose callers are exactly this shape
/// (the telehub floater marking its hub and the selected spawn point, and later
/// the area-search / pathfinding lists marking a chosen object).
///
/// # Why it is not just a position
///
/// A beacon usually marks an **object**, and an object moves. The reference
/// re-derives the position every frame from the live object and only falls back
/// to the cached one when the object is not in the scene
/// (`LLFloaterTelehub::addBeacons`). This carries both, so the renderer can do
/// the same: [`anchor`](Self::anchor) names the object to follow, and
/// [`position`](Self::position) / [`rotation`](Self::rotation) are what to use
/// while it is unknown.
///
/// [`offset`](Self::offset) is applied **in the anchor's frame** — the
/// reference's `hub_pos_region + (spawn_pos * hub_rot)`, which is how a telehub
/// spawn point (stored relative to the hub) turns into a place in the region.
#[derive(Debug, Clone, PartialEq)]
pub struct DebugBeacon {
    /// The region the [`position`](Self::position) is local to.
    pub region: RegionHandle,
    /// The in-world object this beacon follows, when it marks one. Resolved
    /// against the live scene each frame; while it is unknown (out of draw
    /// distance, not yet streamed, or gone) the fallback position and rotation
    /// below are used instead.
    pub anchor: Option<ObjectKey>,
    /// The fallback region-local position, in Second Life metres.
    pub position: Vector,
    /// The fallback rotation [`offset`](Self::offset) is applied in.
    pub rotation: Rotation,
    /// An offset from the anchor (or fallback) position, in the anchor's frame.
    /// Zero for a beacon that marks the object itself.
    pub offset: Vector,
    /// The marker's colour.
    pub color: Color,
}

/// The **debug beacons** currently asked for, grouped by the feature that asked.
///
/// A feature owns one group, named by a `&'static str` it picks, and states its
/// whole set each time anything changes ([`set`](Self::set)) — the reference
/// clears every beacon each frame and lets the open floaters re-add theirs, and
/// a whole-set write is that contract without the per-frame churn. Dropping the
/// group ([`clear`](Self::clear)) is how a window that closes takes its markers
/// with it.
#[derive(Resource, Debug, Default)]
pub struct DebugBeacons {
    /// The beacons each owner is asking for, in owner order so the rendered set
    /// is stable frame to frame.
    groups: BTreeMap<&'static str, Vec<DebugBeacon>>,
}

impl DebugBeacons {
    /// State `owner`'s whole set of beacons, replacing what it asked for before.
    pub fn set(&mut self, owner: &'static str, beacons: Vec<DebugBeacon>) {
        if beacons.is_empty() {
            self.groups.remove(owner);
        } else {
            self.groups.insert(owner, beacons);
        }
    }

    /// Drop `owner`'s beacons — what a closing window does.
    pub fn clear(&mut self, owner: &'static str) {
        self.groups.remove(owner);
    }

    /// Whether `owner` is asking for any beacons.
    #[must_use]
    pub fn has(&self, owner: &'static str) -> bool {
        self.groups.contains_key(owner)
    }

    /// Every beacon asked for, in owner order.
    pub fn iter(&self) -> impl Iterator<Item = &DebugBeacon> {
        self.groups.values().flatten()
    }

    /// Whether nothing is asking for a beacon.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}
