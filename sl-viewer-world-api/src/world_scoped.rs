//! World-scoped stores: what a **distant** teleport throws away.
//!
//! A region *crossing* and a teleport to an *already-connected* (neighbour)
//! region keep the whole world and merely re-base it onto the new origin. A
//! **distant** teleport instead mints a fresh circuit to an unconnected region:
//! the session clears its object / terrain / region caches with *no* per-object
//! `KillObject`, so nothing drives the incremental despawn path and the old
//! region's entities — and every map keyed by them — would linger forever.
//!
//! The viewer used to answer that with one system holding a hand-written list of
//! the stores to purge. A list is exactly the wrong shape for this: it is
//! written once, every later store is added somewhere else entirely, and nothing
//! ever fails when one is missed — the store just grows silently for the whole
//! session. Five stores were on the list and five more that needed to be were
//! not.
//!
//! So a store declares its own scope instead. [`WorldScoped`] is implemented
//! next to the store, [`WorldScopedAppExt::init_world_scoped`] replaces the
//! `init_resource` that would otherwise have registered it, and the purge is
//! wired up by the same line that creates the store — there is no second place
//! to forget.
//!
//! # Which stores are world-scoped
//!
//! A store is world-scoped when **either**:
//!
//! - its keys only mean something inside one connected world — a
//!   [`RegionHandle`](sl_client_bevy::RegionHandle), a
//!   [`ScopedObjectId`](sl_client_bevy::ScopedObjectId), or an
//!   [`ObjectKey`](sl_client_bevy::ObjectKey) of an object only the departed
//!   region streams; **or**
//! - it holds [`Entity`] ids that the purge despawns, so surviving entries
//!   point at dead entities.
//!
//! It is **not** world-scoped when it is an asset cache keyed by a grid-wide
//! UUID (decoded textures, meshes, materials): the destination may well want the
//! same asset, and a refetch is far more expensive than the memory. Nor when it
//! is user state that outlives the world (mutes, contact sets, per-agent render
//! overrides), or when it already reconciles itself against the live region set
//! every frame (the parcel-border bands do).
//!
//! # Ordering
//!
//! [`WorldResetSystems::Detect`] folds the session event stream into
//! [`WorldResetFrame`]; [`WorldResetSystems::Purge`] runs every registered
//! purge, chained after it. The host orders `Purge` before its re-centring
//! systems, so each subsystem's origin — dropped to `None` by its own purge —
//! is re-anchored on the destination without a spurious re-base shift.

#![expect(
    clippy::module_name_repetitions,
    reason = "the module is named for the one concept it owns, so its items read \
              as `world_scoped::WorldScopedRegistry` at every call site; the \
              codebase names them for the trait, not for the module path"
)]

use std::any::{TypeId, type_name};
use std::collections::BTreeMap;

use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, SlEvent, SlIdentity, SlSessionEvent};

/// What a purge needs to know about the world being left behind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorldPurge {
    /// The logged-in agent, whose own avatar, skeleton, appearance and worn
    /// attachments all cross *with* it and are therefore kept — despawning them
    /// would flash the self view and force an appearance / bake refetch.
    /// `None` before login.
    pub own_agent: Option<AgentKey>,
}

/// A store whose contents belong to **one** connected world, and which is
/// therefore emptied when a distant teleport replaces that world.
///
/// Implement it next to the store and register the store with
/// [`WorldScopedAppExt::init_world_scoped`] (or, when the store is inserted at
/// runtime rather than initialised by its plugin,
/// [`WorldScopedAppExt::register_world_scoped`]).
pub trait WorldScoped: Resource<Mutability = Mutable> {
    /// Drop everything that belonged to the departed world, despawning the
    /// entities it owns through `commands`. Anything the destination would
    /// simply have to fetch again — a shared material, a placeholder mesh, an
    /// asset handle keyed by a grid-wide UUID — is kept.
    fn purge_world(&mut self, purge: WorldPurge, commands: &mut Commands);
}

/// The two phases of a world reset, in order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldResetSystems {
    /// Folds the session event stream into [`WorldResetFrame`].
    Detect,
    /// Purges every registered [`WorldScoped`] store. Runs after
    /// [`Detect`](Self::Detect), and the host orders it before its re-centring
    /// systems.
    Purge,
}

/// Whether *this* frame carries a distant teleport's world reset.
///
/// Set by [`detect_world_reset`] at the top of every frame, so it is true for
/// exactly the frame whose event stream carried the reset — the purge systems
/// read it through the [`world_was_reset`] run condition.
#[derive(Resource, Debug, Default)]
pub struct WorldResetFrame(bool);

impl WorldResetFrame {
    /// Whether this frame is a world reset.
    #[must_use]
    pub const fn is_reset(&self) -> bool {
        self.0
    }
}

