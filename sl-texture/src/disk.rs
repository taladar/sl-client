//! A Second Life / Firestorm viewer-compatible on-disk texture cache.
//!
//! The cache directory reproduces the viewer's `LLTextureCache` layout so a
//! viewer could read (and, with care, share) it:
//!
//! - `texture.entries` — a `CacheHeader` (44 bytes) followed by one
//!   `CacheEntry` (28 bytes) per slot.
//! - `texture.cache` — the first 600 bytes of each texture's J2C
//!   codestream, the head of slot `n` at byte offset `n * 600`.
//! - `<hex>/<uuid>.texture` — the remaining codestream bytes (past the head) of
//!   each texture, in a subdirectory named by the first character of the id.
//!
//! All multi-byte fields are little-endian (matching the viewer on the common
//! platforms), assembled with explicit shifts because the crate lints forbid the
//! `to_le_bytes` / `from_le_bytes` family.
//!
//! **Covered-length semantics.** Unlike the viewer, which records the full asset
//! size, this cache stores in each entry's image-size field the length of the
//! codestream *prefix* it holds (head + body). The store decides whether that
//! prefix is enough for a requested level of detail with the same byte-size
//! estimate it uses for network fetches, so no separate "complete" flag is
//! needed. A viewer reading the cache sees a slightly small `imageSize` for
//! partially fetched textures, which it tolerates.

use std::collections::HashMap;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use parking_lot::Mutex;
use sl_proto::Uuid;

/// The `texture.entries` index file name.
const ENTRIES_FILE: &str = "texture.entries";
/// The `texture.cache` shared-head file name.
const CACHE_FILE: &str = "texture.cache";
/// Bytes of each texture's codestream stored inline in `texture.cache` (the
/// viewer's `TEXTURE_CACHE_ENTRY_SIZE` / `FIRST_PACKET_SIZE`).
const HEAD_SIZE: usize = 600;
/// Serialized size of a [`CacheHeader`].
const HEADER_SIZE: usize = 44;
/// Serialized size of a [`CacheEntry`].
const ENTRY_SIZE: usize = 28;
/// Length of the fixed encoder-version field in [`CacheHeader`].
const ENCODER_LEN: usize = 32;
/// The viewer cache format version stored in the header (`sHeaderCacheVersion`).
const CACHE_VERSION: f32 = 1.71;
/// The pointer width the header records (`sHeaderCacheAddressSize`).
const ADDRESS_SIZE: u32 = 32;
/// Our encoder-version identifier, truncated/padded to [`ENCODER_LEN`] bytes.
const ENCODER_VERSION: &str = "sl-texture OpenJPEG";
/// The fraction of the byte budget to purge down to when over a ceiling, leaving
/// headroom so the cache does not purge every session (viewer
/// `1 - TEXTURE_CACHE_PURGE_AMOUNT`).
const PURGE_TARGET_NUMERATOR: u64 = 80;
/// Denominator for [`PURGE_TARGET_NUMERATOR`].
const PURGE_TARGET_DENOMINATOR: u64 = 100;

/// Size ceilings for the cache; when either is exceeded, the least-recently
/// written textures are purged down to 80% of the byte budget.
#[derive(Clone, Copy, Debug)]
pub struct CacheLimits {
    /// Maximum number of live textures.
    pub max_entries: usize,
    /// Maximum total cached codestream bytes.
    pub max_bytes: u64,
}

