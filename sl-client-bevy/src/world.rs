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
    AgentKey, CircuitCode, CircuitId, DEFAULT_GRIDS_PER_EDGE, Event as SessionEvent, ObjectKey,
    ParcelInfo, ParcelOverlayGrid, ParcelOverlayInfo, RegionHandle, RegionIdentity, RegionLimits,
    RegionLocalParcelId, Session, Uuid,
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
    /// The grid's map-tile server base URL from login, if the grid announced
    /// one (`map-server-url`). World-map tiles are fetched from here as
    /// `<url>map-<zoom>-<x>-<y>-objects.jpg`; a region's `SimulatorFeatures`
    /// `map-server-url` — surfaced as
    /// [`Event::SimulatorFeatures`](sl_proto::Event::SimulatorFeatures) — is
    /// fresher and should win where present.
    pub map_server_url: Option<url::Url>,
    /// The OpenID endpoint from login (`openid_url`), or `None` off Second Life.
    /// The viewer POSTs [`openid_token`](Self::openid_token) here and injects the
    /// reply's session cookie into the embedded browser so the in-viewer web
    /// surfaces (web profiles, search, marketplace) open already logged in.
    pub openid_url: Option<url::Url>,
    /// The one-time OpenID token from login (`openid_token`), POSTed to
    /// [`openid_url`](Self::openid_url) to mint the web-session cookie. `None`
    /// off Second Life.
    pub openid_token: Option<String>,
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
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct SlAgentParcel {
    /// The parcel the agent currently stands on, or `None` before it resolves.
    pub current: Option<ParcelInfo>,
    /// Whether the agent may start flying where it currently stands.
    pub can_fly: bool,
    /// The object the agent is currently seated on, or `None` if it is not seated
    /// on an object (standing, ground-sitting, or a sit request still pending).
    ///
    /// Mirrored from [`Session::seat`], which **keeps the seat across a plain
    /// region crossing** (a vehicle carries the agent over the border), so a
    /// consumer can trust this signal through a crossing without debouncing — it
    /// only clears on a real stand / teleport. The viewer routes a seated agent's
    /// steering keys to the vehicle rather than turning the avatar, and follows
    /// the seat with the camera. A temporary lodger here (the "agent state
    /// mirrored each frame" resource); the sit/stand task may move it to its own
    /// resource.
    pub seated_on: Option<ObjectKey>,
}

impl SlAgentParcel {
    /// Refreshes the mirror from the driven session: the fly permission and seat
    /// are read every frame (cheap), while the current parcel is re-cloned only
    /// when it actually changes (a structural compare avoids cloning its ~½ KiB
    /// bitmap and strings on every unchanged frame).
    pub(crate) fn refresh_from(&mut self, session: &Session) {
        self.can_fly = session.can_fly();
        self.seated_on = session.seat();
        let current = session.current_parcel();
        if self.current.as_ref() != current {
            self.current = current.cloned();
        }
    }
}

/// The reassembled parcel-ownership overlays — per region, the 64×64 grid of
/// per-square ownership colour and boundary/sound flags the simulator pushes as
/// four [`ParcelOverlay`](sl_proto::Event::ParcelOverlay) chunks.
///
/// The simulator sends a region's overlay unprompted on entry (there is no
/// overlay-request message — the reference viewer relies on the same push) and
/// re-broadcasts the whole overlay when a parcel is split, joined, or sold, so
/// this resource stays current simply by folding in each chunk. Chunks arrive
/// on the root circuit *and* on neighbour child circuits, each tagged with its
/// source region ([`ParcelOverlayInfo::region_handle`]), so neighbour regions
/// accumulate their own grids (Second Life pushes them on child-agent
/// establishment; OpenSim only on parcel changes). The **current** region's
/// grid is discarded and rebuilt on every region change, so a consumer never
/// reads a stale grid for the region it stands in: check
/// [`region`](Self::region) / [`is_complete`](Self::is_complete) before
/// trusting [`grid`](Self::grid).
///
/// Two consumers want these grids: the minimap parcel-colour overlay (all
/// regions, via [`grid_of`](Self::grid_of)) and the in-world sound clamp (the
/// `sound_local` bit) — the current region's grid stays available through
/// [`grid`](Self::grid).
#[derive(Resource, Default, Debug, Clone)]
pub struct SlParcelOverlay {
    /// The current (root) region, or `None` before the circuit is up. Chunks
    /// arriving without a region tag are attributed here.
    region: Option<RegionHandle>,
    /// The reassembled grid per region.
    grids: HashMap<RegionHandle, ParcelOverlayGrid>,
}

