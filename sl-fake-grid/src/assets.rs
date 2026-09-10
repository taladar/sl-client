//! The grid-wide binary asset store.
//!
//! An asset id on a real grid names a blob the **whole grid** knows about:
//! textures, meshes, animations, sounds and settings live in an asset service
//! behind every region, and a viewer standing in one region routinely fetches
//! ids that only another region's content references. Its own `GetTexture` /
//! `GetMesh2` / `ViewerAsset` capabilities are the root region's, and it asks
//! them for everything — including the textures of the neighbour region it can
//! see across the border but is not standing in.
//!
//! So the fake grid keeps one store, shared by every region and every session.
//! A [`RegionFixture`](crate::RegionFixture) still *describes* the assets its
//! own content needs — that is where a fixture author states them — and the
//! builder folds every region's into this one store when the grid starts.
//!
//! # Locking
//!
//! A plain `std` lock, not an async one, because the one writer runs inside the
//! driver's synchronous flush rule (an arriving agent's own bakes are minted
//! from its agent id, which is only known then) and the readers hold it for a
//! `HashMap` lookup and a copy. **Every path takes the session lock before this
//! one**, never the other way round, so the two can never deadlock against each
//! other.
//!
//! Poisoning is recovered from rather than propagated: the store is bytes with
//! no invariant a panicking writer could have broken half of, and a fake grid
//! that stopped serving textures because an unrelated task panicked would hide
//! the panic behind a much more confusing symptom.
//!
//! # Two stores, because the two live grids disagree
//!
//! There is a second store beside the served one, and it holds exactly one
//! thing: the body of an object a take filed away on a grid configured to
//! imitate Second Life ([`ObjectAssetPolicy::Withheld`]). Nothing serves it —
//! that is the point — and the only reader is the rez path, which resolves an
//! item's body by the **item's** id rather than by an asset id the viewer was
//! never told. See [`ObjectAssetPolicy`] for why.
//!
//! # The body is whichever format the item's own grid writes
//!
//! `AssetType::Object` is **two formats on the two grids**, so the store that
//! took the body decides which one it holds:
//!
//! - a **withheld** body is the Linden text (`sl_object_asset`), the format
//!   Second Life is known to have written;
//! - a **served** body is the `<SceneObjectGroup>` XML
//!   (`sl_object_asset::opensim`), which is what OpenSim stores under this
//!   class and the only thing a viewer fetching it there could be handed.
//!
//! Writing the text under [`ObjectAssetPolicy::Served`] would name OpenSim and
//! serve bytes no OpenSim ever wrote, which is the one thing about that policy
//! that was not faithful.
//!
//! # And a third store, because the *text* cannot hold a whole prim
//!
//! The Linden text has no keyword for a face's glow or material id, for the
//! `ExtraParams` block (flexi, light, sculpt, **mesh**, light image, extended
//! mesh, render material, reflection probe), for floating text, a media URL, a
//! texture animation or a particle system. `sl_object_asset::bridge`'s own
//! `the_text_carries_none_of_the_modern_prim` is the record of that. A grid
//! that rezzed out of those bytes would hand a resident who took a light back
//! a plain box.
//!
//! Neither live grid does that. OpenSim's XML carries all of it; Second Life's
//! simulator has the object itself and never has to read an asset at all. So
//! the third store is the fake grid having the same thing they have: **the
//! linkset a take removed**, kept under the item that stands for it, and
//! rezzed from in preference to either body.
//!
//! The stores do not compete. A body is what a viewer may *fetch* — under
//! [`ObjectAssetPolicy::Served`], the only configuration where one crosses the
//! wire — and stays exactly the bytes its format says. The linkset is what the
//! *simulator* rezzes from, and is nobody else's business. An item with no
//! linkset behind it (the seeded `Fixture Object`, which no take ever made)
//! still rezzes from its body, which is the whole reason that fixture exists.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use sl_proto::{InMemoryAssetSource, Object};
use sl_types::key::InventoryKey;

