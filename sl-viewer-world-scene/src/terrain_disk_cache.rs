//! The land-height cache on disk: the last-known land patches of each region,
//! kept between sessions so the avatar has a ground floor from the first frame
//! after login.
//!
//! [`TerrainState::land_cache`] retains every land patch the session has seen,
//! which keeps [`TerrainState::land_height`] answering through a region's
//! teardown / rebuild. But it starts empty, and the login window — before the
//! region's terrain has streamed — is exactly when the avatar's ground floor
//! (`physics.rs`'s `avatar_collision_floor`) has nothing but the
//! avatar-tracking collision plane and can let the avatar sink through the
//! ground. This module fills that window from disk:
//!
//! - **Load**: when a region first shows up — the agent's region as soon as
//!   the login response names it (a live grid's `RegionHandshake` can arrive
//!   before the session has learnt the circuit's handle, stamped with handle
//!   zero), a neighbour by its handshake, a handover's destination — its
//!   cached patches are read on the [`IoTaskPool`] and
//!   merged into the land cache — under any patch that already arrived live,
//!   never over it.
//! - **Save**: a region whose land changed (it streamed, or was terraformed) is
//!   written once its patches have been quiet for [`SAVE_QUIET_SECS`] (or after
//!   [`SAVE_MAX_DELAY_SECS`] of continuous change), on the [`IoTaskPool`], and
//!   every still-unsaved region is written synchronously at exit.
//! - **Bound**: at most [`DEFAULT_MAX_REGIONS`] region files are kept; each save
//!   drops the least recently written beyond that. A region visited again is
//!   re-streamed and re-saved, which keeps its file the newest.
//!
//! Land is effectively static — only terraforming changes it — so a stale
//! cached height is a far better floor than none, and the live patch replaces
//! it the moment it streams. There is no other invalidation: newest wins.
//!
//! The cache is **grid-scoped** (region handles are grid coordinates, and two
//! grids reuse the same ones), one file per region under
//! `<cache root>/terraincache/<grid>/`, in a small binary format of its own
//! ([`encode_region`] / [`decode_region`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use sl_client_bevy::{
    RegionHandle, SlEvent, SlIdentity, SlSessionEvent, TerrainLayerType, TerrainPatch,
};
use sl_viewer_world_api::{PatchKey, TerrainState};

/// The first bytes of every cache file.
const MAGIC: [u8; 4] = *b"SLTL";

/// The cache file format version; a file of another version is ignored (and
/// overwritten by the next save).
const FORMAT_VERSION: u32 = 1;

/// The extension of a region's cache file.
const FILE_EXTENSION: &str = "terrain";

/// The infix of a file being written, before it is renamed into place.
const TEMP_INFIX: &str = ".terrain.tmp-";

/// How old a leftover temporary file (from a write a crash interrupted) must be
/// before a prune removes it — old enough that no running write still owns it.
const STALE_TEMP_AGE: Duration = Duration::from_secs(600);

/// How many region files the cache keeps: the regions most recently written.
/// A standard region's land is 256 patches of 16×16 heights, about 260 KiB on
/// disk, so this caps the cache near 33 MiB.
pub const DEFAULT_MAX_REGIONS: usize = 128;

/// How long a region's land must have been unchanged before it is saved, so a
/// region streaming in is written once rather than once per patch.
pub const SAVE_QUIET_SECS: f64 = 5.0;

/// The longest a changed region waits for a save while its land keeps
/// changing (a long terraforming session).
pub const SAVE_MAX_DELAY_SECS: f64 = 30.0;

/// The largest patch edge a cache file may state. Real patches are 16 cells
/// (32 for a variable region's large patches); anything this big is corrupt.
const MAX_PATCH_SIZE: u32 = 256;

/// The most patches a cache file may state: a 4096 m variable region of
/// 16-cell patches.
const MAX_PATCHES_PER_FILE: u32 = 0x0001_0000;

