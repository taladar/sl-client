//! A Firestorm-compatible on-disk mesh cache in a dedicated directory.
//!
//! Each mesh is cached as one `<hex>/<uuid>.mesh` file (the subdirectory named
//! by the first character of the id, as the viewer's texture cache does),
//! holding the viewer's 12-byte preamble followed by the cached asset region:
//!
//! - **preamble** (12 bytes, little-endian): `version` (`1`), `header_size`
//!   (bytes of the mesh header), and `flags` (a bitmask of which blocks are
//!   present in the file — the `FLAG_SKIN` / `flag_lod` bits).
//! - **data**: the asset bytes from offset 0, i.e. the header followed by each
//!   fetched block written at its absolute header offset (gaps zero-padded), so
//!   a block at header offset `o` sits at file offset `12 + header_size + o` and
//!   the store can slice it out with the same offset arithmetic it uses for the
//!   network.
//!
//! All multi-byte preamble fields are little-endian, assembled with explicit
//! shifts because the crate lints forbid the `to_le_bytes` / `from_le_bytes`
//! family. The cache is purged least-recently-written-first when it exceeds its
//! byte or entry ceiling; the LRU order uses each file's modification time,
//! rebuilt by a directory scan when the cache is opened.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use parking_lot::Mutex;
use sl_proto::Uuid;

/// The preamble format version this cache writes (`CACHE_PREAMBLE_VERSION`).
const PREAMBLE_VERSION: u32 = 1;
/// The 12-byte preamble length (`version`, `header_size`, `flags`).
const PREAMBLE_SIZE: usize = 12;
/// The `flags` bit for the skin block.
pub(crate) const FLAG_SKIN: u32 = 1 << 4_u32;
/// The `flags` bit for the convex physics block.
pub(crate) const FLAG_PHYSICS_CONVEX: u32 = 1 << 5_u32;
/// The `flags` bit for the triangle physics-mesh block.
pub(crate) const FLAG_PHYSICS_MESH: u32 = 1 << 6_u32;
/// The `.mesh` cache file extension.
const MESH_EXTENSION: &str = "mesh";
/// The fraction (numerator) of the byte budget to purge down to.
const PURGE_TARGET_NUMERATOR: u64 = 80;
/// The denominator for [`PURGE_TARGET_NUMERATOR`].
const PURGE_TARGET_DENOMINATOR: u64 = 100;

/// The `flags` bit for geometry LOD `index` (`0..4`).
pub(crate) const fn flag_lod(index: u8) -> u32 {
    match index {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 8,
        _other => 0,
    }
}

/// An in-memory mesh asset: the contiguous asset region (header + fetched blocks
/// at their absolute offsets, zero-padded), the header length, and a bitmask of
/// which blocks the region contains.
#[derive(Clone, Debug)]
pub struct AssetBytes {
    /// The contiguous asset bytes from offset 0.
    pub(crate) data: Bytes,
    /// The mesh header length in bytes.
    pub(crate) header_size: usize,
    /// The bitmask of blocks present in [`Self::data`].
    pub(crate) flags: u32,
}

impl AssetBytes {
    /// An asset with only the header region fetched (no blocks yet).
    pub(crate) const fn header_only(data: Bytes, header_size: usize) -> Self {
        Self {
            data,
            header_size,
            flags: 0,
        }
    }

    /// The block bytes at absolute range `[start, end)`, if the region covers it.
    pub(crate) fn slice(&self, start: usize, end: usize) -> Option<Bytes> {
        if end > self.data.len() || start > end {
            return None;
        }
        Some(self.data.slice(start..end))
    }

    /// Returns a copy of this asset with `block` written at absolute offset
    /// `start` (growing and zero-padding the region as needed) and `flag` set.
    pub(crate) fn with_block(&self, start: usize, block: &[u8], flag: u32) -> Self {
        let end = start.saturating_add(block.len());
        let mut grown = Vec::with_capacity(self.data.len().max(end));
        grown.extend_from_slice(&self.data);
        if grown.len() < end {
            grown.resize(end, 0);
        }
        if let Some(target) = grown.get_mut(start..end) {
            target.copy_from_slice(block);
        }
        Self {
            data: Bytes::from(grown),
            header_size: self.header_size,
            flags: self.flags | flag,
        }
    }
}

