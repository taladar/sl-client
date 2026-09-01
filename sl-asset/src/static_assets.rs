//! Assets that **ship with the viewer** rather than being fetched from a grid.
//!
//! A handful of asset UUIDs are fixed forever: the built-in agent animations a
//! viewer plays for `stand` / `walk` / the emotes, the library body parts and
//! clothing a default avatar is assembled from, and the library gestures. A
//! live grid serves them out of its own library, so a viewer *can* fetch them —
//! but it does not have to, and a viewer that ships them keeps working against
//! a grid whose library is incomplete (OpenSim), against a test grid that has
//! none (`sl-fake-grid`), and before the region's `ViewerAsset` capability has
//! even arrived.
//!
//! The reference viewer does exactly this. Firestorm keeps the files as
//! `<uuid>.<class>` under `app_settings/static_assets` and
//! `app_settings/fs_static_assets`, and `LLDiskCache::prepopulateCacheWithStatic`
//! copies them into the asset cache at start-up and puts their UUIDs on a skip
//! list so a cache purge can never evict them. The effect is that any later
//! fetch of one of those ids is answered locally and never reaches the network.
//!
//! This module is the same idea without the copying: a [`StaticAssetLibrary`]
//! indexes the directories once (one `read_dir` each, no file is read) and
//! [`AssetStore`](crate::AssetStore) consults it ahead of both its disk cache
//! and its fetcher. One [`install`] at start-up therefore serves every consumer
//! — animations, wearables, gestures — with no consumer needing to know the
//! library exists.
//!
//! The asset *class* is not part of the key. It is recoverable from the file
//! extension, but the store is keyed by UUID alone (mirroring OpenSim's
//! `IAssetService.Get(uuid)` and the reference's own cache, which stores static
//! assets under `AT_UNKNOWN`), so the extension is only ever a hint about what
//! the bytes are.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use uuid::Uuid;

use crate::AssetKey;

/// A read-only index of viewer-shipped assets, by UUID.
///
/// Built by [`load`](Self::load) from one or more directories of
/// `<uuid>.<class>` files. Indexing reads no file contents — the library holds
/// paths, and [`read`](Self::read) opens one on demand — so a library of a few
/// hundred assets costs a directory listing at start-up and nothing else until
/// something is actually wanted.
#[derive(Debug, Default, Clone)]
pub struct StaticAssetLibrary {
    /// Where each asset's bytes live, by the UUID its file is named for.
    files: HashMap<Uuid, PathBuf>,
}

impl StaticAssetLibrary {
    /// An empty library, which answers nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Indexes every `<uuid>.<class>` file in each of `dirs`, in order.
    ///
    /// A directory that does not exist (or cannot be listed) contributes
    /// nothing and is logged at debug — shipping the assets is optional, and a
    /// viewer without them simply fetches from the grid as before. A file whose
    /// stem is not a UUID is skipped for the same reason: the directories are
    /// vendored upstream content and a stray `README` in one is not an error.
    ///
    /// Later directories win, so a caller can list a general library first and
    /// an overriding one after it.
    #[must_use]
    pub fn load<I, P>(dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut files = HashMap::new();
        for dir in dirs {
            let dir = dir.as_ref();
            let entries = match fs_err::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) => {
                    tracing::debug!("no static assets in {}: {error}", dir.display());
                    continue;
                }
            };
            let mut found = 0_usize;
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(id) = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .and_then(|stem| Uuid::parse_str(stem).ok())
                else {
                    continue;
                };
                let _previous = files.insert(id, path);
                found = found.saturating_add(1);
            }
            tracing::debug!("indexed {found} static asset(s) in {}", dir.display());
        }
        Self { files }
    }

    /// Whether the library holds an asset for `id`.
    #[must_use]
    pub fn contains(&self, id: AssetKey) -> bool {
        self.files.contains_key(&id.uuid())
    }

    /// The bytes of the asset for `id`, or `None` when the library has none —
    /// or holds a path it can no longer read, which is logged and treated as a
    /// miss so the caller falls back to the grid.
    #[must_use]
    pub fn read(&self, id: AssetKey) -> Option<Bytes> {
        let path = self.files.get(&id.uuid())?;
        match fs_err::read(path) {
            Ok(bytes) => Some(Bytes::from(bytes)),
            Err(error) => {
                tracing::warn!("reading static asset {}: {error}", path.display());
                None
            }
        }
    }

    /// How many assets the library holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the library holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// The process-wide library, installed once at start-up.
static INSTALLED: OnceLock<Arc<StaticAssetLibrary>> = OnceLock::new();