/// What a take's object asset is worth to a viewer: the `AssetType::Object`
/// half of [`ImitatedGrid`](crate::ImitatedGrid), the class where the two live
/// grids disagree about whether a viewer may see an asset at all.
///
/// A grid takes its side from the live grid it is imitating
/// ([`ImitatedGrid::object_assets`](crate::ImitatedGrid::object_assets)); set it
/// directly only to make a grid that is deliberately one grid about everything
/// else and the other about this.
///
/// Measured, not assumed. On **Second Life** (aditi, 2026-09-06, the
/// `object-asset-format` conformance case) a viewer is given no asset id for an
/// object inventory item: eleven of eleven object items answered with a nil
/// `asset_id` in the AIS3 folder listing *and* again in the per-item
/// `GET /item/<id>`, and all eleven were full-perm to their owner — so it is
/// not the "no asset id unless you fully own it" rule, it is the class.
/// **OpenSim** is the opposite: every object item names an asset, and
/// `ViewerAsset` serves it as `SceneObjectSerializer` XML.
///
/// Both are real grid behaviour, so the fake grid does both and says which. The
/// **default is [`Withheld`](Self::Withheld)**, because the grid a fake grid
/// imitates by default is Second Life, and because it is the configuration that
/// *fails* a viewer which has come to rely on opening a taken object's asset —
/// something Second Life will never let it do.
///
/// Either way the **rez** works: a taken object comes back into the world in
/// both configurations, because on Second Life too the simulator resolves the
/// body itself and the viewer never needs to see it. That is the whole shape of
/// the divergence — what a viewer may *fetch*, not what a resident may *do*.
///
/// What this does **not** govern is the seeded `Fixture Object`
/// ([`sl_test_assets::inventory`]), which keeps its asset id and stays
/// fetchable in both configurations: it is the fake grid's own fixture, seeded
/// so the `asset-round-trip` case has an authored object body to read back, and
/// no live grid has it at all. The switch is about what a **take** files away,
/// which is the only object item both live grids agree exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ObjectAssetPolicy {
    /// Second Life: a take files an item with a **nil** asset id, and the
    /// object's body — the Linden text — goes where no capability can reach it.
    #[default]
    Withheld,
    /// OpenSim: a take mints an asset id, names it in the item, and the grid
    /// serves the body under it like any other asset — as the
    /// `<SceneObjectGroup>` XML OpenSim itself writes for this class, not as
    /// the text the other side files.
    Served,
}

/// The one asset store a running grid serves, shared by every session, and
/// beside it the withheld object bodies no capability reads and the linksets a
/// take removed (see the module docs).
///
/// Cheap to clone; all clones are the same three stores.
#[derive(Clone, Debug, Default)]
pub(crate) struct GridAssets {
    /// The shared store behind its lock.
    inner: Arc<RwLock<InMemoryAssetSource>>,
    /// The object bodies a take wrote on a [`ObjectAssetPolicy::Withheld`]
    /// grid, keyed by the inventory item that stands for them — which is all a
    /// viewer is given, and all the rez path needs.
    objects: Arc<RwLock<HashMap<InventoryKey, Vec<u8>>>>,
    /// The **linksets** a take removed from the world, root first, keyed by the
    /// item they were filed as — what a rez puts back, because the published
    /// body cannot say all of it (see the module docs).
    ///
    /// Keyed by item under **both** policies, unlike the body: the rez path
    /// always has the item in hand, and one key means the two halves of a take
    /// cannot end up filed under different ones.
    taken: Arc<RwLock<HashMap<InventoryKey, Vec<Object>>>>,
}

