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

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use sl_proto::InMemoryAssetSource;
use sl_types::key::InventoryKey;

/// Which live grid the fake one imitates for `AssetType::Object` — the class
/// where the two disagree about whether a viewer may see an asset at all.
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
/// **default is [`Withheld`](Self::Withheld)**, because that is the grid this
/// workspace targets and because it is the configuration that *fails* a viewer
/// which has come to rely on opening a taken object's asset — something Second
/// Life will never let it do. A test that wants the OpenSim side asks for
/// [`Served`](Self::Served).
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
    /// object's body goes where no capability can reach it.
    #[default]
    Withheld,
    /// OpenSim: a take mints an asset id, names it in the item, and the grid
    /// serves the body under it like any other asset.
    Served,
}

/// The one asset store a running grid serves, shared by every session, and
/// beside it the withheld object bodies no capability reads (see the module
/// docs).
///
/// Cheap to clone; all clones are the same pair of stores.
#[derive(Clone, Debug, Default)]
pub(crate) struct GridAssets {
    /// The shared store behind its lock.
    inner: Arc<RwLock<InMemoryAssetSource>>,
    /// The object bodies a take wrote on a [`ObjectAssetPolicy::Withheld`]
    /// grid, keyed by the inventory item that stands for them — which is all a
    /// viewer is given, and all the rez path needs.
    objects: Arc<RwLock<HashMap<InventoryKey, Vec<u8>>>>,
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
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{AssetKey, AssetSource as _};

    use super::*;

    /// The default is the grid this workspace targets: Second Life tells a
    /// viewer nothing about where an object item's asset lives.
    #[test]
    fn the_default_policy_withholds() {
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