/// Little-endian byte conversions, which the cache file format is defined in.
///
/// The workspace denies both explicit-endian conversions (a stray one in a
/// wire payload is a protocol bug), so the file format's are confined here
/// behind one localized expectation, as `panorama::xmp` does for its
/// big-endian container fields.
mod le {
    #![expect(
        clippy::little_endian_bytes,
        reason = "the terrain cache file is defined little-endian, so a cache written on one \
                  machine reads back the same on any other"
    )]

    /// A `u32`'s four file bytes.
    pub(super) const fn u32_to(value: u32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// A `u64`'s eight file bytes.
    pub(super) const fn u64_to(value: u64) -> [u8; 8] {
        value.to_le_bytes()
    }

    /// An `f32`'s four file bytes.
    pub(super) const fn f32_to(value: f32) -> [u8; 4] {
        value.to_le_bytes()
    }

    /// A `u32` from its four file bytes.
    pub(super) const fn u32_from(bytes: [u8; 4]) -> u32 {
        u32::from_le_bytes(bytes)
    }

    /// A `u64` from its eight file bytes.
    pub(super) const fn u64_from(bytes: [u8; 8]) -> u64 {
        u64::from_le_bytes(bytes)
    }

    /// An `f32` from its four file bytes.
    pub(super) const fn f32_from(bytes: [u8; 4]) -> f32 {
        f32::from_le_bytes(bytes)
    }
}

/// Why a cache file could not be read back.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CacheFileError {
    /// The file does not start with the cache's magic bytes.
    #[error("not a terrain cache file")]
    BadMagic,
    /// The file was written in another format version.
    #[error("terrain cache format version {found}, expected {FORMAT_VERSION}")]
    Version {
        /// The version the file states.
        found: u32,
    },
    /// The file ends in the middle of what its header promises.
    #[error("terrain cache file is truncated")]
    Truncated,
    /// The file has bytes after its last patch.
    #[error("terrain cache file has {0} trailing bytes")]
    Trailing(usize),
    /// The file holds another region's land.
    #[error("terrain cache file is for region {found:#x}, expected {expected:#x}")]
    WrongRegion {
        /// The region the file states.
        found: u64,
        /// The region it was read for.
        expected: u64,
    },
    /// The file states more patches than any region has.
    #[error("terrain cache file states {0} patches")]
    PatchCount(u32),
    /// A patch states an edge no real patch has.
    #[error("terrain cache patch states an edge of {0} cells")]
    PatchSize(u32),
    /// A patch is not a land layer.
    #[error("terrain cache patch has non-land layer code {0:#04x}")]
    NotLand(u8),
}

/// Encode a region's land patches as one cache file.
///
/// Little-endian throughout: the magic, the format version (`u32`), the region
/// handle (`u64`) and the patch count (`u32`), then per patch its layer code
/// (`u8`), grid position and edge (`u32` each) and its `size × size` heights
/// (`f32`, row-major). A patch whose height count does not match its edge is
/// skipped rather than written in a shape [`decode_region`] would misread.
#[must_use]
pub fn encode_region(region: RegionHandle, patches: &[&TerrainPatch]) -> Vec<u8> {
    let well_formed: Vec<&TerrainPatch> = patches
        .iter()
        .copied()
        .filter(|patch| {
            patch.layer.is_land()
                && usize::try_from(u64::from(patch.size).saturating_mul(u64::from(patch.size)))
                    .is_ok_and(|cells| cells == patch.values.len())
        })
        .collect();
    let count = u32::try_from(well_formed.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(
        well_formed
            .iter()
            .map(|patch| patch.values.len().saturating_mul(4).saturating_add(13))
            .sum::<usize>()
            .saturating_add(20),
    );
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&le::u32_to(FORMAT_VERSION));
    bytes.extend_from_slice(&le::u64_to(region.0));
    bytes.extend_from_slice(&le::u32_to(count));
    for patch in well_formed
        .into_iter()
        .take(usize::try_from(count).unwrap_or(usize::MAX))
    {
        bytes.push(patch.layer.code());
        bytes.extend_from_slice(&le::u32_to(patch.patch_x));
        bytes.extend_from_slice(&le::u32_to(patch.patch_y));
        bytes.extend_from_slice(&le::u32_to(patch.size));
        for value in &patch.values {
            bytes.extend_from_slice(&le::f32_to(*value));
        }
    }
    bytes
}

