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

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use sl_proto::InMemoryAssetSource;

/// The one asset store a running grid serves, shared by every session.
///
/// Cheap to clone; all clones are the same store.
#[derive(Clone, Debug, Default)]
pub(crate) struct GridAssets {
    /// The shared store behind its lock.
    inner: Arc<RwLock<InMemoryAssetSource>>,
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
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{AssetKey, AssetSource as _};

    use super::*;

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
}