/// Every [`WorldScoped`] store registered with the app, by type name.
///
/// Kept so the purge can be *seen*: the debug log names what it emptied, and a
/// test can assert that a store it cares about is actually wired up rather than
/// trusting that someone remembered.
#[derive(Resource, Debug, Default)]
pub struct WorldScopedRegistry {
    /// The registered stores' type names, keyed by type so a double
    /// registration counts once.
    stores: BTreeMap<TypeId, &'static str>,
}

impl WorldScopedRegistry {
    /// The registered stores' type names, in a stable (by-`TypeId`) order.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.stores.values().copied()
    }

    /// Whether `T` is registered as world-scoped.
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

/// Fold the frame's session events into [`WorldResetFrame`].
///
/// Only [`SlSessionEvent::RegionChanged`]'s `world_reset` flag counts — the
/// session sets it exactly on the fresh-circuit branch, so a crossing or a
/// neighbour teleport (which keep the world and re-base it) leave the flag
/// false. A burst of them in one frame is still one reset.
pub fn detect_world_reset(mut events: MessageReader<SlEvent>, mut frame: ResMut<WorldResetFrame>) {
    let reset = events.read().any(|event| {
        matches!(
            &event.0,
            SlSessionEvent::RegionChanged {
                world_reset: true,
                ..
            }
        )
    });
    // Write-on-change: the flag is false on almost every frame of a session, and
    // rewriting it would mark the resource changed forever.
    if frame.0 != reset {
        frame.0 = reset;
    }
}

/// Run condition: this frame is a world reset.
///
/// Tolerates a missing [`WorldResetFrame`] (a test app that added a store's
/// plugin but not [`WorldScopedPlugin`]) by never running.
#[must_use]
pub fn world_was_reset(frame: Option<Res<WorldResetFrame>>) -> bool {
    frame.is_some_and(|frame| frame.is_reset())
}

/// Purge one registered store. Generic over the store, so the registration is a
/// single turbofished line and nothing has to be repeated per store.
///
/// The store is optional because some are inserted at runtime by a startup
/// system rather than initialised by their plugin — a reset before that has run
/// has nothing to purge.
fn purge_world_scoped<T: WorldScoped>(
    identity: Option<Res<SlIdentity>>,
    store: Option<ResMut<T>>,
    mut commands: Commands,
) {
    let Some(mut store) = store else {
        return;
    };
    let purge = WorldPurge {
        own_agent: identity.and_then(|identity| identity.agent_id),
    };
    store.purge_world(purge, &mut commands);
    debug!("world reset: purged {}", type_name::<T>());
}

/// Name every registered store once, at startup.
///
/// The failure mode this whole module exists to prevent — a store that is
/// world-scoped in fact but never registered — is otherwise invisible: nothing
/// errors, the store simply keeps growing. One line naming the set makes the
/// omission something a log can be read for.
fn log_world_scoped_registry(registry: Res<WorldScopedRegistry>) {
    let names: Vec<&str> = registry.names().collect();
    debug!(
        "{} world-scoped store(s) purge on a distant teleport: {}",
        registry.len(),
        names.join(", ")
    );
}

/// Wires the world-reset phases up. Added automatically by the first
/// [`WorldScopedAppExt`] call, so a store's plugin never has to remember it.
#[derive(Debug)]
pub struct WorldScopedPlugin;

impl Plugin for WorldScopedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldResetFrame>()
            .init_resource::<WorldScopedRegistry>()
            .configure_sets(
                Update,
                (WorldResetSystems::Detect, WorldResetSystems::Purge).chain(),
            )
            .add_systems(Startup, log_world_scoped_registry)
            .add_systems(Update, detect_world_reset.in_set(WorldResetSystems::Detect));
    }
}

/// Registering a [`WorldScoped`] store with the app.
pub trait WorldScopedAppExt {
    /// Initialise `T` **and** register its purge — the world-scoped replacement
    /// for `init_resource::<T>()`. Use this wherever the store would otherwise
    /// have been initialised, so creating the store and scoping it are one line
    /// and cannot drift apart.
    fn init_world_scoped<T: WorldScoped + FromWorld>(&mut self) -> &mut Self;

    /// Register `T`'s purge without initialising it — for a store some startup
    /// system inserts once it has the assets it needs (the water planes' shared
    /// material and mesh, say).
    fn register_world_scoped<T: WorldScoped>(&mut self) -> &mut Self;
}

impl WorldScopedAppExt for App {
    fn init_world_scoped<T: WorldScoped + FromWorld>(&mut self) -> &mut Self {
        self.init_resource::<T>().register_world_scoped::<T>()
    }