/// Installs `library` as the process-wide static-asset library that every
/// [`AssetStore`](crate::AssetStore) built afterwards consults.
///
/// Process-wide because that is what the thing being modelled is: assets that
/// ship with the binary, the same for every store in the process, and reached
/// by consumers (the animation resolver, the wearable fetcher, …) that are
/// constructed lazily from a Bevy `World` and have no start-up options to be
/// handed a library through. It mirrors the reference viewer, whose equivalent
/// is a global disk cache seeded once before anything asks for an asset.
///
/// Returns `false` if a library was already installed, in which case `library`
/// is dropped and the existing one stays — the first install wins, so a
/// late second call cannot change what a store already snapshotted.
pub fn install(library: StaticAssetLibrary) -> bool {
    INSTALLED.set(Arc::new(library)).is_ok()
}

/// The installed process-wide library, if [`install`] has been called.
#[must_use]
pub fn installed() -> Option<Arc<StaticAssetLibrary>> {
    INSTALLED.get().map(Arc::clone)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{AssetKey, StaticAssetLibrary, Uuid};

    type TestError = Box<dyn core::error::Error>;

    /// A key from a small integer, for readable fixtures.
    fn key(n: u128) -> AssetKey {
        AssetKey::from(Uuid::from_u128(n))
    }

    /// A fresh, empty scratch directory named for `test` and this process, in
    /// the workspace's no-extra-dependency style (see `disk.rs`).
    fn scratch(test: &str) -> Result<std::path::PathBuf, TestError> {
        let dir =
            std::env::temp_dir().join(format!("sl-asset-static-{test}-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        fs_err::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Files named for a UUID are indexed whatever their class extension is,
    /// and read back byte for byte.
    #[test]
    fn a_uuid_named_file_is_indexed_and_read() -> Result<(), TestError> {
        let dir = scratch("indexed")?;
        fs_err::write(
            dir.join(format!("{}.animatn", Uuid::from_u128(1))),
            b"an-animation",
        )?;
        fs_err::write(
            dir.join(format!("{}.bodypart", Uuid::from_u128(2))),
            b"a-body-part",
        )?;
        let library = StaticAssetLibrary::load([&dir]);
        assert_eq!(library.len(), 2);
        assert!(!library.is_empty());
        assert!(library.contains(key(1)));
        assert_eq!(
            library.read(key(1)).as_deref(),
            Some(b"an-animation".as_slice())
        );
        assert_eq!(
            library.read(key(2)).as_deref(),
            Some(b"a-body-part".as_slice())
        );
        // An id the library does not hold is a miss, not an error.
        assert!(!library.contains(key(3)));
        assert_eq!(library.read(key(3)), None);
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    /// A file whose stem is not a UUID is skipped rather than failing the load:
    /// the directories are vendored upstream content and a stray note in one
    /// must not cost the viewer its whole library.
    #[test]
    fn a_non_uuid_file_is_skipped() -> Result<(), TestError> {
        let dir = scratch("skipped")?;
        fs_err::write(dir.join("README.md"), b"not an asset")?;
        fs_err::write(dir.join("not-a-uuid.animatn"), b"nor this")?;
        fs_err::write(
            dir.join(format!("{}.gesture", Uuid::from_u128(7))),
            b"a-gesture",
        )?;
        let library = StaticAssetLibrary::load([&dir]);
        assert_eq!(library.len(), 1);
        assert!(library.contains(key(7)));
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    /// A missing directory contributes nothing, and a library over only missing
    /// directories is simply empty — shipping the assets is optional.
    #[test]
    fn a_missing_directory_is_not_an_error() {
        let library = StaticAssetLibrary::load(["/nonexistent/static/assets"]);
        assert!(library.is_empty());
        assert_eq!(library.read(key(1)), None);
        assert!(StaticAssetLibrary::empty().is_empty());
    }

    /// Later directories win, so a caller can layer an overriding library over
    /// a general one.
    #[test]
    fn a_later_directory_overrides_an_earlier_one() -> Result<(), TestError> {
        let first = scratch("layered-general")?;
        let second = scratch("layered-override")?;
        let name = format!("{}.animatn", Uuid::from_u128(9));
        fs_err::write(first.join(&name), b"general")?;
        fs_err::write(second.join(&name), b"override")?;
        let library = StaticAssetLibrary::load([&first, &second]);
        assert_eq!(library.len(), 1);
        assert_eq!(
            library.read(key(9)).as_deref(),
            Some(b"override".as_slice())
        );
        let _removed = fs_err::remove_dir_all(&first);
        let _removed = fs_err::remove_dir_all(&second);
        Ok(())
    }
}