/// A cursor over a cache file's bytes.
struct Reader<'bytes> {
    /// What has not been read yet.
    rest: &'bytes [u8],
}

impl Reader<'_> {
    /// The next `N` bytes, or [`CacheFileError::Truncated`] when fewer remain.
    fn take<const N: usize>(&mut self) -> Result<[u8; N], CacheFileError> {
        let (head, tail) = self
            .rest
            .split_first_chunk::<N>()
            .ok_or(CacheFileError::Truncated)?;
        self.rest = tail;
        Ok(*head)
    }

    /// The next little-endian `u32`.
    fn u32(&mut self) -> Result<u32, CacheFileError> {
        Ok(le::u32_from(self.take()?))
    }

    /// The next little-endian `u64`.
    fn u64(&mut self) -> Result<u64, CacheFileError> {
        Ok(le::u64_from(self.take()?))
    }

    /// The next little-endian `f32`.
    fn f32(&mut self) -> Result<f32, CacheFileError> {
        Ok(le::f32_from(self.take()?))
    }
}

/// Decode a cache file written by [`encode_region`] for `expected`.
///
/// # Errors
///
/// A [`CacheFileError`] when the bytes are not a well-formed cache file of the
/// current format for `expected`.
pub fn decode_region(
    expected: RegionHandle,
    bytes: &[u8],
) -> Result<Vec<TerrainPatch>, CacheFileError> {
    let mut reader = Reader { rest: bytes };
    if reader.take::<4>()? != MAGIC {
        return Err(CacheFileError::BadMagic);
    }
    let version = reader.u32()?;
    if version != FORMAT_VERSION {
        return Err(CacheFileError::Version { found: version });
    }
    let region = reader.u64()?;
    if region != expected.0 {
        return Err(CacheFileError::WrongRegion {
            found: region,
            expected: expected.0,
        });
    }
    let count = reader.u32()?;
    if count > MAX_PATCHES_PER_FILE {
        return Err(CacheFileError::PatchCount(count));
    }
    let mut patches = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
    for _ in 0..count {
        let [code] = reader.take::<1>()?;
        let layer = TerrainLayerType::from_code(code);
        if !layer.is_land() {
            return Err(CacheFileError::NotLand(code));
        }
        let patch_x = reader.u32()?;
        let patch_y = reader.u32()?;
        let size = reader.u32()?;
        if size == 0 || size > MAX_PATCH_SIZE {
            return Err(CacheFileError::PatchSize(size));
        }
        let cells = size.saturating_mul(size);
        let mut values = Vec::with_capacity(usize::try_from(cells).unwrap_or(0));
        for _ in 0..cells {
            values.push(reader.f32()?);
        }
        patches.push(TerrainPatch {
            region_handle: expected,
            layer,
            patch_x,
            patch_y,
            size,
            values,
        });
    }
    if !reader.rest.is_empty() {
        return Err(CacheFileError::Trailing(reader.rest.len()));
    }
    Ok(patches)
}

/// The cache file of `region` under `dir`.
fn region_file(dir: &Path, region: RegionHandle) -> PathBuf {
    dir.join(format!("{:016x}.{FILE_EXTENSION}", region.0))
}