impl Default for CacheLimits {
    fn default() -> Self {
        Self {
            // The viewer's `sCacheMaxEntries` default (~1M textures).
            max_entries: 1024 * 1024,
            // A conservative 2 GiB default byte budget.
            max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// The 44-byte header at the start of `texture.entries`.
#[derive(Clone, Copy, Debug)]
struct CacheHeader {
    /// The format version bits (`CACHE_VERSION` as raw `f32` bits).
    version_bits: u32,
    /// The recorded pointer width.
    address_size: u32,
    /// The encoder-version identifier bytes.
    encoder: [u8; ENCODER_LEN],
    /// The number of slots that follow (live or free).
    entries: u32,
}

impl CacheHeader {
    /// A fresh header for an empty cache written by this crate.
    fn fresh() -> Self {
        let mut encoder = [0_u8; ENCODER_LEN];
        for (slot, byte) in encoder.iter_mut().zip(ENCODER_VERSION.bytes()) {
            *slot = byte;
        }
        Self {
            version_bits: CACHE_VERSION.to_bits(),
            address_size: ADDRESS_SIZE,
            encoder,
            entries: 0,
        }
    }

    /// Appends the 44-byte little-endian serialization to `out`.
    fn encode(&self, out: &mut Vec<u8>) {
        write_u32_le(out, self.version_bits);
        write_u32_le(out, self.address_size);
        out.extend_from_slice(&self.encoder);
        write_u32_le(out, self.entries);
    }

    /// Parses a header from the start of `data`, or `None` if it is too short or
    /// its version does not match [`CACHE_VERSION`].
    fn decode(data: &[u8]) -> Option<Self> {
        let version_bits = read_u32_le(data, 0)?;
        if version_bits != CACHE_VERSION.to_bits() {
            return None;
        }
        let address_size = read_u32_le(data, 4)?;
        let encoder_slice = data.get(8..8_usize.checked_add(ENCODER_LEN)?)?;
        let encoder = <[u8; ENCODER_LEN]>::try_from(encoder_slice).ok()?;
        let entries = read_u32_le(data, 40)?;
        Some(Self {
            version_bits,
            address_size,
            encoder,
            entries,
        })
    }
}

/// A 28-byte per-texture record in `texture.entries`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CacheEntry {
    /// The texture id, or [`Uuid::nil`] for a free slot.
    id: Uuid,
    /// The length of the cached codestream prefix (head + body).
    image_size: i32,
    /// The number of body bytes (codestream past the head), in the body file.
    body_size: i32,
    /// The last-write time (unix seconds), used as the LRU key.
    time: u32,
}

impl CacheEntry {
    /// A free (unused) slot.
    const fn empty() -> Self {
        Self {
            id: Uuid::nil(),
            image_size: 0,
            body_size: 0,
            time: 0,
        }
    }

    /// Whether this slot is free.
    const fn is_free(&self) -> bool {
        self.id.is_nil()
    }

    /// Appends the 28-byte little-endian serialization to `out`.
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.id.as_bytes());
        write_u32_le(out, self.image_size.cast_unsigned());
        write_u32_le(out, self.body_size.cast_unsigned());
        write_u32_le(out, self.time);
    }

    /// Parses a 28-byte record at `offset` in `data`, or `None` if out of range.
    fn decode(data: &[u8], offset: usize) -> Option<Self> {
        let id_slice = data.get(offset..offset.checked_add(16)?)?;
        let id = Uuid::from_bytes(<[u8; 16]>::try_from(id_slice).ok()?);
        let image_size = read_u32_le(data, offset.checked_add(16)?)?.cast_signed();
        let body_size = read_u32_le(data, offset.checked_add(20)?)?.cast_signed();
        let time = read_u32_le(data, offset.checked_add(24)?)?;
        Some(Self {
            id,
            image_size,
            body_size,
            time,
        })
    }
}

/// The in-memory index mirroring `texture.entries`, guarded by a mutex.
#[derive(Debug, Default)]
struct Index {
    /// Slot records, indexed by slot number (matches `texture.cache` layout).
    slots: Vec<CacheEntry>,
    /// Map from live texture id to its slot.
    by_id: HashMap<Uuid, usize>,
    /// Free slot indices available for reuse.
    free: Vec<usize>,
    /// Running sum of live slots' `image_size` (total cached codestream bytes).
    total: u64,
}

/// A viewer-compatible on-disk texture cache in a dedicated directory.
#[derive(Debug)]
pub struct TextureDiskCache {
    /// The cache directory holding `texture.entries`, `texture.cache`, and body
    /// subdirectories.
    dir: PathBuf,
    /// The in-memory index.
    index: Mutex<Index>,
    /// The configured size ceilings.
    limits: CacheLimits,
    /// A monotonic counter making each atomic-write temp file name unique, so
    /// concurrent writes (many textures decoding at once) never share — and race
    /// on — the same `.tmp` path.
    tmp_seq: AtomicU64,
}

impl TextureDiskCache {
    /// Opens (or creates) a cache in `dir`, loading any existing
    /// `texture.entries` index. A malformed or version-mismatched index is
    /// treated as empty (cache-tolerant).
    ///
    /// # Errors
    ///
    /// Returns an error if `dir` cannot be created.
    pub fn open(dir: PathBuf, limits: CacheLimits) -> std::io::Result<Self> {
        fs_err::create_dir_all(&dir)?;
        let index = load_index(&dir).unwrap_or_default();
        Ok(Self {
            dir,
            index: Mutex::new(index),
            limits,
            tmp_seq: AtomicU64::new(0),
        })
    }

