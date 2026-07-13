//! The ECS world model the plugin maintains from the session event stream: the
//! global login-identity [`Resource`](SlIdentity) and the per-region entities
//! ([`SlRegion`] and friends).
//!
//! Globally-unique login facts (agent id, session id, circuit code, seed
//! capability, current region handle) live in the single [`SlIdentity`]
//! resource — a consumer reads them at any tick with `Res<SlIdentity>` rather
//! than catching a one-shot event. Per-region state lives on **components of
//! region entities**: the login/root region and every neighbour each get an
//! entity carrying [`SlRegion`] (handle + sim address), with richer state
//! ([`SlRegionIdentity`], [`SlRegionLimits`], parcels) attached as further
//! components / child entities. The [`maintain_world`] system folds the
//! high-level [`SlEvent`](crate::SlEvent) stream into this model each frame.

use std::collections::HashMap;
use std::net::SocketAddr;

use bevy::prelude::*;

use sl_proto::{
    AgentKey, CircuitCode, CircuitId, Event as SessionEvent, ParcelInfo, RegionHandle,
    RegionIdentity, RegionLimits, RegionLocalParcelId, Session, Uuid,
};

use crate::SlEvent;

/// The session's login-derived identity facts — global and unique for the
/// lifetime of one logged-in session. The plugin inserts a default (all-`None`)
/// instance at startup, populates it once the circuit comes up, and updates
/// [`region_handle`](Self::region_handle) on each region change.
///
/// This supersedes the former fire-once `SlIdentity` *event*: a consumer reads
/// the facts at any tick (`Res<SlIdentity>`, optionally gated on
/// [`is_changed`](bevy::ecs::change_detection::DetectChanges::is_changed))
/// instead of having to catch a one-shot event. Mirrors the tokio client's
/// `agent_id` / `session_id` / `circuit_code` / `seed_capability` /
/// `region_handle` accessors for runtime parity.
#[derive(Resource, Debug, Clone, Default)]
pub struct SlIdentity {
    /// The logged-in avatar's agent id.
    pub agent_id: Option<AgentKey>,
    /// The session id assigned by the grid.
    pub session_id: Option<Uuid>,
    /// The circuit code assigned by the grid.
    pub circuit_code: Option<CircuitCode>,
    /// The seed capability URL, if the login response carried one.
    pub seed_capability: Option<url::Url>,
    /// The agent-appearance (server-side "Sunshine" bake) service base URL from
    /// login, if the grid central-bakes. Server-baked avatar textures are fetched
    /// from here (`<url>texture/<avatar>/<slot>/<uuid>`), not by UUID from the
    /// `GetTexture` CDN. `None` on a grid without central baking (OpenSim).
    pub agent_appearance_service: Option<url::Url>,
    /// The handle of the region the agent's root circuit currently occupies.
    /// Seeded from the login response and updated on each `RegionChanged`. The
    /// matching region entity is the one marked [`SlCurrentRegion`].
    pub region_handle: Option<RegionHandle>,
    /// The identity of the current root circuit. Seeded from the login response
    /// and refreshed on each `CircuitEstablished` / `RegionChanged`. Pair it with
    /// a region-local id to build the [`ScopedParcelId`](sl_proto::ScopedParcelId)
    /// / [`ScopedObjectId`](sl_proto::ScopedObjectId) the scoped parcel/object
    /// commands take.
    pub circuit_id: Option<CircuitId>,
}

/// The agent's current parcel and derived fly permission, mirrored from the
/// driven [`Session`] each frame so ECS systems can read them without holding the
/// session. The resolution logic lives in `sl-proto`
/// ([`Session::current_parcel`] / [`Session::can_fly`]) so the tokio client
/// shares it; this resource is just the Bevy-side bridge.
///
/// [`current`](Self::current) is `None` until the agent's parcel resolves (the
/// simulator pushes it on region entry / parcel crossing);
/// [`can_fly`](Self::can_fly) combines the region-wide fly block and the parcel's
/// `ALLOW_FLY`, and is permissive while the parcel is unknown (so a take-off is
/// not blocked before the push arrives) — but `false` before login.
#[derive(Resource, Default, Debug, Clone)]
pub struct SlAgentParcel {
    /// The parcel the agent currently stands on, or `None` before it resolves.
    pub current: Option<ParcelInfo>,
    /// Whether the agent may start flying where it currently stands.
    pub can_fly: bool,
}

