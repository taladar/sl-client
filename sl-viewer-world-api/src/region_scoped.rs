//! Region-scoped stores: what a **retired neighbour** takes with it.
//!
//! A simulator takes a neighbouring region away with a `DisableSimulator` down
//! its child circuit once the agent's draw distance stops reaching it (within
//! a second on OpenSim, fifty seconds later on Second Life —
//! `book/src/gridspec/teleport.md`, *Neighbours and crossings*). The session
//! reports the region's objects removed and raises
//! [`SlSessionEvent::NeighborRetired`]; `sl-client-bevy` drops the region's
//! entity and parcel overlay. Everything else the viewer keeps per region —
//! its land patches and their material, its sea level, its time dilation —
//! has to go too, or the ground of a region the grid stopped sending stays
//! drawn, frozen, where the reference shows water (`LLWorld::removeRegion`).
//!
//! This is the per-region counterpart of [`crate::world_scoped`], built the
//! same way and for the same reason: a store declares its own scope with
//! [`RegionScoped`], and [`RegionScopedAppExt::register_region_scoped`] wires
//! its purge up next to the line that creates it, so there is no central list
//! to forget a store on. A store that is world-scoped *because* it is keyed
//! by region is nearly always region-scoped too.
//!
//! # What is not region-scoped
//!
//! The same exclusions as a world reset — asset caches keyed by grid-wide
//! UUIDs, user state — plus what deliberately outlives a region's circuit:
//! the retained land cache ([`TerrainState::land_cache`]) that answers ground
//! heights while live patches are absent, and whatever already reconciles
//! itself against the live [`SlRegion`](sl_client_bevy::SlRegion) set every
//! frame (the parcel-border bands, the minimap's backdrops).
//!
//! # Ordering
//!
//! The retirement is detected in [`WorldResetSystems::Detect`] and each purge
//! runs in [`WorldResetSystems::Purge`], so a retirement is ordered exactly as
//! a world reset is: after the session drain, before the host's re-centring
//! and the folds that rebuild from the next events.
//!
//! [`TerrainState::land_cache`]: crate::TerrainState::land_cache

#![expect(
    clippy::module_name_repetitions,
    reason = "the module is named for the one concept it owns, as `world_scoped` \
              is; the codebase names its items for the trait, not the module path"
)]

use std::any::{TypeId, type_name};
use std::collections::BTreeMap;

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use sl_client_bevy::{RegionHandle, SlEvent, SlIdentity, SlSessionEvent};

use crate::world_scoped::{WorldResetSystems, WorldScopedPlugin};

/// A store holding something of **one region**, which is therefore emptied of
/// that region when the simulator retires it as a neighbour.
///
/// Implement it next to the store and register the store with
/// [`RegionScopedAppExt::register_region_scoped`] beside wherever it is
/// initialised.
pub trait RegionScoped: Resource<Mutability = Mutable> {
    /// Drop everything that belonged to `region`, despawning the entities it
    /// owns through `commands`. Never called for the region the agent stands
    /// in.
    fn forget_region(&mut self, region: RegionHandle, commands: &mut Commands);
}

/// The neighbours retired in *this* frame, in the order they were retired.
///
/// Set by [`detect_retired_regions`] at the top of every frame and read by
/// every registered purge through the [`regions_were_retired`] run condition.
#[derive(Resource, Debug, Default)]
pub struct RetiredRegionsFrame(Vec<RegionHandle>);

impl RetiredRegionsFrame {
    /// The regions retired this frame.
    #[must_use]
    pub fn regions(&self) -> &[RegionHandle] {
        &self.0
    }
}

/// Every [`RegionScoped`] store registered with the app, by type name — so a
/// test can assert a store it cares about is wired up, as
/// [`WorldScopedRegistry`](crate::world_scoped::WorldScopedRegistry) does.
#[derive(Resource, Debug, Default)]
pub struct RegionScopedRegistry {
    /// The registered stores' type names, keyed by type so a double
    /// registration counts once.
    stores: BTreeMap<TypeId, &'static str>,
}