    fn register_world_scoped<T: WorldScoped>(&mut self) -> &mut Self {
        if !self.is_plugin_added::<WorldScopedPlugin>() {
            self.add_plugins(WorldScopedPlugin);
        }
        let mut registry = self.world_mut().resource_mut::<WorldScopedRegistry>();
        if registry
            .stores
            .insert(TypeId::of::<T>(), type_name::<T>())
            .is_some()
        {
            // Two plugins claiming the same store would add the purge twice —
            // harmless (the second purge finds an empty store) but a sign the
            // ownership is unclear, so say so rather than hide it.
            warn!("world-scoped store {} registered twice", type_name::<T>());
            return self;
        }
        self.add_systems(
            Update,
            purge_world_scoped::<T>
                .in_set(WorldResetSystems::Purge)
                .run_if(world_was_reset),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WorldPurge, WorldResetFrame, WorldScoped, WorldScopedAppExt as _, WorldScopedPlugin,
        WorldScopedRegistry,
    };
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{CircuitId, RegionHandle, SlEvent, SlSessionEvent};

    /// A stand-in world-scoped store: counts its purges and drops its entries.
    #[derive(Resource, Default)]
    struct Store {
        /// Entries the departed world owned.
        entries: Vec<u32>,
        /// How many times the store was purged.
        purges: u32,
        /// The agent the last purge said to keep.
        last_own: Option<sl_client_bevy::AgentKey>,
    }

    impl WorldScoped for Store {
        fn purge_world(&mut self, purge: WorldPurge, _commands: &mut Commands) {
            self.entries.clear();
            self.purges = self.purges.saturating_add(1);
            self.last_own = purge.own_agent;
        }
    }

    /// A minimal app with the message stream a detect needs.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_message::<SlEvent>();
        app
    }

    /// A `RegionChanged` with the given reset flag.
    fn region_changed(world_reset: bool) -> SlEvent {
        SlEvent(SlSessionEvent::RegionChanged {
            region_handle: RegionHandle::from_global(256_000, 256_000),
            sim: std::net::SocketAddr::from(([127, 0, 0, 1], 9000)),
            circuit: CircuitId::new(1),
            world_reset,
        })
    }

    #[test]
    fn a_distant_teleport_purges_a_registered_store() {
        let mut app = test_app();
        app.init_world_scoped::<Store>();
        app.world_mut().resource_mut::<Store>().entries.push(7);

        app.world_mut().write_message(region_changed(true));
        app.update();

        let store = app.world().resource::<Store>();
        assert!(
            store.entries.is_empty(),
            "the departed world's entries stay"
        );
        assert_eq!(store.purges, 1);
    }

    #[test]
    fn a_crossing_purges_nothing() {
        let mut app = test_app();
        app.init_world_scoped::<Store>();
        app.world_mut().resource_mut::<Store>().entries.push(7);

        app.world_mut().write_message(region_changed(false));
        app.update();

        let store = app.world().resource::<Store>();
        assert_eq!(store.entries, vec![7], "a crossing keeps the world");
        assert_eq!(store.purges, 0);
        assert!(!app.world().resource::<WorldResetFrame>().is_reset());
    }

    #[test]
    fn the_reset_flag_lasts_exactly_one_frame() {
        let mut app = test_app();
        app.init_world_scoped::<Store>();

        app.world_mut().write_message(region_changed(true));
        app.update();
        assert_eq!(app.world().resource::<Store>().purges, 1);

        app.update();
        assert!(!app.world().resource::<WorldResetFrame>().is_reset());
        assert_eq!(
            app.world().resource::<Store>().purges,
            1,
            "a quiet frame must not purge again"
        );
    }

    #[test]
    fn a_store_inserted_later_is_purged_once_it_exists() {
        let mut app = test_app();
        app.register_world_scoped::<Store>();

        // No resource yet: the reset must not panic on the missing store.
        app.world_mut().write_message(region_changed(true));
        app.update();

        app.world_mut().insert_resource(Store {
            entries: vec![1, 2],
            ..Store::default()
        });
        app.world_mut().write_message(region_changed(true));
        app.update();

        assert_eq!(app.world().resource::<Store>().purges, 1);
    }

    #[test]
    fn registering_names_the_store_and_installs_the_plugin() {
        let mut app = test_app();
        app.init_world_scoped::<Store>();

        assert!(app.is_plugin_added::<WorldScopedPlugin>());
        let registry = app.world().resource::<WorldScopedRegistry>();
        assert!(registry.contains::<Store>());
        assert_eq!(registry.len(), 1);
        assert!(
            registry
                .names()
                .any(|name| name.ends_with("world_scoped::tests::Store"))
        );
    }

    #[test]
    fn a_double_registration_counts_once() {
        let mut app = test_app();
        app.init_world_scoped::<Store>();
        app.register_world_scoped::<Store>();

        assert_eq!(app.world().resource::<WorldScopedRegistry>().len(), 1);

        app.world_mut().resource_mut::<Store>().entries.push(7);
        app.world_mut().write_message(region_changed(true));
        app.update();
        assert_eq!(
            app.world().resource::<Store>().purges,
            1,
            "the second registration must not add a second purge"
        );
    }
}