/// Read `region`'s cached land patches from `dir`: `None` when it has no cache
/// file, or (with a warning) when the file cannot be read or decoded — the
/// region's live terrain then simply arrives without a head start, and the next
/// save replaces the bad file.
fn load_region(dir: &Path, region: RegionHandle) -> Option<Vec<TerrainPatch>> {
    let path = region_file(dir, region);
    let bytes = match fs_err::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warn!("reading the terrain cache: {error}");
            return None;
        }
    };
    match decode_region(region, &bytes) {
        Ok(patches) => Some(patches),
        Err(error) => {
            warn!(
                "ignoring the terrain cache file {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// Write one region's encoded land into `dir` — to a temporary name first,
/// renamed into place, so a reader never sees half a file — then prune the
/// directory to `max_regions` files.
fn write_region(dir: &Path, region: RegionHandle, bytes: &[u8], temp_tag: u64, max_regions: usize) {
    let path = region_file(dir, region);
    let temp = dir.join(format!(
        "{:016x}{TEMP_INFIX}{}-{temp_tag}",
        region.0,
        std::process::id()
    ));
    let written = fs_err::create_dir_all(dir)
        .and_then(|()| fs_err::write(&temp, bytes))
        .and_then(|()| fs_err::rename(&temp, &path));
    if let Err(error) = written {
        warn!(
            "writing the terrain cache for region {:#x}: {error}",
            region.0
        );
        // Do not leave the half-written temporary behind; it may not exist.
        if let Err(error) = fs_err::remove_file(&temp)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!("removing a terrain cache temporary: {error}");
        }
        return;
    }
    prune_regions(dir, max_regions);
}

/// Remove all but the `max_regions` most recently written region files in
/// `dir`, and any temporary a crash left behind.
fn prune_regions(dir: &Path, max_regions: usize) {
    let entries = match fs_err::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            warn!("listing the terrain cache: {error}");
            return;
        }
    };
    let now = SystemTime::now();
    let mut regions: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let modified = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let is_temp = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(TEMP_INFIX));
        if is_temp {
            if now.duration_since(modified).unwrap_or_default() >= STALE_TEMP_AGE {
                remove_quietly_if_gone(&path);
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(FILE_EXTENSION) {
            regions.push((modified, path));
        }
    }
    if regions.len() <= max_regions {
        return;
    }
    // Newest first; everything past the cap goes.
    regions.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_modified, path) in regions.into_iter().skip(max_regions) {
        remove_quietly_if_gone(&path);
    }
}

/// Remove `path`, accepting that a concurrent prune may have removed it first.
fn remove_quietly_if_gone(path: &Path) {
    if let Err(error) = fs_err::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        warn!("pruning the terrain cache: {error}");
    }
}

/// When a region's land last changed, for the save debounce.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Dirty {
    /// When it first changed since its last save, in seconds of app time.
    first: f64,
    /// When it last changed, in seconds of app time.
    last: f64,
}

impl Dirty {
    /// Whether the region is due a save at `now`: quiet long enough, or
    /// changing for too long.
    fn due(self, now: f64) -> bool {
        now - self.last >= SAVE_QUIET_SECS || now - self.first >= SAVE_MAX_DELAY_SECS
    }
}

/// The land-height cache on disk — see the [module documentation](self).
///
/// `Default` keeps nothing on disk; the viewer inserts one with its grid's
/// directory ([`TerrainDiskCache::new`]).
#[derive(Debug, Resource, Default)]
pub struct TerrainDiskCache {
    /// This grid's cache directory, or `None` to keep nothing on disk.
    dir: Option<PathBuf>,
    /// How many region files to keep ([`DEFAULT_MAX_REGIONS`] unless set).
    max_regions: usize,
    /// The regions whose cache file has been asked for this session, so each
    /// is read at most once.
    requested: HashSet<RegionHandle>,
    /// The cache reads in flight.
    loading: HashMap<RegionHandle, Task<Option<Vec<TerrainPatch>>>>,
    /// The regions whose land changed since they were last saved.
    dirty: HashMap<RegionHandle, Dirty>,
    /// A counter that keeps two writes' temporary file names apart.
    writes: u64,
}

impl TerrainDiskCache {
    /// A cache kept in `dir` (`None` keeps nothing on disk), bounded to
    /// [`DEFAULT_MAX_REGIONS`] regions.
    #[must_use]
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            max_regions: DEFAULT_MAX_REGIONS,
            ..Self::default()
        }
    }

    /// Start reading `region`'s cache file, unless it has been asked for
    /// already this session (or there is no cache directory, or the region is
    /// not yet known).
    fn request(&mut self, region: RegionHandle) {
        // Checked before the directory is cloned: this runs every frame for the
        // agent's region.
        if self.dir.is_none() || region.0 == 0 || !self.requested.insert(region) {
            return;
        }
        let Some(dir) = self.dir.clone() else {
            return;
        };
        debug!("reading the terrain cache for region {:#x}", region.0);
        let task = IoTaskPool::get().spawn(async move { load_region(&dir, region) });
        let _previous = self.loading.insert(region, task);
    }

    /// Note that `region`'s land changed at `now`.
    fn mark_dirty(&mut self, region: RegionHandle, now: f64) {
        if self.dir.is_none() || region.0 == 0 {
            return;
        }
        self.dirty
            .entry(region)
            .and_modify(|dirty| dirty.last = now)
            .or_insert(Dirty {
                first: now,
                last: now,
            });
    }

    /// A fresh tag for a write's temporary file name.
    const fn next_write_tag(&mut self) -> u64 {
        self.writes = self.writes.wrapping_add(1);
        self.writes
    }
}