/// Size ceilings for the cache; when either is exceeded, the least-recently
/// written meshes are purged down to 80% of the byte budget.
#[derive(Clone, Copy, Debug)]
pub struct CacheLimits {
    /// Maximum number of cached meshes.
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

/// One cached mesh's accounting: its file byte length and last-write time.
#[derive(Clone, Copy, Debug)]
struct IndexEntry {
    /// The cache file's total byte length (preamble + data).
    bytes: u64,
    /// The last-write time (unix seconds), the LRU key.
    time: u32,
}

/// The in-memory index of cached meshes, guarded by a mutex.
#[derive(Debug, Default)]
struct Index {
    /// Per-mesh accounting, keyed by asset id.
    entries: HashMap<Uuid, IndexEntry>,
    /// Running sum of all entries' byte lengths.
    total: u64,
}

/// A Firestorm-compatible per-UUID on-disk mesh cache in a dedicated directory.
#[derive(Debug)]
pub struct MeshDiskCache {
    /// The cache directory holding the `<hex>/<uuid>.mesh` files.
    dir: PathBuf,
    /// The in-memory index (LRU accounting).
    index: Mutex<Index>,
    /// The configured size ceilings.
    limits: CacheLimits,
}

impl MeshDiskCache {
    /// Opens (or creates) a cache in `dir`, rebuilding the LRU index from any
    /// existing `.mesh` files.
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

    /// Returns the cached asset for `id`, or `None` on a miss or any read error
    /// (treated as a miss, so a corrupt file just re-fetches).
    #[must_use]
    pub fn read(&self, id: Uuid) -> Option<AssetBytes> {
        let bytes = fs_err::read(self.mesh_path(id)).ok()?;
        decode_file(&bytes)
    }

    /// Stores `asset` for `id`, stamped with `now_unix` for LRU, then purges if
    /// over a ceiling.
    ///
    /// # Errors
    ///
    /// Returns an error if writing the cache file fails.
    pub fn write(&self, id: Uuid, asset: &AssetBytes, now_unix: u32) -> std::io::Result<()> {
        let encoded = encode_file(asset);
        let len = u64::try_from(encoded.len()).unwrap_or(u64::MAX);
        self.write_file(id, &encoded)?;
        self.record(id, len, now_unix);
        self.purge()?;
        Ok(())
    }

    /// The cache file path for `id`: `<dir>/<first-char>/<uuid>.mesh`.
    fn mesh_path(&self, id: Uuid) -> PathBuf {
        let name = format!("{id}");
        let sub = name.get(..1).unwrap_or("0").to_owned();
        self.dir.join(sub).join(format!("{name}.{MESH_EXTENSION}"))
    }

