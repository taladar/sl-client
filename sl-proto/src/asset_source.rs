//! The read-only byte source the asset-delivery caps
//! ([`AssetCaps`](crate::AssetCaps)) serve from.
//!
//! Deliberately *not* [`SimSession`](crate::SimSession): assets are grid-wide
//! content, not per-UDP-session state, so a content-delivery process that
//! serves nothing but `GetTexture`/`GetMesh`/`GetMesh2`/`ViewerAsset` holds
//! only an [`AssetSource`] and never a session. That is what lets the asset
//! surface live in a **different process** (a CDN) from the simulator.
//!
//! The source is keyed by [`AssetKey`] (the raw UUID) *alone*, mirroring
//! OpenSim's `IAssetService.Get(uuid)`: the same bytes are served regardless
//! of which capability requested them, and the asset *type* is request-side
//! metadata that only picks the response `Content-Type`. The name is
//! [`AssetSource`], not `AssetStore`, because `sl-asset` already exports a
//! client-side `AssetStore` and the two must not collide.

use std::collections::HashMap;

use crate::AssetKey;

/// A read-only source of raw asset bytes, keyed by UUID.
///
/// Object-safe (`&dyn AssetSource`) and `Send + Sync` so a future threaded
/// caps HTTP server — or a standalone CDN process — can share one behind an
/// `Arc`. [`get`](AssetSource::get) returns a borrow so the caps handler can
/// read the total length and slice a byte range without cloning the whole
/// asset; the only copy is the range slice into the owned response body.
pub trait AssetSource: Send + Sync {
    /// The full bytes of the asset named by `key`, or `None` if the asset
    /// genuinely does not exist (which the caps turn into a `404`).
    fn get(&self, key: AssetKey) -> Option<&[u8]>;
}

/// An in-memory [`AssetSource`] over a `HashMap`, for the fake grid and the
/// loopback tests.
///
/// This is the pure, sans-I/O fixture. A directory-backed source is an
/// eager loader in an I/O-capable crate
/// (`sl_client_tokio::load_asset_dir`) that reads a directory into one of
/// these at construction, keeping this serving path free of filesystem
/// access.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InMemoryAssetSource {
    /// The stored assets, keyed by their raw UUID.
    assets: HashMap<AssetKey, Vec<u8>>,
}

impl InMemoryAssetSource {
    /// An empty source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts (or replaces) the bytes for `key`, returning any previous
    /// bytes.
    pub fn insert(&mut self, key: AssetKey, data: impl Into<Vec<u8>>) -> Option<Vec<u8>> {
        self.assets.insert(key, data.into())
    }

    /// The builder form of [`insert`](Self::insert), for fixture setup:
    /// `InMemoryAssetSource::new().with_asset(a, bytes_a).with_asset(b, …)`.
    #[must_use]
    pub fn with_asset(mut self, key: AssetKey, data: impl Into<Vec<u8>>) -> Self {
        self.insert(key, data);
        self
    }

    /// Whether the source holds an asset for `key`.
    #[must_use]
    pub fn contains(&self, key: AssetKey) -> bool {
        self.assets.contains_key(&key)
    }

    /// The number of stored assets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether the source is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

impl AssetSource for InMemoryAssetSource {
    fn get(&self, key: AssetKey) -> Option<&[u8]> {
        self.assets.get(&key).map(Vec::as_slice)
    }
}

impl FromIterator<(AssetKey, Vec<u8>)> for InMemoryAssetSource {
    fn from_iter<I: IntoIterator<Item = (AssetKey, Vec<u8>)>>(iter: I) -> Self {
        Self {
            assets: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::*;

    /// A key from a small integer, for readable fixtures.
    fn key(n: u128) -> AssetKey {
        AssetKey::from(Uuid::from_u128(n))
    }

    /// `insert` stores bytes, `get` returns the borrow, a miss is `None`.
    #[test]
    fn insert_and_get_round_trip() {
        let mut source = InMemoryAssetSource::new();
        assert!(source.is_empty());
        assert_eq!(source.insert(key(1), b"hello".to_vec()), None);
        assert_eq!(source.get(key(1)), Some(b"hello".as_slice()));
        assert_eq!(source.get(key(2)), None);
        assert!(source.contains(key(1)));
        assert!(!source.contains(key(2)));
        assert_eq!(source.len(), 1);
    }

    /// `insert` replaces and returns the previous bytes.
    #[test]
    fn insert_replaces_and_returns_previous() {
        let mut source = InMemoryAssetSource::new();
        source.insert(key(1), b"first".to_vec());
        assert_eq!(
            source.insert(key(1), b"second".to_vec()),
            Some(b"first".to_vec())
        );
        assert_eq!(source.get(key(1)), Some(b"second".as_slice()));
    }

    /// The builder and `FromIterator` forms populate equivalently.
    #[test]
    fn builder_and_from_iter_agree() {
        let built = InMemoryAssetSource::new()
            .with_asset(key(1), b"a".to_vec())
            .with_asset(key(2), b"b".to_vec());
        let collected: InMemoryAssetSource = [(key(1), b"a".to_vec()), (key(2), b"b".to_vec())]
            .into_iter()
            .collect();
        assert_eq!(built, collected);
    }
}