/// Merge a region's patches read from disk into `state`'s land cache, under
/// any patch already there (a live one that streamed while the file was being
/// read is newer). Returns how many were added.
fn merge_cached_land(
    state: &mut TerrainState,
    region: RegionHandle,
    patches: Vec<TerrainPatch>,
) -> usize {
    let mut added = 0_usize;
    for patch in patches {
        let key: PatchKey = (region, patch.patch_x, patch.patch_y);
        if let std::collections::hash_map::Entry::Vacant(slot) = state.land_cache.entry(key) {
            let _patch = slot.insert(patch);
            added = added.saturating_add(1);
        }
    }
    if added > 0 {
        // A consumer sampling `land_height` (the parcel-border bands) can now
        // follow the ground where it found nothing before.
        state.bump_revision(region);
    }
    added
}

/// Every land patch of `region` in `state`'s land cache, encoded as its cache
/// file.
fn encode_cached_region(state: &TerrainState, region: RegionHandle) -> Vec<u8> {
    let patches: Vec<&TerrainPatch> = state
        .land_cache
        .iter()
        .filter_map(|(key, patch)| (key.0 == region).then_some(patch))
        .collect();
    encode_region(region, &patches)
}

/// Follow the session's terrain events: ask for a region's cache file when the
/// region first appears, and note each region whose land streamed or changed.
///
/// The agent's own region is asked for from [`SlIdentity`], which the login
/// response seeds and every handover updates: on a live grid that is known
/// before anything else about the region, its handshake included (which can
/// arrive stamped with handle zero, before the circuit's handle is learnt).
pub fn track_terrain_for_disk_cache(
    mut events: MessageReader<SlEvent>,
    mut cache: ResMut<TerrainDiskCache>,
    identity: Option<Res<SlIdentity>>,
    time: Res<Time<Real>>,
) {
    if let Some(region) = identity.and_then(|identity| identity.region_handle) {
        cache.request(region);
    }
    let now = time.elapsed_secs_f64();
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::RegionInfoHandshake(identity) => cache.request(identity.region_handle),
            SlSessionEvent::RegionChanged { region_handle, .. } => cache.request(*region_handle),
            SlSessionEvent::TerrainPatch(patch) if patch.layer.is_land() => {
                cache.mark_dirty(patch.region_handle, now);
            }
            _other => {}
        }
    }
}

/// Merge each finished cache read into the land cache.
pub fn apply_cached_land(mut cache: ResMut<TerrainDiskCache>, mut state: ResMut<TerrainState>) {
    let mut finished = Vec::new();
    for (region, task) in &mut cache.loading {
        if let Some(result) = block_on(poll_once(task)) {
            finished.push((*region, result));
        }
    }
    for (region, result) in finished {
        let _task = cache.loading.remove(&region);
        if let Some(patches) = result {
            let added = merge_cached_land(&mut state, region, patches);
            debug!(
                "restored {added} cached land patches for region {:#x}",
                region.0
            );
        }
    }
}

/// Save each region whose land has settled, on the [`IoTaskPool`].
pub fn save_settled_land(
    mut cache: ResMut<TerrainDiskCache>,
    state: Res<TerrainState>,
    time: Res<Time<Real>>,
) {
    let Some(dir) = cache.dir.clone() else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let due: Vec<RegionHandle> = cache
        .dirty
        .iter()
        .filter_map(|(region, dirty)| dirty.due(now).then_some(*region))
        .collect();
    for region in due {
        let _dirty = cache.dirty.remove(&region);
        let bytes = encode_cached_region(&state, region);
        let tag = cache.next_write_tag();
        let max_regions = cache.max_regions;
        let dir = dir.clone();
        IoTaskPool::get()
            .spawn(async move { write_region(&dir, region, &bytes, tag, max_regions) })
            .detach();
    }
}