    /// Writes `encoded` to `id`'s cache file (temp file + rename).
    fn write_file(&self, id: Uuid, encoded: &[u8]) -> std::io::Result<()> {
        let path = self.mesh_path(id);
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let mut name = path.clone().into_os_string();
        name.push(format!(".{}.tmp", std::process::id()));
        let tmp = PathBuf::from(name);
        {
            let mut file = fs_err::File::create(&tmp)?;
            file.write_all(encoded)?;
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

    /// Purges least-recently-written meshes when over a ceiling.
    fn purge(&self) -> std::io::Result<()> {
        for id in self.take_victims() {
            let _removed = fs_err::remove_file(self.mesh_path(id));
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

/// Serializes an [`AssetBytes`] to its on-disk form (12-byte preamble + data).
fn encode_file(asset: &AssetBytes) -> Vec<u8> {
    let header_size = u32::try_from(asset.header_size).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(PREAMBLE_SIZE.saturating_add(asset.data.len()));
    write_u32_le(&mut out, PREAMBLE_VERSION);
    write_u32_le(&mut out, header_size);
    write_u32_le(&mut out, asset.flags);
    out.extend_from_slice(&asset.data);
    out
}

/// Parses an on-disk cache file into an [`AssetBytes`], or `None` if the bytes
/// are too short or carry an unrecognized preamble version.
fn decode_file(bytes: &[u8]) -> Option<AssetBytes> {
    let version = read_u32_le(bytes, 0)?;
    if version != PREAMBLE_VERSION {
        return None;
    }
    let header_size = usize::try_from(read_u32_le(bytes, 4)?).ok()?;
    let flags = read_u32_le(bytes, 8)?;
    let data = bytes.get(PREAMBLE_SIZE..)?;
    Some(AssetBytes {
        data: Bytes::copy_from_slice(data),
        header_size,
        flags,
    })
}

/// Rebuilds the LRU index by scanning the cache directory's `<hex>/<uuid>.mesh`
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

/// Reads one `.mesh` file's id (from its name), byte length, and modification
/// time (unix seconds) for the LRU index, or `None` if it is not a cache file.
fn scan_file(path: &std::path::Path) -> Option<(Uuid, IndexEntry)> {
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some(MESH_EXTENSION) {
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

/// Appends `value` to `out` as four little-endian bytes.
fn write_u32_le(out: &mut Vec<u8>, value: u32) {
    out.push(byte_of(value, 0));
    out.push(byte_of(value, 8));
    out.push(byte_of(value, 16));
    out.push(byte_of(value, 24));
}

/// The byte of `value` at bit `shift` (0/8/16/24), for little-endian writing.
fn byte_of(value: u32, shift: u32) -> u8 {
    u8::try_from(value.wrapping_shr(shift) & 0xFF).unwrap_or(0)
}

/// Reads a little-endian `u32` at `offset` in `data`, or `None` if out of range.
fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let b0 = u32::from(*data.get(offset)?);
    let b1 = u32::from(*data.get(offset.checked_add(1)?)?);
    let b2 = u32::from(*data.get(offset.checked_add(2)?)?);
    let b3 = u32::from(*data.get(offset.checked_add(3)?)?);
    Some(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
}

#[cfg(test)]
mod tests {
    use super::{AssetBytes, CacheLimits, MeshDiskCache};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::Uuid;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn asset_with_block_pads_and_sets_flag() {
        let base = AssetBytes::header_only(Bytes::from(vec![1_u8, 2, 3, 4]), 4);
        // Write a 2-byte block at offset 6 (a 2-byte gap is zero-padded).
        let grown = base.with_block(6, &[9, 9], super::flag_lod(3));
        assert_eq!(grown.data.as_ref(), &[1, 2, 3, 4, 0, 0, 9, 9]);
        assert_eq!(grown.flags, super::flag_lod(3));
        assert_eq!(grown.slice(6, 8), Some(Bytes::from(vec![9_u8, 9])));
        assert_eq!(grown.slice(6, 99), None);
    }

    #[test]
    fn write_then_read_round_trips() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-mesh-disk-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let cache = MeshDiskCache::open(dir.clone(), CacheLimits::default())?;
        let id = Uuid::from_u128(0xABCD);
        let asset = AssetBytes {
            data: Bytes::from(
                (0..500_u32)
                    .map(|n| u8::try_from(n & 0xFF).unwrap_or(0))
                    .collect::<Vec<_>>(),
            ),
            header_size: 40,
            flags: super::FLAG_SKIN | super::flag_lod(3),
        };
        cache.write(id, &asset, 1_700_000_000)?;
        let got = cache.read(id).ok_or("cache hit")?;
        assert_eq!(got.data, asset.data);
        assert_eq!(got.header_size, 40);
        assert_eq!(got.flags, asset.flags);
        // A fresh cache opened on the same dir reloads the entry (index scan).
        let reopened = MeshDiskCache::open(dir.clone(), CacheLimits::default())?;
        assert!(reopened.read(id).is_some());
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn purge_evicts_oldest_over_byte_budget() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-mesh-disk-purge-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let limits = CacheLimits {
            max_entries: 1024,
            max_bytes: 2000,
        };
        let cache = MeshDiskCache::open(dir.clone(), limits)?;
        let old = Uuid::from_u128(0x01);
        let new = Uuid::from_u128(0x02);
        let blob = |byte: u8| AssetBytes {
            data: Bytes::from(vec![byte; 1500]),
            header_size: 10,
            flags: 0,
        };
        cache.write(old, &blob(1), 100)?;
        cache.write(new, &blob(2), 200)?;
        assert!(cache.read(old).is_none(), "oldest should be purged");
        assert!(cache.read(new).is_some(), "newest should remain");
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }
}