    /// A unique temp path for an atomic write of `path`: `<path>.<pid>.<seq>.tmp`.
    /// The per-cache sequence number keeps concurrent writers from colliding on
    /// (and racing to rename) a shared temp file.
    fn tmp_path(&self, path: &std::path::Path) -> PathBuf {
        let seq = self.tmp_seq.fetch_add(1, Ordering::Relaxed);
        let mut name = path.to_path_buf().into_os_string();
        name.push(format!(".{}.{seq}.tmp", std::process::id()));
        PathBuf::from(name)
    }

    /// Returns the cached codestream prefix for `id`, or `None` on a miss or any
    /// read error (treated as a miss, so a corrupt entry just re-fetches).
    #[must_use]
    pub fn read(&self, id: Uuid) -> Option<Bytes> {
        let (slot, entry) = self.lookup(id)?;
        let image_size = usize::try_from(entry.image_size).ok()?;
        let body_size = usize::try_from(entry.body_size).ok()?;
        let head_len = image_size.min(HEAD_SIZE);
        let mut bytes = self.read_head(slot, head_len).ok()?;
        if body_size > 0 {
            let body = fs_err::read(self.body_path(id)).ok()?;
            bytes.extend_from_slice(&body);
        }
        Some(Bytes::from(bytes))
    }

    /// Stores `codestream` (a full or LOD-prefix J2C codestream) for `id`,
    /// stamping it with `now_unix` for LRU, then purges if over a ceiling.
    ///
    /// # Errors
    ///
    /// Returns an error if writing the head, body, or index files fails.
    pub fn write(&self, id: Uuid, codestream: &[u8], now_unix: u32) -> std::io::Result<()> {
        let image_size = i32::try_from(codestream.len()).unwrap_or(i32::MAX);
        let head_len = codestream.len().min(HEAD_SIZE);
        let head = codestream.get(..head_len).unwrap_or(codestream);
        let body = codestream.get(head_len..).unwrap_or(&[]);
        let body_size = i32::try_from(body.len()).unwrap_or(i32::MAX);

        let entry = CacheEntry {
            id,
            image_size,
            body_size,
            time: now_unix,
        };
        let slot = self.stage_write(id, entry);

        self.write_head(slot, head)?;
        self.write_body(id, body)?;
        self.persist_index()?;
        self.purge()?;
        Ok(())
    }

    /// Looks up `id`'s slot and entry under the index lock (confined here so the
    /// guard is released before any file I/O).
    fn lookup(&self, id: Uuid) -> Option<(usize, CacheEntry)> {
        let index = self.index.lock();
        let slot = *index.by_id.get(&id)?;
        Some((slot, *index.slots.get(slot)?))
    }

    /// Records `entry` for `id` in the index (allocating or reusing a slot) and
    /// returns the slot, all under the index lock.
    fn stage_write(&self, id: Uuid, entry: CacheEntry) -> usize {
        let mut index = self.index.lock();
        let slot = allocate_slot(&mut index, id);
        replace_slot(&mut index, slot, entry);
        drop(index);
        slot
    }

    /// Reads `head_len` bytes of `slot`'s head from `texture.cache`.
    fn read_head(&self, slot: usize, head_len: usize) -> std::io::Result<Vec<u8>> {
        let mut file = fs_err::File::open(self.dir.join(CACHE_FILE))?;
        let _position = file.seek(SeekFrom::Start(head_offset(slot)))?;
        let mut buf = vec![0_u8; head_len];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Writes `head` into `slot`'s fixed record in `texture.cache`, in place.
    fn write_head(&self, slot: usize, head: &[u8]) -> std::io::Result<()> {
        let mut file = fs_err::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(CACHE_FILE))?;
        let _position = file.seek(SeekFrom::Start(head_offset(slot)))?;
        file.write_all(head)?;
        Ok(())
    }