impl SlAgentParcel {
    /// Refreshes the mirror from the driven session: the fly permission is read
    /// every frame (cheap), while the current parcel is re-cloned only when it
    /// actually changes (a structural compare avoids cloning its ~½ KiB bitmap
    /// and strings on every unchanged frame).
    pub(crate) fn refresh_from(&mut self, session: &Session) {
        self.can_fly = session.can_fly();
        let current = session.current_parcel();
        if self.current.as_ref() != current {
            self.current = current.cloned();
        }
    }
}

/// A region the client knows about — the login/root region and every neighbour
/// announced via `EnableSimulator`. The handle and sim address are the region's
/// stable identity; richer state ([`SlRegionIdentity`], [`SlRegionLimits`],
/// parcels) attaches as further components and child entities.
#[derive(Component, Debug, Clone, Copy)]
pub struct SlRegion {
    /// The region's handle (its grid position, packed).
    pub handle: RegionHandle,
    /// The region simulator's UDP address.
    pub sim: SocketAddr,
}

/// Marker on the single region entity the agent's root circuit currently
/// occupies. Moves to the destination region on each `RegionChanged`; the same
/// handle is mirrored in [`SlIdentity::region_handle`].
#[derive(Component, Debug)]
pub struct SlCurrentRegion;

/// Marker on a region entity that is a discovered neighbour reachable over a
/// child circuit (not the current root region). Cleared if the agent later
/// crosses into that region (it then gains [`SlCurrentRegion`]).
#[derive(Component, Debug)]
pub struct SlNeighbor;

/// A region's identity / maturity / product info, from its `RegionHandshake`
/// (`Event::RegionInfoHandshake`).
#[derive(Component, Debug, Clone)]
pub struct SlRegionIdentity(pub RegionIdentity);

/// A region's agent / object limits, from a `RegionInfo` reply
/// (`Event::RegionLimits`).
#[derive(Component, Debug, Clone)]
pub struct SlRegionLimits(pub RegionLimits);

/// A parcel within a region (`Event::ParcelProperties`), held on a child entity
/// of the owning region entity and keyed by its region-local id.
#[derive(Component, Debug, Clone)]
pub struct SlParcel(pub ParcelInfo);

/// Internal index from region handle to its spawned entity (and parcels to
/// theirs), plus the current region's handle. It lets [`maintain_world`] attach
/// components to a region — and find the current region — without a query, which
/// could not see entities the same system spawned earlier in the frame (spawns
/// are deferred, but the returned [`Entity`] id is usable immediately). The
/// authoritative state still lives on the components; this is only a fast index.
#[derive(Resource, Default)]
pub(crate) struct SlRegionIndex {
    /// Region handle → its entity, for every known region (root + neighbours).
    by_handle: HashMap<RegionHandle, Entity>,
    /// (region handle, parcel local id) → the parcel's child entity.
    parcels: HashMap<(RegionHandle, RegionLocalParcelId), Entity>,
    /// The handle of the region currently marked [`SlCurrentRegion`].
    current: Option<RegionHandle>,
}

