//! A Firestorm-compatible on-disk asset cache in a dedicated directory.
//!
//! Each asset is cached as one `<hex>/<uuid>.asset` file (the subdirectory named
//! by the first character of the id, as the viewer's disk cache shards its
//! `sl_cache_<uuid>_0.asset` files). A generic asset is opaque and fetched
//! whole, so — unlike the mesh cache, which prepends a preamble to track fetched
//! blocks — the file holds the raw asset bytes and nothing else.
//!
//! The cache is purged least-recently-written-first when it exceeds its byte or
//! entry ceiling; the LRU order uses each file's modification time, rebuilt by a
//! directory scan when the cache is opened.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use parking_lot::Mutex;
use sl_proto::Uuid;

/// The `.asset` cache file extension.
const ASSET_EXTENSION: &str = "asset";
/// The fraction (numerator) of the byte budget to purge down to.
const PURGE_TARGET_NUMERATOR: u64 = 80;
/// The denominator for [`PURGE_TARGET_NUMERATOR`].
const PURGE_TARGET_DENOMINATOR: u64 = 100;

/// Size ceilings for the cache; when either is exceeded, the least-recently
/// written assets are purged down to 80% of the byte budget.
#[derive(Clone, Copy, Debug)]
pub struct CacheLimits {
    /// Maximum number of cached assets.
    pub max_entries: usize,
    /// Maximum total cached asset bytes.
    pub max_bytes: u64,
}

impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            max_entries: 256 * 1024,
            max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// One cached asset's accounting: its file byte length and last-write time.
#[derive(Clone, Copy, Debug)]
struct IndexEntry {
    /// The cache file's total byte length.
    bytes: u64,
    /// The last-write time (unix seconds), the LRU key.
    time: u32,
}

/// The in-memory index of cached assets, guarded by a mutex.
#[derive(Debug, Default)]
struct Index {
    /// Per-asset accounting, keyed by asset id.
    entries: HashMap<Uuid, IndexEntry>,
    /// Running sum of all entries' byte lengths.
    total: u64,
}

/// A Firestorm-compatible per-UUID on-disk asset cache in a dedicated directory.
#[derive(Debug)]
pub struct AssetDiskCache {
    /// The cache directory holding the `<hex>/<uuid>.asset` files.
    dir: PathBuf,
    /// The in-memory index (LRU accounting).
    index: Mutex<Index>,
    /// The configured size ceilings.
    limits: CacheLimits,
}

impl AssetDiskCache {
    /// Opens (or creates) a cache in `dir`, rebuilding the LRU index from any
    /// existing `.asset` files.
    ///
    /// # Errors
    ///
    /// Returns an error if `dir` cannot be created.
    pub fn open(dir: PathBuf, limits: CacheLimits) -> std::io::Result<Self> {
        fs_err::create_dir_all(&dir)?;
        let index = scan_index(&dir);
        Ok(Self {
            dir,
            index: Mutex::new(index),
            limits,
        })
    }

    /// Returns the cached bytes for `id`, or `None` on a miss or any read error
    /// (treated as a miss, so a corrupt file just re-fetches).
    #[must_use]
    pub fn read(&self, id: Uuid) -> Option<Bytes> {
        let bytes = fs_err::read(self.asset_path(id)).ok()?;
        Some(Bytes::from(bytes))
    }

    /// Stores `data` for `id`, stamped with `now_unix` for LRU, then purges if
    /// over a ceiling.
    ///
    /// # Errors
    ///
    /// Returns an error if writing the cache file fails.
    pub fn write(&self, id: Uuid, data: &[u8], now_unix: u32) -> std::io::Result<()> {
        let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
        self.write_file(id, data)?;
        self.record(id, len, now_unix);
        self.purge()?;
        Ok(())
    }

    /// The cache file path for `id`: `<dir>/<first-char>/<uuid>.asset`.
    fn asset_path(&self, id: Uuid) -> PathBuf {
        let name = format!("{id}");
        let sub = name.get(..1).unwrap_or("0").to_owned();
        self.dir.join(sub).join(format!("{name}.{ASSET_EXTENSION}"))
    }

    /// Writes `data` to `id`'s cache file (temp file + rename).
    fn write_file(&self, id: Uuid, data: &[u8]) -> std::io::Result<()> {
        let path = self.asset_path(id);
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let mut name = path.clone().into_os_string();
        name.push(format!(".{}.tmp", std::process::id()));
        let tmp = PathBuf::from(name);
        {
            let mut file = fs_err::File::create(&tmp)?;
            file.write_all(data)?;
            file.sync_all()?;
        }
        fs_err::rename(&tmp, &path)?;
        Ok(())
    }