impl SlParcelOverlay {
    /// The current region the untagged chunks are attributed to, or `None`
    /// before any region is known.
    #[must_use]
    pub const fn region(&self) -> Option<RegionHandle> {
        self.region
    }

    /// The **current** region's reassembled overlay grid, or `None` before its
    /// first chunk arrives. Pair with [`is_complete`](Self::is_complete) to
    /// know whether every chunk is in.
    #[must_use]
    pub fn grid(&self) -> Option<&ParcelOverlayGrid> {
        self.grids.get(&self.region?)
    }

    /// The reassembled overlay grid of `region` (the current region or a
    /// neighbour), or `None` before its first chunk arrives.
    #[must_use]
    pub fn grid_of(&self, region: RegionHandle) -> Option<&ParcelOverlayGrid> {
        self.grids.get(&region)
    }

    /// Whether the current region's grid exists and every one of its chunks
    /// has arrived.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.grid().is_some_and(ParcelOverlayGrid::is_complete)
    }

    /// Folds one pushed overlay chunk into its region's grid, creating the
    /// grid (sized for a standard 256 m region) on the first chunk. An untagged
    /// chunk (region handle 0 — the source circuit was not yet associated)
    /// goes to the current region; a malformed chunk is logged and dropped,
    /// leaving the grid intact.
    fn ingest(&mut self, info: &ParcelOverlayInfo) {
        let region = if info.region_handle.0 == 0 {
            match self.region {
                Some(region) => region,
                None => {
                    tracing::warn!("dropping a parcel-overlay chunk with no attributable region");
                    return;
                }
            }
        } else {
            info.region_handle
        };
        let grid = self
            .grids
            .entry(region)
            .or_insert_with(|| ParcelOverlayGrid::new(DEFAULT_GRIDS_PER_EDGE));
        if let Err(error) = grid.ingest_chunk(info.sequence_id, &info.data) {
            tracing::warn!(%error, "dropping malformed parcel-overlay chunk");
        }
    }

    /// Records the region the next untagged chunks describe on a region
    /// change, discarding any grid already held for it — the destination
    /// re-pushes its overlay on entry, so the grid rebuilds from scratch and a
    /// consumer never reads a stale grid for the region the agent stands in.
    /// Neighbour grids are kept (their regions re-push on their own churn).
    fn reset_for_region(&mut self, region: RegionHandle) {
        self.region = Some(region);
        self.grids.remove(&region);
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
    mut overlay: ResMut<SlParcelOverlay>,
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
                    // Record the login region so the overlay grid, assembled from
                    // the chunks the sim pushes on entry, is attributed to it.
                    overlay.reset_for_region(handle);
                }
            }
            SessionEvent::RegionInfoHandshake(region_identity) => {
                // Attach the identity to the region it actually describes, keyed by
                // its handle — not merely whichever region is current. A neighbour's
                // `RegionHandshake` arrives on its child circuit (before any
                // crossing), so caching it on that neighbour's entity means a later
                // crossing shows the region name immediately instead of falling back
                // to "Connecting…". Attributing to "current" also mislabels the
                // current region when a neighbour's handshake lands while another
                // region is current. The handle is `0` only when the session never
                // learned one for the circuit; fall back to the current region then.
                let handle = region_identity.region_handle;
                let entity = if handle == RegionHandle(0) {
                    current_entity(&index)
                } else {
                    entity_for_handle(&index, handle)
                };
                tracing::debug!(
                    ?handle,
                    name = ?region_identity.sim_name,
                    attributed = entity.is_some(),
                    current = handle == RegionHandle(0),
                    "region handshake identity"
                );
                if let Some(entity) = entity {
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
            // Overlay chunks arrive unprompted on region entry (and after any
            // parcel edit), on the root and neighbour child circuits alike.
            // Fold each into its source region's grid.
            SessionEvent::ParcelOverlay(info) => {
                overlay.ingest(info);
            }
            // A teleport handover completed: the destination is now the root
            // region. Update the global handle and move the current marker.
            SessionEvent::RegionChanged {
                region_handle,
                sim,
                circuit,
                world_reset,
            } => {
                identity.region_handle = Some(*region_handle);
                identity.circuit_id = Some(*circuit);
                // A distant teleport purged the session's world caches (a fresh
                // circuit to an unconnected region); drop every mirrored region
                // — the source and its neighbours are all gone — before the
                // destination is spawned fresh below. A crossing / neighbour
                // teleport keeps them (they keep streaming as child circuits).
                if *world_reset {
                    clear_world(&mut commands, &mut index);
                }
                set_current_region(&mut commands, &mut index, *region_handle, *sim);
                // Drop the previous region's overlay; the destination re-pushes
                // its own on entry.
                overlay.reset_for_region(*region_handle);
            }
            SessionEvent::Disconnected(_) | SessionEvent::LoggedOut => {
                clear_world(&mut commands, &mut index);
                *overlay = SlParcelOverlay::default();
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

/// The entity of the region with the given `handle` (root or neighbour), if one
/// has been spawned. Used to attribute a `RegionHandshake` to the region it
/// describes rather than to whichever region is current.
fn entity_for_handle(index: &SlRegionIndex, handle: RegionHandle) -> Option<Entity> {
    index.by_handle.get(&handle).copied()
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
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use super::{
        SlCurrentRegion, SlIdentity, SlNeighbor, SlParcelOverlay, SlRegion, SlRegionIdentity,
        SlRegionIndex, maintain_world,
    };

    use std::net::SocketAddr;

    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use sl_proto::{
        CircuitId, Event as SessionEvent, GridCoordinates, Maturity, NeighborInfo,
        ParcelOverlayInfo, ParcelOwnership, ProductType, RegionHandle, RegionIdentity, RegionName,
        RegionTerrainComposition,
    };
    use uuid::Uuid;

    use crate::SlEvent;

    /// A loopback simulator address on the given port, for synthesising events.
    fn sim(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// A minimal [`RegionIdentity`] naming the region `name`, tagged with `handle`
    /// — enough to exercise how a `RegionHandshake` is attributed to a region.
    fn region_identity(handle: RegionHandle, name: &str) -> RegionIdentity {
        RegionIdentity {
            sim_name: RegionName::try_new(name).ok(),
            region_id: Uuid::nil(),
            region_handle: handle,
            grid_coordinates: GridCoordinates::new(1000, 1000),
            region_flags: 0,
            region_flags_extended: 0,
            region_protocols: 0,
            maturity: Maturity::Pg,
            product: ProductType::Unknown,
            product_sku: String::new(),
            product_name: String::new(),
            cpu_class_id: 0,
            cpu_ratio: 0,
            sim_owner: Uuid::nil(),
            is_estate_manager: false,
            water_height: 20.0,
            billable_factor: 1.0,
            terrain: RegionTerrainComposition {
                detail_textures: [Uuid::nil(); 4],
                start_heights: [0.0; 4],
                height_ranges: [0.0; 4],
            },
        }
    }

    /// A minimal app wired exactly like the plugin's world maintenance: the event
    /// channel, the identity + index + overlay resources, and the
    /// `maintain_world` system.
    fn world_app() -> App {
        let mut app = App::new();
        app.add_message::<SlEvent>()
            .init_resource::<SlIdentity>()
            .init_resource::<SlRegionIndex>()
            .init_resource::<SlParcelOverlay>()
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
                world_reset: false,
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

    /// A neighbour's `RegionHandshake` (delivered on its child circuit before any
    /// crossing) is cached on that neighbour's own entity — keyed by handle, not
    /// attributed to the current region — so crossing into it immediately shows
    /// the region's name instead of "Connecting…". Regression test for the
    /// top-bar-region-name-stuck-on-Connecting bug.
    #[test]
    fn a_neighbour_handshake_is_cached_on_its_own_region() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        let next = RegionHandle(0x0000_03e9_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        // The neighbour is discovered, then sends its handshake on the child
        // circuit while home is still the current region.
        app.world_mut()
            .write_message(SlEvent(SessionEvent::NeighborDiscovered(NeighborInfo {
                region_handle: next,
                sim: sim(9001),
                grid_coordinates: GridCoordinates::new(1001, 1000),
            })));
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionInfoHandshake(Box::new(
                region_identity(next, "Next Region"),
            ))));
        // The current region also gets its own handshake.
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionInfoHandshake(Box::new(
                region_identity(home, "Home Region"),
            ))));
        app.update();

        // The neighbour's identity landed on the neighbour, not on home — so the
        // handshakes did not clobber each other.
        let name_of = |app: &mut App, handle: RegionHandle| -> Option<String> {
            let mut query = app.world_mut().query::<(&SlRegion, &SlRegionIdentity)>();
            query
                .iter(app.world())
                .find(|(region, _identity)| region.handle == handle)
                .and_then(|(_region, identity)| {
                    identity.0.sim_name.as_ref().map(ToString::to_string)
                })
        };
        assert_eq!(name_of(&mut app, home), Some("Home Region".to_owned()));
        assert_eq!(name_of(&mut app, next), Some("Next Region".to_owned()));

        // Cross into the neighbour: it becomes current and already carries its
        // name, so the top bar reads it immediately (no "Connecting…").
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionChanged {
                region_handle: next,
                sim: sim(9001),
                circuit: CircuitId(2),
                world_reset: false,
            }));
        app.update();

        let mut current = app
            .world_mut()
            .query_filtered::<&SlRegionIdentity, With<SlCurrentRegion>>();
        let current_name = current
            .single(app.world())
            .ok()
            .and_then(|identity| identity.0.sim_name.as_ref().map(ToString::to_string));
        assert_eq!(current_name, Some("Next Region".to_owned()));
    }

    /// A handshake whose handle the session never learned (`0`) still lands on the
    /// current region — the pre-per-region fallback, so an unattributed handshake
    /// is not dropped.
    #[test]
    fn a_handleless_handshake_falls_back_to_the_current_region() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionInfoHandshake(Box::new(
                region_identity(RegionHandle(0), "Home Region"),
            ))));
        app.update();

        let mut current = app
            .world_mut()
            .query_filtered::<&SlRegionIdentity, With<SlCurrentRegion>>();
        let current_name = current
            .single(app.world())
            .ok()
            .and_then(|identity| identity.0.sim_name.as_ref().map(ToString::to_string));
        assert_eq!(current_name, Some("Home Region".to_owned()));
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

    /// Synthesises the `c`-th overlay chunk of a standard 256 m region: a
    /// 1024-byte southern band whose squares all carry ownership class `class`.
    /// Untagged (region handle 0), as a root-circuit chunk decodes before the
    /// circuit associates.
    fn overlay_chunk(sequence_id: i32, class: u8) -> ParcelOverlayInfo {
        ParcelOverlayInfo {
            sequence_id,
            data: vec![class; 1024],
            region_handle: RegionHandle(0),
        }
    }

    /// A chunk tagged with its source region, as a neighbour child circuit
    /// (or an associated root circuit) delivers it.
    fn overlay_chunk_for(region: RegionHandle, sequence_id: i32, class: u8) -> ParcelOverlayInfo {
        ParcelOverlayInfo {
            region_handle: region,
            ..overlay_chunk(sequence_id, class)
        }
    }

    /// Chunks tagged with a neighbour region assemble into that region's own
    /// grid without touching the current region's, and both stay readable
    /// through [`SlParcelOverlay::grid_of`].
    #[test]
    fn neighbour_overlay_chunks_assemble_per_region() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        let neighbour = RegionHandle(0x0000_03e9_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        for sequence in 0..4 {
            // The home region public, the neighbour self-owned.
            app.world_mut()
                .write_message(SlEvent(SessionEvent::ParcelOverlay(overlay_chunk(
                    sequence, 0x00,
                ))));
            app.world_mut()
                .write_message(SlEvent(SessionEvent::ParcelOverlay(overlay_chunk_for(
                    neighbour, sequence, 0x03,
                ))));
        }
        app.update();
        let overlay = app.world().resource::<SlParcelOverlay>();
        assert_eq!(overlay.region(), Some(home));
        assert!(overlay.is_complete());
        assert_eq!(
            overlay
                .grid_of(home)
                .and_then(|grid| grid.cell(0, 0))
                .map(|cell| cell.ownership),
            Some(ParcelOwnership::Public)
        );
        assert_eq!(
            overlay
                .grid_of(neighbour)
                .and_then(|grid| grid.cell(0, 0))
                .map(|cell| cell.ownership),
            Some(ParcelOwnership::SelfOwned)
        );
    }

    /// The four pushed overlay chunks assemble into a complete grid attributed
    /// to the login region.
    #[test]
    fn overlay_chunks_assemble_for_the_login_region() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        // Chunk 0 (southern band) is self-owned, the rest public.
        for chunk in [
            overlay_chunk(0, 0x03),
            overlay_chunk(1, 0x00),
            overlay_chunk(2, 0x00),
            overlay_chunk(3, 0x00),
        ] {
            app.world_mut()
                .write_message(SlEvent(SessionEvent::ParcelOverlay(chunk)));
        }
        app.update();

        let overlay = app.world().resource::<SlParcelOverlay>();
        assert_eq!(overlay.region(), Some(home));
        assert!(overlay.is_complete(), "all four chunks arrived");
        let grid = overlay.grid().expect("a grid exists");
        assert_eq!(
            grid.cell(0, 0).expect("on grid").ownership,
            ParcelOwnership::SelfOwned
        );
        assert_eq!(
            grid.cell(63, 0).expect("on grid").ownership,
            ParcelOwnership::Public
        );
    }

    /// Crossing into a new region drops the old overlay and re-attributes the
    /// resource to the destination, so a stale grid is never read.
    #[test]
    fn a_region_change_invalidates_the_overlay() {
        let mut app = world_app();
        let home = RegionHandle(0x0000_03e8_0000_03e8);
        let next = RegionHandle(0x0000_03e9_0000_03e8);
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(home);
        app.world_mut()
            .write_message(SlEvent(SessionEvent::CircuitEstablished {
                sim: sim(9000),
                circuit: CircuitId(1),
            }));
        for chunk in [
            overlay_chunk(0, 0x03),
            overlay_chunk(1, 0x03),
            overlay_chunk(2, 0x03),
            overlay_chunk(3, 0x03),
        ] {
            app.world_mut()
                .write_message(SlEvent(SessionEvent::ParcelOverlay(chunk)));
        }
        app.update();
        assert!(app.world().resource::<SlParcelOverlay>().is_complete());

        // Teleport into the neighbour: the overlay is cleared, pending the
        // destination's own push.
        app.world_mut()
            .write_message(SlEvent(SessionEvent::RegionChanged {
                region_handle: next,
                sim: sim(9001),
                circuit: CircuitId(2),
                world_reset: false,
            }));
        app.update();
        let overlay = app.world().resource::<SlParcelOverlay>();
        assert_eq!(overlay.region(), Some(next));
        assert!(
            overlay.grid().is_none(),
            "the old region's grid was dropped"
        );
        assert!(!overlay.is_complete());

        // The destination re-pushes its overlay, which rebuilds under the new
        // region.
        for sequence in 0..4 {
            app.world_mut()
                .write_message(SlEvent(SessionEvent::ParcelOverlay(overlay_chunk(
                    sequence, 0x00,
                ))));
        }
        app.update();
        let overlay = app.world().resource::<SlParcelOverlay>();
        assert!(overlay.is_complete());
        assert_eq!(overlay.region(), Some(next));
    }
}