impl RegionScopedRegistry {
    /// The registered stores' type names, in a stable (by-`TypeId`) order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.stores.values().copied()
    }

    /// Whether `T` is registered as region-scoped.
    #[must_use]
    pub fn contains<T: 'static>(&self) -> bool {
        self.stores.contains_key(&TypeId::of::<T>())
    }

    /// How many distinct stores are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stores.len()
    }

    /// Whether no store is registered at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stores.is_empty()
    }
}

/// Fold the frame's session events into [`RetiredRegionsFrame`].
///
/// A retirement naming the region the agent now stands in is skipped, as
/// `sl-client-bevy`'s own region index skips it: a child circuit's
/// `DisableSimulator` that raced a crossing into the same region must not take
/// the ground from under the agent. A retirement with no handle (a circuit the
/// session never learned a region for) names nothing to forget.
pub fn detect_retired_regions(
    mut events: MessageReader<SlEvent>,
    identity: Option<Res<SlIdentity>>,
    mut frame: ResMut<RetiredRegionsFrame>,
) {
    let current = identity.and_then(|identity| identity.region_handle);
    let mut retired = Vec::new();
    for event in events.read() {
        if let SlSessionEvent::NeighborRetired {
            region_handle: Some(region),
            reason,
            ..
        } = &event.0
        {
            if Some(*region) == current {
                debug!("neighbour {region} retired ({reason:?}) but is now current: kept");
                continue;
            }
            info!("neighbour {region} retired ({reason:?}): its ground, sea and state are dropped");
            if !retired.contains(region) {
                retired.push(*region);
            }
        }
    }
    // Write-on-change: almost every frame retires nothing.
    if frame.0 != retired {
        frame.0 = retired;
    }
}

/// Run condition: this frame retired at least one neighbour.
///
/// Tolerates a missing [`RetiredRegionsFrame`] by never running.
#[must_use]
pub fn regions_were_retired(frame: Option<Res<RetiredRegionsFrame>>) -> bool {
    frame.is_some_and(|frame| !frame.0.is_empty())
}

/// Purge this frame's retired regions from one registered store. Generic over
/// the store so a registration is a single turbofished line.
///
/// The store is optional because some are inserted at runtime by a startup
/// system rather than initialised by their plugin.
fn purge_region_scoped<T: RegionScoped>(
    frame: Res<RetiredRegionsFrame>,
    store: Option<ResMut<T>>,
    mut commands: Commands,
) {
    let Some(mut store) = store else {
        return;
    };
    for &region in frame.regions() {
        store.forget_region(region, &mut commands);
        debug!("neighbour {region} retired: purged {}", type_name::<T>());
    }
}

/// Wires the retirement detection up. Added automatically by the first
/// [`RegionScopedAppExt`] call.
#[derive(Debug)]
pub struct RegionScopedPlugin;

impl Plugin for RegionScopedPlugin {
    fn build(&self, app: &mut App) {
        // The phases are the world reset's, so the session-drain edge and the
        // host's purge-before-recentre edge hold for a retirement too.
        if !app.is_plugin_added::<WorldScopedPlugin>() {
            app.add_plugins(WorldScopedPlugin);
        }
        app.init_resource::<RetiredRegionsFrame>()
            .init_resource::<RegionScopedRegistry>()
            .add_systems(
                Update,
                detect_retired_regions.in_set(WorldResetSystems::Detect),
            );
    }
}

/// Registering a [`RegionScoped`] store with the app.
pub trait RegionScopedAppExt {
    /// Register `T`'s per-region purge. The store itself is initialised (or
    /// inserted at runtime) as before; call this beside that line.
    fn register_region_scoped<T: RegionScoped>(&mut self) -> &mut Self;
}

impl RegionScopedAppExt for App {
    fn register_region_scoped<T: RegionScoped>(&mut self) -> &mut Self {
        if !self.is_plugin_added::<RegionScopedPlugin>() {
            self.add_plugins(RegionScopedPlugin);
        }
        let mut registry = self.world_mut().resource_mut::<RegionScopedRegistry>();
        if registry
            .stores
            .insert(TypeId::of::<T>(), type_name::<T>())
            .is_some()
        {
            warn!("region-scoped store {} registered twice", type_name::<T>());
            return self;
        }
        self.add_systems(
            Update,
            purge_region_scoped::<T>
                .in_set(WorldResetSystems::Purge)
                .run_if(regions_were_retired),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RegionScoped, RegionScopedAppExt as _, RegionScopedPlugin, RegionScopedRegistry,
        RetiredRegionsFrame,
    };
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        CircuitId, NeighborRetirement, RegionHandle, SlEvent, SlIdentity, SlSessionEvent,
    };