/// Write every region not yet saved, synchronously, as the app exits.
///
/// In `Last`, because Bevy checks for [`AppExit`] only once the whole schedule
/// has run, so there is no later point at which the land can still reach the
/// disk.
pub fn save_land_on_exit(
    mut exit: MessageReader<AppExit>,
    mut cache: ResMut<TerrainDiskCache>,
    state: Res<TerrainState>,
) {
    if exit.read().count() == 0 {
        return;
    }
    let Some(dir) = cache.dir.clone() else {
        return;
    };
    let dirty: Vec<RegionHandle> = cache.dirty.drain().map(|(region, _dirty)| region).collect();
    for region in dirty {
        let bytes = encode_cached_region(&state, region);
        let tag = cache.next_write_tag();
        write_region(&dir, region, &bytes, tag, cache.max_regions);
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use pretty_assertions::assert_eq;
    use sl_client_bevy::{RegionHandle, TerrainLayerType, TerrainPatch};
    use sl_viewer_world_api::TerrainState;

    use super::{
        CacheFileError, Dirty, SAVE_MAX_DELAY_SECS, SAVE_QUIET_SECS, decode_region,
        encode_cached_region, encode_region, load_region, merge_cached_land, prune_regions,
        region_file, write_region,
    };

    /// The region the test patches belong to.
    const REGION: RegionHandle = RegionHandle(0x0003_e800_0003_e900);

    /// A land patch of `REGION` at grid `(x, y)` whose heights all equal `height`.
    fn patch(x: u32, y: u32, height: f32) -> TerrainPatch {
        TerrainPatch {
            region_handle: REGION,
            layer: TerrainLayerType::Land,
            patch_x: x,
            patch_y: y,
            size: 16,
            values: (0..256_u16).map(|i| height + f32::from(i) * 0.25).collect(),
        }
    }

    /// A fresh, empty directory of this test's own.
    fn tempdir(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after the epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "{}-terrain-cache-{tag}-{nanos}-{:?}",
            env!("CARGO_PKG_NAME"),
            std::thread::current().id()
        ));
        fs_err::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_region_round_trips_through_its_cache_file() {
        let a = patch(0, 0, 20.0);
        let b = TerrainPatch {
            layer: TerrainLayerType::LandExtended,
            size: 32,
            values: vec![7.5; 1024],
            ..patch(3, 9, 0.0)
        };
        let bytes = encode_region(REGION, &[&a, &b]);
        let decoded = decode_region(REGION, &bytes).expect("decodes");
        assert_eq!(decoded, vec![a, b]);
    }

    #[test]
    fn a_malformed_patch_is_left_out_rather_than_misencoded() {
        let good = patch(1, 1, 5.0);
        let short = TerrainPatch {
            values: vec![1.0; 10],
            ..patch(2, 2, 0.0)
        };
        let water = TerrainPatch {
            layer: TerrainLayerType::Water,
            ..patch(4, 4, 0.0)
        };
        let decoded = decode_region(REGION, &encode_region(REGION, &[&good, &short, &water]))
            .expect("decodes");
        assert_eq!(decoded, vec![good]);
    }

    #[test]
    fn a_damaged_or_foreign_file_is_refused() {
        let bytes = encode_region(REGION, &[&patch(0, 0, 1.0)]);
        assert_eq!(
            decode_region(RegionHandle(42), &bytes),
            Err(CacheFileError::WrongRegion {
                found: REGION.0,
                expected: 42
            })
        );
        let cut = bytes.get(..bytes.len().saturating_sub(3)).expect("shorter");
        assert_eq!(decode_region(REGION, cut), Err(CacheFileError::Truncated));
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            decode_region(REGION, &longer),
            Err(CacheFileError::Trailing(1))
        );
        let mut foreign = bytes.clone();
        if let Some(first) = foreign.first_mut() {
            *first = b'X';
        }
        assert_eq!(
            decode_region(REGION, &foreign),
            Err(CacheFileError::BadMagic)
        );
        let mut newer = bytes;
        if let Some(version) = newer.get_mut(4) {
            *version = 2;
        }
        assert_eq!(
            decode_region(REGION, &newer),
            Err(CacheFileError::Version { found: 2 })
        );
    }

    #[test]
    fn a_loaded_patch_never_replaces_one_that_streamed_live() {
        let mut state = TerrainState::default();
        let live = patch(0, 0, 100.0);
        let _old = state.land_cache.insert((REGION, 0, 0), live.clone());
        let before = state.region_revision(REGION);
        let added = merge_cached_land(&mut state, REGION, vec![patch(0, 0, 1.0), patch(1, 0, 2.0)]);
        assert_eq!(added, 1, "only the patch with no live copy is added");
        assert_eq!(state.land_cache.get(&(REGION, 0, 0)), Some(&live));
        assert_eq!(
            state.land_cache.get(&(REGION, 1, 0)),
            Some(&patch(1, 0, 2.0))
        );
        assert!(
            state.region_revision(REGION) > before,
            "consumers hear of the new land"
        );
    }

    #[test]
    fn a_saved_region_loads_back_into_a_fresh_session() {
        let dir = tempdir("roundtrip");
        let mut first = TerrainState::default();
        for patch in [patch(0, 0, 30.0), patch(1, 0, 31.0)] {
            let _old = first
                .land_cache
                .insert((REGION, patch.patch_x, patch.patch_y), patch);
        }
        // Another region's land is not part of this region's file.
        let other = RegionHandle(7);
        let _old = first.land_cache.insert(
            (other, 0, 0),
            TerrainPatch {
                region_handle: other,
                ..patch(0, 0, 0.0)
            },
        );
        write_region(&dir, REGION, &encode_cached_region(&first, REGION), 1, 8);

        let mut second = TerrainState::default();
        let loaded = load_region(&dir, REGION).expect("the file is there");
        assert_eq!(merge_cached_land(&mut second, REGION, loaded), 2);
        // Cell (4, 3) of the patch at grid (1, 0): 31 + (3·16 + 4)·0.25 = 44.
        let height = second.land_height(REGION, 20.5, 3.5);
        assert!(
            height.is_some_and(|height| (height - 44.0).abs() <= 1.0e-4),
            "the floor answers from the disk cache, got {height:?}"
        );
        assert!(
            load_region(&dir, other).is_none(),
            "nothing was saved for the other region"
        );
        fs_err::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn the_cache_keeps_only_the_newest_regions() {
        let dir = tempdir("prune");
        let regions: Vec<RegionHandle> = (1..=5_u64).map(RegionHandle).collect();
        for (age, region) in regions.iter().enumerate() {
            let path = region_file(&dir, *region);
            fs_err::write(&path, b"x").expect("write");
            let modified = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000 + u64::try_from(age).expect("small"));
            fs_err::File::options()
                .write(true)
                .open(&path)
                .expect("open")
                .file()
                .set_modified(modified)
                .expect("set mtime");
        }
        // A file that is not a region file is left alone.
        fs_err::write(dir.join("notes.txt"), b"x").expect("write");
        prune_regions(&dir, 2);
        let kept: Vec<bool> = regions
            .iter()
            .map(|region| region_file(&dir, *region).exists())
            .collect();
        assert_eq!(kept, vec![false, false, false, true, true]);
        assert!(dir.join("notes.txt").exists());
        fs_err::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_region_saves_once_quiet_or_after_the_longest_delay() {
        let dirty = Dirty {
            first: 10.0,
            last: 12.0,
        };
        assert!(!dirty.due(12.0 + SAVE_QUIET_SECS - 0.1), "still streaming");
        assert!(dirty.due(12.0 + SAVE_QUIET_SECS), "quiet long enough");
        let busy = Dirty {
            first: 10.0,
            last: 10.0 + SAVE_MAX_DELAY_SECS,
        };
        assert!(
            busy.due(10.0 + SAVE_MAX_DELAY_SECS),
            "changing for too long"
        );
    }
}