    /// Writes `body` to `id`'s body file (via a temp file + rename), or removes
    /// any existing body file when `body` is empty.
    fn write_body(&self, id: Uuid, body: &[u8]) -> std::io::Result<()> {
        let path = self.body_path(id);
        if body.is_empty() {
            let _removed = fs_err::remove_file(&path);
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let tmp = self.tmp_path(&path);
        {
            let mut file = fs_err::File::create(&tmp)?;
            file.write_all(body)?;
            file.sync_all()?;
        }
        fs_err::rename(&tmp, &path)?;
        Ok(())
    }

    /// The body file path for `id`: `<dir>/<first-char>/<uuid>.texture`.
    fn body_path(&self, id: Uuid) -> PathBuf {
        let name = format!("{id}");
        let sub = name.get(..1).unwrap_or("0").to_owned();
        self.dir.join(sub).join(format!("{name}.texture"))
    }

    /// Rewrites `texture.entries` from the in-memory index (atomic temp+rename).
    /// The index file is small, so a full rewrite per write is acceptable.
    fn persist_index(&self) -> std::io::Result<()> {
        let bytes = self.serialize_index();
        let path = self.dir.join(ENTRIES_FILE);
        let tmp = self.tmp_path(&path);
        {
            let mut file = fs_err::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs_err::rename(&tmp, &path)?;
        Ok(())
    }

    /// Serializes the index (header + all slot records) to a byte buffer under
    /// the index lock (confined here so the guard is released before file I/O).
    fn serialize_index(&self) -> Vec<u8> {
        let index = self.index.lock();
        let mut header = CacheHeader::fresh();
        header.entries = u32::try_from(index.slots.len()).unwrap_or(u32::MAX);
        let mut out = Vec::with_capacity(
            HEADER_SIZE.saturating_add(index.slots.len().saturating_mul(ENTRY_SIZE)),
        );
        header.encode(&mut out);
        for entry in &index.slots {
            entry.encode(&mut out);
        }
        drop(index);
        out
    }

    /// Purges least-recently-written textures when over the entry-count or
    /// byte-size ceiling, down to [`PURGE_TARGET_NUMERATOR`]% of the byte budget.
    fn purge(&self) -> std::io::Result<()> {
        let victims = self.take_victims();
        if victims.is_empty() {
            return Ok(());
        }
        for id in &victims {
            let _removed = fs_err::remove_file(self.body_path(*id));
        }
        self.persist_index()
    }

    /// Selects and evicts victims from the index under the lock, returning their
    /// ids (empty when under both ceilings). Confined here so the guard releases
    /// before the caller does file I/O.
    fn take_victims(&self) -> Vec<Uuid> {
        let mut index = self.index.lock();
        let over_bytes = index.total > self.limits.max_bytes;
        let over_entries = index.by_id.len() > self.limits.max_entries;
        if !over_bytes && !over_entries {
            return Vec::new();
        }
        let victims = self.select_victims(&mut index);
        drop(index);
        victims
    }

    /// Evicts oldest live slots until under the byte target and entry ceiling,
    /// returning the purged ids (whose body files the caller then removes).
    fn select_victims(&self, index: &mut Index) -> Vec<Uuid> {
        let target = self
            .limits
            .max_bytes
            .saturating_mul(PURGE_TARGET_NUMERATOR)
            .checked_div(PURGE_TARGET_DENOMINATOR)
            .unwrap_or(0);
        let mut order: Vec<(u32, usize)> = index
            .slots
            .iter()
            .enumerate()
            .filter(|(_slot, entry)| !entry.is_free())
            .map(|(slot, entry)| (entry.time, slot))
            .collect();
        order.sort_unstable();
        let mut victims = Vec::new();
        for (_time, slot) in order {
            let over = index.total > target || index.by_id.len() > self.limits.max_entries;
            if !over {
                break;
            }
            if let Some(entry) = index.slots.get(slot).copied() {
                index.total = index.total.saturating_sub(covered(&entry));
                let _removed = index.by_id.remove(&entry.id);
                if let Some(existing) = index.slots.get_mut(slot) {
                    *existing = CacheEntry::empty();
                }
                index.free.push(slot);
                victims.push(entry.id);
            }
        }
        victims
    }
}

/// Finds the slot to write `id` into: its current slot, else a free slot, else a
/// freshly appended one. Subtracts the vacated slot's bytes from the total.
fn allocate_slot(index: &mut Index, id: Uuid) -> usize {
    if let Some(&slot) = index.by_id.get(&id) {
        if let Some(existing) = index.slots.get(slot) {
            index.total = index.total.saturating_sub(covered(existing));
        }
        return slot;
    }
    if let Some(slot) = index.free.pop() {
        return slot;
    }
    let slot = index.slots.len();
    index.slots.push(CacheEntry::empty());
    slot
}

/// Writes `entry` into `slot`, updating the id map and running total.
fn replace_slot(index: &mut Index, slot: usize, entry: CacheEntry) {
    let _inserted = index.by_id.insert(entry.id, slot);
    index.total = index.total.saturating_add(covered(&entry));
    if let Some(existing) = index.slots.get_mut(slot) {
        *existing = entry;
    }
}

/// The cached codestream byte count of an entry, as a `u64` for accounting.
fn covered(entry: &CacheEntry) -> u64 {
    u64::try_from(entry.image_size).unwrap_or(0)
}

/// The byte offset of `slot`'s head record in `texture.cache`.
fn head_offset(slot: usize) -> u64 {
    u64::try_from(slot)
        .unwrap_or(0)
        .saturating_mul(u64::try_from(HEAD_SIZE).unwrap_or(0))
}

/// Loads and parses the `texture.entries` index in `dir`, or `None` if it is
/// absent, unreadable, or malformed.
fn load_index(dir: &std::path::Path) -> Option<Index> {
    let data = fs_err::read(dir.join(ENTRIES_FILE)).ok()?;
    let header = CacheHeader::decode(&data)?;
    let count = usize::try_from(header.entries).ok()?;
    let mut index = Index::default();
    for slot in 0..count {
        let offset = HEADER_SIZE.checked_add(slot.checked_mul(ENTRY_SIZE)?)?;
        let Some(entry) = CacheEntry::decode(&data, offset) else {
            break;
        };
        if entry.is_free() {
            index.free.push(slot);
        } else {
            let _inserted = index.by_id.insert(entry.id, slot);
            index.total = index.total.saturating_add(covered(&entry));
        }
        index.slots.push(entry);
    }
    Some(index)
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
    use super::{CacheEntry, CacheHeader, CacheLimits, ENTRY_SIZE, HEADER_SIZE, TextureDiskCache};
    use pretty_assertions::assert_eq;
    use sl_proto::Uuid;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn header_round_trips_and_is_44_bytes() -> Result<(), TestError> {
        let mut header = CacheHeader::fresh();
        header.entries = 7;
        let mut bytes = Vec::new();
        header.encode(&mut bytes);
        assert_eq!(bytes.len(), HEADER_SIZE);
        let parsed = CacheHeader::decode(&bytes).ok_or("decode header")?;
        assert_eq!(parsed.entries, 7);
        assert_eq!(parsed.version_bits, header.version_bits);
        Ok(())
    }

    #[test]
    fn entry_round_trips_and_is_28_bytes() -> Result<(), TestError> {
        let entry = CacheEntry {
            id: Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788),
            image_size: 40_000,
            body_size: 39_400,
            time: 1_700_000_000,
        };
        let mut bytes = Vec::new();
        entry.encode(&mut bytes);
        assert_eq!(bytes.len(), ENTRY_SIZE);
        let parsed = CacheEntry::decode(&bytes, 0).ok_or("decode entry")?;
        assert_eq!(parsed, entry);
        Ok(())
    }

    #[test]
    fn write_then_read_round_trips_codestream() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-texture-disk-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let cache = TextureDiskCache::open(dir.clone(), CacheLimits::default())?;
        let id = Uuid::from_u128(0xAA);
        // A codestream longer than one head record, so it splits head/body.
        let data: Vec<u8> = (0..2000_u32)
            .map(|n| u8::try_from(n & 0xFF).unwrap_or(0))
            .collect();
        cache.write(id, &data, 1_700_000_000)?;
        let got = cache.read(id).ok_or("cache hit")?;
        assert_eq!(got.as_ref(), data.as_slice());
        // A fresh cache opened on the same dir reloads the entry from disk.
        let reopened = TextureDiskCache::open(dir.clone(), CacheLimits::default())?;
        assert_eq!(
            reopened.read(id).ok_or("reload hit")?.as_ref(),
            data.as_slice()
        );
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn small_texture_fits_entirely_in_head() -> Result<(), TestError> {
        let dir =
            std::env::temp_dir().join(format!("sl-texture-disk-small-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let cache = TextureDiskCache::open(dir.clone(), CacheLimits::default())?;
        let id = Uuid::from_u128(0xBB);
        let data: Vec<u8> = vec![0xAB; 400];
        cache.write(id, &data, 1)?;
        assert_eq!(cache.read(id).ok_or("hit")?.as_ref(), data.as_slice());
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn purge_evicts_oldest_over_byte_budget() -> Result<(), TestError> {
        let dir =
            std::env::temp_dir().join(format!("sl-texture-disk-purge-{}", std::process::id()));
        let _removed = fs_err::remove_dir_all(&dir);
        let limits = CacheLimits {
            max_entries: 1024,
            max_bytes: 2000,
        };
        let cache = TextureDiskCache::open(dir.clone(), limits)?;
        let old = Uuid::from_u128(0x01);
        let new = Uuid::from_u128(0x02);
        cache.write(old, &vec![1_u8; 1500], 100)?;
        // Writing a second 1500-byte texture exceeds the 2000-byte budget, so the
        // older one is purged down toward 80% (1600 bytes).
        cache.write(new, &vec![2_u8; 1500], 200)?;
        assert!(cache.read(old).is_none(), "oldest should be purged");
        assert!(cache.read(new).is_some(), "newest should remain");
        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }
}