    /// The region the agent stands in.
    const HOME: RegionHandle = RegionHandle((256_000_u64 << 32) | 256_000);

    /// The neighbour to its east.
    const EAST: RegionHandle = RegionHandle((256_256_u64 << 32) | 256_000);

    /// The neighbour to its north.
    const NORTH: RegionHandle = RegionHandle((256_000_u64 << 32) | 256_256);

    /// A stand-in region-scoped store: one entry per region, and the order the
    /// purges named them in.
    #[derive(Resource, Default)]
    struct Store {
        /// The regions the store holds something of.
        regions: Vec<RegionHandle>,
        /// The regions each purge was asked to forget, in order.
        forgotten: Vec<RegionHandle>,
    }

    impl RegionScoped for Store {
        fn forget_region(&mut self, region: RegionHandle, _commands: &mut Commands) {
            self.regions.retain(|held| *held != region);
            self.forgotten.push(region);
        }
    }

    /// An app holding [`Store`] with all three regions, the agent in `HOME`.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_message::<SlEvent>()
            .init_resource::<SlIdentity>()
            .insert_resource(Store {
                regions: vec![HOME, EAST, NORTH],
                forgotten: Vec::new(),
            })
            .register_region_scoped::<Store>();
        app.world_mut().resource_mut::<SlIdentity>().region_handle = Some(HOME);
        app
    }

    /// A `NeighborRetired` naming `region`.
    fn retired(region: Option<RegionHandle>) -> SlEvent {
        SlEvent(SlSessionEvent::NeighborRetired {
            region_handle: region,
            sim: std::net::SocketAddr::from(([127, 0, 0, 1], 9001)),
            circuit: CircuitId::new(2),
            reason: NeighborRetirement::Disabled,
        })
    }

    #[test]
    fn a_retired_neighbour_is_forgotten_and_nothing_else() {
        let mut app = test_app();
        app.world_mut().write_message(retired(Some(EAST)));
        app.update();

        let store = app.world().resource::<Store>();
        assert_eq!(store.regions, vec![HOME, NORTH]);
        assert_eq!(store.forgotten, vec![EAST]);
    }

    #[test]
    fn the_current_region_is_never_forgotten() {
        let mut app = test_app();
        app.world_mut().write_message(retired(Some(HOME)));
        app.world_mut().write_message(retired(None));
        app.update();

        let store = app.world().resource::<Store>();
        assert_eq!(store.regions, vec![HOME, EAST, NORTH]);
        assert!(store.forgotten.is_empty(), "{:?}", store.forgotten);
    }

    #[test]
    fn a_burst_forgets_each_region_once_and_only_in_its_frame() {
        let mut app = test_app();
        app.world_mut().write_message(retired(Some(EAST)));
        app.world_mut().write_message(retired(Some(NORTH)));
        app.world_mut().write_message(retired(Some(EAST)));
        app.update();
        assert_eq!(
            app.world().resource::<RetiredRegionsFrame>().regions(),
            [EAST, NORTH]
        );

        app.update();
        assert!(
            app.world()
                .resource::<RetiredRegionsFrame>()
                .regions()
                .is_empty()
        );
        let store = app.world().resource::<Store>();
        assert_eq!(store.regions, vec![HOME]);
        assert_eq!(
            store.forgotten,
            vec![EAST, NORTH],
            "a quiet frame forgets nothing again"
        );
    }

    #[test]
    fn registering_names_the_store_and_installs_the_plugin() {
        let mut app = test_app();
        app.register_region_scoped::<Store>();

        assert!(app.is_plugin_added::<RegionScopedPlugin>());
        let registry = app.world().resource::<RegionScopedRegistry>();
        assert!(registry.contains::<Store>());
        assert_eq!(registry.len(), 1, "a double registration counts once");
    }
}