/// Plugin system: folds the high-level session event stream into the ECS world
/// model — the [`SlIdentity`] resource's current region handle and the
/// per-region entities/components. Scheduled after `drive`, so the events it
/// reads were produced this same frame.
pub(crate) fn maintain_world(
    mut events: MessageReader<SlEvent>,
    mut identity: ResMut<SlIdentity>,
    mut index: ResMut<SlRegionIndex>,
    mut commands: Commands,
) {
    for SlEvent(event) in events.read() {
        match event {
            // The root circuit came up: the login region's handle is already in
            // the identity resource; pair it with the sim address to spawn (or
            // adopt) the current region entity.
            SessionEvent::CircuitEstablished { sim, circuit } => {
                identity.circuit_id = Some(*circuit);
                if let Some(handle) = identity.region_handle {
                    set_current_region(&mut commands, &mut index, handle, *sim);
                }
            }
            SessionEvent::RegionInfoHandshake(region_identity) => {
                if let Some(entity) = current_entity(&index) {
                    commands
                        .entity(entity)
                        .insert(SlRegionIdentity((**region_identity).clone()));
                }
            }
            SessionEvent::RegionLimits(limits) => {
                if let Some(entity) = current_entity(&index) {
                    commands
                        .entity(entity)
                        .insert(SlRegionLimits(limits.clone()));
                }
            }
            SessionEvent::NeighborDiscovered(info) => {
                ensure_neighbor(&mut commands, &mut index, info.region_handle, info.sim);
            }
            SessionEvent::ParcelProperties(info) => {
                upsert_parcel(&mut commands, &mut index, (**info).clone());
            }
            // A teleport handover completed: the destination is now the root
            // region. Update the global handle and move the current marker.
            SessionEvent::RegionChanged {
                region_handle,
                sim,
                circuit,
                ..
            } => {
                identity.region_handle = Some(*region_handle);
                identity.circuit_id = Some(*circuit);
                set_current_region(&mut commands, &mut index, *region_handle, *sim);
            }
            SessionEvent::Disconnected(_) | SessionEvent::LoggedOut => {
                clear_world(&mut commands, &mut index);
            }
            _other => {}
        }
    }
}

/// The entity of the region currently marked [`SlCurrentRegion`], if one is
/// known.
fn current_entity(index: &SlRegionIndex) -> Option<Entity> {
    index
        .current
        .and_then(|handle| index.by_handle.get(&handle).copied())
}

/// Promotes the region identified by `handle` / `sim` to the current root
/// region: spawns or reuses its entity, marks it [`SlCurrentRegion`] (clearing
/// any [`SlNeighbor`] marker), and moves the current marker off the previous
/// region.
fn set_current_region(
    commands: &mut Commands,
    index: &mut SlRegionIndex,
    handle: RegionHandle,
    sim: SocketAddr,
) {
    if let Some(previous) = index.current
        && previous != handle
        && let Some(&entity) = index.by_handle.get(&previous)
    {
        commands.entity(entity).remove::<SlCurrentRegion>();
    }
    let entity = match index.by_handle.get(&handle).copied() {
        Some(entity) => {
            commands.entity(entity).insert(SlRegion { handle, sim });
            entity
        }
        None => {
            let entity = commands.spawn(SlRegion { handle, sim }).id();
            index.by_handle.insert(handle, entity);
            entity
        }
    };
    commands
        .entity(entity)
        .insert(SlCurrentRegion)
        .remove::<SlNeighbor>();
    index.current = Some(handle);
}

/// Spawns a neighbour region entity for `handle` / `sim` if no entity for that
/// handle exists yet (the current region and already-known neighbours are left
/// untouched).
fn ensure_neighbor(
    commands: &mut Commands,
    index: &mut SlRegionIndex,
    handle: RegionHandle,
    sim: SocketAddr,
) {
    if index.by_handle.contains_key(&handle) {
        return;
    }
    let entity = commands.spawn((SlRegion { handle, sim }, SlNeighbor)).id();
    index.by_handle.insert(handle, entity);
}

/// Upserts a parcel of the current region: updates the existing child entity for
/// the parcel's local id in place, or spawns one and parents it to the region.
fn upsert_parcel(commands: &mut Commands, index: &mut SlRegionIndex, info: ParcelInfo) {
    let Some(region_handle) = index.current else {
        return;
    };
    let Some(&region) = index.by_handle.get(&region_handle) else {
        return;
    };
    let key = (region_handle, info.local_id);
    match index.parcels.get(&key).copied() {
        Some(entity) => {
            commands.entity(entity).insert(SlParcel(info));
        }
        None => {
            let entity = commands.spawn(SlParcel(info)).id();
            commands.entity(region).add_child(entity);
            index.parcels.insert(key, entity);
        }
    }
}