    /// Records `id`'s new byte length and write time in the index under the lock.
    fn record(&self, id: Uuid, bytes: u64, now_unix: u32) {
        let mut index = self.index.lock();
        if let Some(previous) = index.entries.insert(
            id,
            IndexEntry {
                bytes,
                time: now_unix,
            },
        ) {
            index.total = index.total.saturating_sub(previous.bytes);
        }
        index.total = index.total.saturating_add(bytes);
        drop(index);
    }

    /// Purges least-recently-written assets when over a ceiling.
    fn purge(&self) -> std::io::Result<()> {
        for id in self.take_victims() {
            let _removed = fs_err::remove_file(self.asset_path(id));
        }
        Ok(())
    }

    /// Selects and evicts victims from the index under the lock, returning their
    /// ids (empty when under both ceilings).
    fn take_victims(&self) -> Vec<Uuid> {
        let mut index = self.index.lock();
        let over_bytes = index.total > self.limits.max_bytes;
        let over_entries = index.entries.len() > self.limits.max_entries;
        if !over_bytes && !over_entries {
            return Vec::new();
        }
        let target = self
            .limits
            .max_bytes
            .saturating_mul(PURGE_TARGET_NUMERATOR)
            .checked_div(PURGE_TARGET_DENOMINATOR)
            .unwrap_or(0);
        let mut order: Vec<(u32, Uuid)> = index
            .entries
            .iter()
            .map(|(id, entry)| (entry.time, *id))
            .collect();
        order.sort_unstable();
        let mut victims = Vec::new();
        for (_time, id) in order {
            let over = index.total > target || index.entries.len() > self.limits.max_entries;
            if !over {
                break;
            }
            if let Some(entry) = index.entries.remove(&id) {
                index.total = index.total.saturating_sub(entry.bytes);
                victims.push(id);
            }
        }
        drop(index);
        victims
    }
}

/// Rebuilds the LRU index by scanning the cache directory's `<hex>/<uuid>.asset`
/// files for their sizes and modification times.
fn scan_index(dir: &std::path::Path) -> Index {
    let mut index = Index::default();
    let Ok(subdirs) = fs_err::read_dir(dir) else {
        return index;
    };
    for subdir in subdirs.flatten() {
        let Ok(files) = fs_err::read_dir(subdir.path()) else {
            continue;
        };
        for file in files.flatten() {
            if let Some((id, entry)) = scan_file(&file.path()) {
                index.total = index.total.saturating_add(entry.bytes);
                let _previous = index.entries.insert(id, entry);
            }
        }
    }
    index
}

/// Reads one `.asset` file's id (from its name), byte length, and modification
/// time (unix seconds) for the LRU index, or `None` if it is not a cache file.
fn scan_file(path: &std::path::Path) -> Option<(Uuid, IndexEntry)> {
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some(ASSET_EXTENSION) {
        return None;
    }
    let stem = path.file_stem().and_then(std::ffi::OsStr::to_str)?;
    let id = Uuid::parse_str(stem).ok()?;
    let metadata = fs_err::metadata(path).ok()?;
    let bytes = metadata.len();
    let time = metadata
        .modified()
        .ok()
        .and_then(|when| when.duration_since(UNIX_EPOCH).ok())
        .and_then(|elapsed| u32::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0);
    Some((id, IndexEntry { bytes, time }))
}

/// The current time in whole unix seconds (0 on error), for LRU stamping.
#[must_use]
pub(crate) fn now_unix() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u32::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{AssetDiskCache, CacheLimits};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::Uuid;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn write_then_read_round_trips() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-asset-disk-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let cache = AssetDiskCache::open(dir.clone(), CacheLimits::default())?;
        let id = Uuid::from_u128(0xABCD);
        let data: Vec<u8> = (0..500_u32)
            .map(|n| u8::try_from(n & 0xFF).unwrap_or(0))
            .collect();
        cache.write(id, &data, 1_700_000_000)?;
        let got = cache.read(id).ok_or("cache hit")?;
        assert_eq!(got, Bytes::from(data));
        // A fresh cache opened on the same dir reloads the entry (index scan).
        let reopened = AssetDiskCache::open(dir.clone(), CacheLimits::default())?;
        assert!(reopened.read(id).is_some());
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn purge_evicts_oldest_over_byte_budget() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-asset-disk-purge-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let limits = CacheLimits {
            max_entries: 1024,
            max_bytes: 2000,
        };
        let cache = AssetDiskCache::open(dir.clone(), limits)?;
        let old = Uuid::from_u128(0x01);
        let new = Uuid::from_u128(0x02);
        cache.write(old, &vec![1_u8; 1500], 100)?;
        cache.write(new, &vec![2_u8; 1500], 200)?;
        assert!(cache.read(old).is_none(), "oldest should be purged");
        assert!(cache.read(new).is_some(), "newest should remain");
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }
}