impl GridAssets {
    /// Read access, recovering from a poisoned lock (see the module docs).
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, InMemoryAssetSource> {
        self.inner.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Write access, recovering from a poisoned lock.
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, InMemoryAssetSource> {
        self.inner.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Folds `assets` into the store, last write winning per key — how a
    /// region's fixture contributes what its own content references.
    pub(crate) fn extend(&self, assets: &InMemoryAssetSource) {
        let mut store = self.write();
        for (key, bytes) in assets.iter() {
            let _previous = store.insert(key, bytes.to_vec());
        }
    }

    /// Files an object body under the item that names it, out of reach of every
    /// capability — what a take writes on a [`ObjectAssetPolicy::Withheld`]
    /// grid.
    pub(crate) fn insert_withheld_object(&self, item: InventoryKey, body: Vec<u8>) {
        let _previous = self
            .objects
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(item, body);
    }

    /// The withheld body filed under `item`, if there is one.
    pub(crate) fn withheld_object(&self, item: InventoryKey) -> Option<Vec<u8>> {
        self.objects
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&item)
            .cloned()
    }

    /// Records the linkset a take removed from the world under the item it was
    /// filed as — root first, children after, the order a rez wants them in.
    pub(crate) fn insert_taken_linkset(&self, item: InventoryKey, linkset: Vec<Object>) {
        let _previous = self
            .taken
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(item, linkset);
    }

    /// The linkset filed under `item`, if this grid is the one that took it.
    ///
    /// [`None`] for an item no take made — a fixture's seeded object item —
    /// which is what sends the rez to the published body instead.
    pub(crate) fn taken_linkset(&self, item: InventoryKey) -> Option<Vec<Object>> {
        self.taken
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&item)
            .cloned()
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{AssetKey, AssetSource as _};

    use super::*;

    /// The default is the grid this workspace targets: Second Life tells a
    /// viewer nothing about where an object item's asset lives. Stated as "the
    /// default *is* what the default grid does" rather than as a literal, so a
    /// flavour that changed its mind could not leave the two disagreeing.
    #[test]
    fn the_default_policy_is_the_default_grids() {
        assert_eq!(
            ObjectAssetPolicy::default(),
            crate::ImitatedGrid::default().object_assets()
        );
        assert_eq!(ObjectAssetPolicy::default(), ObjectAssetPolicy::Withheld);
    }

    /// Two regions' fixtures fold into one store, and every clone of the handle
    /// sees both — which is the whole point: a viewer rooted in one region
    /// fetches the other's texture ids over its own region's capability.
    #[test]
    fn every_region_contributes_to_one_store() {
        let mine = AssetKey::from(uuid::Uuid::from_u128(1));
        let theirs = AssetKey::from(uuid::Uuid::from_u128(2));
        let assets = GridAssets::default();
        assets.extend(&InMemoryAssetSource::new().with_asset(mine, vec![1]));
        let other_session = assets.clone();
        assets.extend(&InMemoryAssetSource::new().with_asset(theirs, vec![2]));
        assert_eq!(other_session.read().get(mine), Some([1].as_slice()));
        assert_eq!(other_session.read().get(theirs), Some([2].as_slice()));
    }

    /// A later fold replaces an earlier one's bytes for the same id, so a
    /// fixture that deliberately re-uses a stock id wins over the stock one.
    #[test]
    fn a_later_fold_wins_the_key() {
        let key = AssetKey::from(uuid::Uuid::from_u128(3));
        let assets = GridAssets::default();
        assets.extend(&InMemoryAssetSource::new().with_asset(key, vec![1]));
        assets.extend(&InMemoryAssetSource::new().with_asset(key, vec![9]));
        assert_eq!(assets.read().get(key), Some([9].as_slice()));
    }

    /// A withheld object body is reachable by the item that names it and by
    /// nothing else — it is in the other store, so no asset id resolves to it
    /// and no capability can serve it however the id was arrived at.
    #[test]
    fn a_withheld_object_is_in_neither_the_served_store_nor_another_item() {
        let item = InventoryKey::from(uuid::Uuid::from_u128(4));
        let other = InventoryKey::from(uuid::Uuid::from_u128(5));
        let assets = GridAssets::default();
        assets.insert_withheld_object(item, vec![7]);
        assert_eq!(assets.withheld_object(item), Some(vec![7]));
        assert_eq!(assets.withheld_object(other), None);
        // The item's own id read as an asset id resolves to nothing: the two
        // stores share no keyspace, which is what makes the body unfetchable.
        assert_eq!(assets.read().get(AssetKey::from(item.uuid())), None);
        assert_eq!(assets.read().len(), 0);
    }
}