/// Despawns every region entity (their parcel children despawn with them) and
/// clears the index, on logout or disconnect.
fn clear_world(commands: &mut Commands, index: &mut SlRegionIndex) {
    for (_handle, entity) in index.by_handle.drain() {
        commands.entity(entity).despawn();
    }
    index.parcels.clear();
    index.current = None;
}

#[cfg(test)]
mod tests {
    use super::{SlCurrentRegion, SlIdentity, SlNeighbor, SlRegion, SlRegionIndex, maintain_world};

    use std::net::SocketAddr;

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use sl_proto::{CircuitId, Event as SessionEvent, GridCoordinates, NeighborInfo, RegionHandle};

    use crate::SlEvent;

    /// A loopback simulator address on the given port, for synthesising events.
    fn sim(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// A minimal app wired exactly like the plugin's world maintenance: the event
    /// channel, the identity + index resources, and the `maintain_world` system.
    fn world_app() -> App {
        let mut app = App::new();
        app.add_message::<SlEvent>()
            .init_resource::<SlIdentity>()
            .init_resource::<SlRegionIndex>()
            .add_systems(Update, maintain_world);
        app
    }

    #[test]
    fn circuit_established_spawns_the_current_region() {
        let mut app = world_app();
        let handle = RegionHandle(0x0000_03e8_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(handle);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        app.update();

        let mut query = app
            .world_mut()
            .query_filtered::<&SlRegion, With<SlCurrentRegion>>();
        let regions: Vec<(RegionHandle, SocketAddr)> =
            query.iter(app.world()).map(|r| (r.handle, r.sim)).collect();
        assert_eq!(regions, vec![(handle, sim(9000))]);
    }

    #[test]
    fn neighbor_is_distinct_and_a_crossing_promotes_it() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        let next = RegionHandle(0x0000_03e9_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        app.world_mut()
            .write_message(SlEvent(SessionEvent::NeighborDiscovered(NeighborInfo {
                region_handle: next,
                sim: sim(9001),
                grid_coordinates: GridCoordinates::new(1001, 1000),
            })));
        app.update();

        // Two regions total: the current home and the neighbour.
        let mut all = app.world_mut().query::<&SlRegion>();
        assert_eq!(all.iter(app.world()).count(), 2);
        let mut neighbors = app
            .world_mut()
            .query_filtered::<&SlRegion, With<SlNeighbor>>();
        let listed: Vec<RegionHandle> = neighbors.iter(app.world()).map(|r| r.handle).collect();
        assert_eq!(listed, vec![next], "the neighbour is marked, not current");

        // Cross into the neighbour: it becomes current, the marker leaves home,
        // and the global handle follows.
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionChanged {
                region_handle: next,
                sim: sim(9001),
                circuit: CircuitId(2),
            }));
        app.update();

        assert_eq!(
            app.world().resource::<SlIdentity>().region_handle,
            Some(next)
        );
        let mut current = app
            .world_mut()
            .query_filtered::<&SlRegion, With<SlCurrentRegion>>();
        let now: Vec<RegionHandle> = current.iter(app.world()).map(|r| r.handle).collect();
        assert_eq!(
            now,
            vec![next],
            "exactly one current region after the crossing"
        );
        // The promoted region dropped its neighbour marker, and no entity was
        // duplicated.
        let mut neighbors_after = app
            .world_mut()
            .query_filtered::<&SlRegion, With<SlNeighbor>>();
        assert_eq!(neighbors_after.iter(app.world()).count(), 0);
        assert_eq!(all.iter(app.world()).count(), 2);
    }

    #[test]
    fn logout_clears_every_region() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        app.update();
        app.world_mut()
            .write_message(SlEvent(SessionEvent::LoggedOut));
        app.update();

        let mut all = app.world_mut().query::<&SlRegion>();
        assert_eq!(all.iter(app.world()).count(), 0);
    }
}
