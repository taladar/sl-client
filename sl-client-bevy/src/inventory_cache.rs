//! The thin file-I/O shell over the sans-IO inventory disk-cache core
//! ([`Session::inventory_cache_bytes`](sl_proto::Session::inventory_cache_bytes) /
//! [`load_inventory_cache`](sl_proto::Session::load_inventory_cache) /
//! [`merge_inventory_skeleton`](sl_proto::Session::merge_inventory_skeleton)).
//!
//! The pure crate owns the cache *format* (the 4-byte version header + binary
//! LLSD), the cacheable-snapshot filter (`Loaded` folders only), and the
//! skeleton reconciliation; this shell supplies the two things it cannot: the
//! **gzip envelope** (Firestorm's `<agent-uuid>.inv.llsd.gz` is gzipped) and the
//! actual filesystem read/write. Writes are **crash-safe**: the gzip is streamed
//! to a same-directory `…<pid>.tmp`, flushed and `fsync`ed, then atomically
//! `rename`d over the live file, so a crash mid-write never corrupts the cache
//! (the old file survives on any error).
//!
//! Save timing follows Firestorm's shutdown-only save (on logout, via
//! [`InventoryCache::save`]) plus an *optional* dirty/idle tick
//! ([`InventoryCache::maybe_save`]) purely for crash-safety — gated on
//! [`Session::inventory_dirty`](sl_proto::Session::inventory_dirty) so an
//! unchanged model is never needlessly rewritten.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read as _, Write as _};

use sl_proto::{AgentKey, InventoryCacheConfig, InventoryFolder, InventoryOwner, Session};

/// How long after the last save the dirty/idle tick waits before it persists a
/// changed model again. Firestorm has no periodic inventory save at all; this is
/// a crash-safety backstop, so the interval is coarse (a save costs a gzip +
/// `fsync`) and only fires when the model is actually dirty.
const SAVE_INTERVAL: Duration = Duration::from_secs(60);

/// The runtime inventory disk-cache reader/writer: the configuration, the
/// per-account cache directory, our own agent id (the cache file is named for
/// it), and the last-save instant the dirty/idle tick measures against.
#[derive(Debug)]
pub(crate) struct InventoryCache {
    /// The pure configuration (the master enable flag and the Library toggle).
    config: InventoryCacheConfig,
    /// The directory the `<agent-uuid>.inv.llsd.gz` / `.lib.inv.llsd.gz` files are
    /// written **directly** under (supplied verbatim by the runtime), or `None`
    /// to disable the cache. When `None`, [`enabled`](Self::enabled) is false and
    /// every load/save short-circuits.
    cache_dir: Option<PathBuf>,
    /// Our own agent id, once known — the cache file is named for it. `None`
    /// before login disables the cache (there is no file name to use).
    agent_id: Option<AgentKey>,
    /// The instant of the last save (or of construction), so the dirty/idle tick
    /// can space saves by [`SAVE_INTERVAL`].
    last_save: Instant,
}

impl InventoryCache {
    /// Builds the cache shell for `agent_id`'s account. `agent_cache_dir` is the
    /// directory the cache files are written **directly** under, supplied
    /// verbatim by the runtime; `None` (or a disabled `config`, or an unknown
    /// `agent_id`) disables the feature. `now` seeds the dirty/idle clock so the
    /// first periodic save is at most one [`SAVE_INTERVAL`] after login.
    pub(crate) const fn new(
        config: InventoryCacheConfig,
        agent_cache_dir: Option<PathBuf>,
        agent_id: Option<AgentKey>,
        now: Instant,
    ) -> Self {
        Self {
            config,
            cache_dir: agent_cache_dir,
            agent_id,
            last_save: now,
        }
    }

    /// Whether the cache is active — the feature enabled, a directory supplied,
    /// and our agent id known (it names the file). The cheap gate every load/save
    /// checks first, so a consumer that leaves the feature off pays nothing.
    pub(crate) const fn enabled(&self) -> bool {
        self.config.enabled && self.cache_dir.is_some() && self.agent_id.is_some()
    }

    /// The path of the agent tree's cache file (`<agent-uuid>.inv.llsd.gz`), or
    /// `None` when the cache is inactive.
    fn agent_path(&self) -> Option<PathBuf> {
        let (dir, agent) = (self.cache_dir.as_ref()?, self.agent_id?);
        self.config
            .enabled
            .then(|| dir.join(format!("{}.inv.llsd.gz", agent.uuid())))
    }

    /// The path of the Library tree's cache file (`<agent-uuid>.lib.inv.llsd.gz`),
    /// or `None` when the cache is inactive or the Library toggle is off.
    fn library_path(&self) -> Option<PathBuf> {
        let (dir, agent) = (self.cache_dir.as_ref()?, self.agent_id?);
        (self.config.enabled && self.config.cache_library)
            .then(|| dir.join(format!("{}.lib.inv.llsd.gz", agent.uuid())))
    }

    /// Loads the cached **agent** tree (if any) into `session` and reconciles it
    /// against the login `skeleton`, on the [`Event::InventorySkeleton`] tap. The
    /// disk cache is read and folded **before** the merge: a cached folder whose
    /// version matches the skeleton keeps its loaded contents (and is skipped by
    /// the background crawl), the rest are invalidated and queued for refetch. A
    /// cold/absent/corrupt file is simply ignored (a full refetch). Clears the
    /// dirty flag afterwards so the post-login state is the save baseline.
    pub(crate) fn load_agent(&self, session: &mut Session, skeleton: &[InventoryFolder]) {
        if !self.enabled() {
            return;
        }
        load_and_merge(session, InventoryOwner::Agent, self.agent_path(), skeleton);
    }

    /// Loads the cached **Library** tree (if the Library toggle is on) into
    /// `session` and reconciles it against the login `skeleton`, on the
    /// [`Event::LibraryInventory`] tap. Mirrors [`load_agent`](Self::load_agent)
    /// for the read-only Library tree.
    pub(crate) fn load_library(&self, session: &mut Session, skeleton: &[InventoryFolder]) {
        if !self.enabled() {
            return;
        }
        load_and_merge(
            session,
            InventoryOwner::Library,
            self.library_path(),
            skeleton,
        );
    }

    /// The dirty/idle save tick: persists the model when it has changed and at
    /// least [`SAVE_INTERVAL`] has passed since the last save. Cheap and
    /// self-gating — a no-op while the cache is disabled, the interval has not
    /// elapsed, or the model is unchanged. Call once per run-loop iteration.
    pub(crate) fn maybe_save(&mut self, session: &mut Session, now: Instant) {
        if !self.enabled() || now.duration_since(self.last_save) < SAVE_INTERVAL {
            return;
        }
        self.last_save = now;
        if session.inventory_dirty() {
            self.persist(session);
            session.clear_inventory_dirty();
        }
    }

    /// Persists the model unconditionally (the logout/shutdown save, mirroring
    /// Firestorm's save-at-cleanup). A no-op while the cache is disabled.
    pub(crate) fn save(&self, session: &mut Session) {
        if !self.enabled() {
            return;
        }
        self.persist(session);
        session.clear_inventory_dirty();
    }

    /// Writes the cacheable snapshot of each enabled tree to its gzip file
    /// (crash-safely). Best-effort: a serialise or I/O failure for one tree is
    /// logged and leaves the previous cache file intact, never failing the
    /// session.
    fn persist(&self, session: &Session) {
        for (owner, path) in [
            (InventoryOwner::Agent, self.agent_path()),
            (InventoryOwner::Library, self.library_path()),
        ] {
            let Some(path) = path else {
                continue;
            };
            match session.inventory_cache_bytes(owner) {
                Ok(bytes) => {
                    if let Err(error) = write_gz_atomic(&path, &bytes) {
                        tracing::warn!(path = %path.display(), %error, "inventory cache write failed");
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "inventory cache serialise failed");
                }
            }
        }
    }
}

/// The shared load+merge for one tree: read+gunzip `path` (if present) into the
/// model under `owner`, then merge against `skeleton` and reset the dirty
/// baseline. `path` is `None` when that tree's cache is disabled (e.g. the
/// Library toggle off), in which case only the merge runs (a full refetch). The
/// cache is folded **before** the merge so a version-matching folder keeps its
/// loaded contents.
fn load_and_merge(
    session: &mut Session,
    owner: InventoryOwner,
    path: Option<PathBuf>,
    skeleton: &[InventoryFolder],
) {
    if let Some(path) = path
        && let Some(bytes) = read_gz(&path)
        && let Err(error) = session.load_inventory_cache(owner, &bytes)
    {
        tracing::warn!(path = %path.display(), %error, "inventory cache decode failed");
    }
    let _needing = session.merge_inventory_skeleton(owner, skeleton);
    session.clear_inventory_dirty();
}

/// Reads and gunzips the cache file at `path`, returning its un-gzipped bytes, or
/// `None` if the file is absent or unreadable/undecodable (treated as a cold
/// cache — the caller refetches the whole tree).
fn read_gz(path: &Path) -> Option<Vec<u8>> {
    let file = fs_err::File::open(path).ok()?;
    let mut decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The same-directory temp path a crash-safe write streams to before the atomic
/// rename: the target path with a `.<pid>.tmp` suffix, so concurrent processes
/// (or a crashed previous run) never clash on one temp name.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

/// Crash-safely writes the gzip of `bytes` to `path`: streams the gzip to a
/// same-directory temp file, flushes and `fsync`s it, then atomically renames it
/// over the live file. On any error the temp file is removed and the previous
/// cache (if any) is left untouched.
fn write_gz_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    let tmp = temp_path(path);
    if let Err(error) = stream_gz(&tmp, bytes) {
        let _removed = fs_err::remove_file(&tmp);
        return Err(error);
    }
    if let Err(error) = fs_err::rename(&tmp, path) {
        let _removed = fs_err::remove_file(&tmp);
        return Err(error);
    }
    Ok(())
}

/// Streams the gzip of `bytes` into the file at `tmp`, finishing the gzip trailer
/// and `fsync`ing the file before it is closed (so the rename publishes a fully
/// durable file).
fn stream_gz(tmp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file = fs_err::File::create(tmp)?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(bytes)?;
    let file = encoder.finish()?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{InventoryCache, read_gz};
    use pretty_assertions::assert_eq;
    use sl_proto::{
        AgentKey, FolderState, InventoryCacheConfig, InventoryFolder, InventoryFolderKey,
        InventoryOwner, LoginParams, LoginRequest, Session, StartLocation,
    };
    use sl_wire::Llsd;
    use std::collections::HashMap;
    use std::time::Instant;
    use uuid::Uuid;

    /// Boxed error so a test can `?` both the wire errors and a `url` parse — the
    /// strict lints forbid `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A per-test temporary directory under the system temp dir, keyed by `tag`
    /// and namespaced by crate so the byte-identical tokio / bevy shells do not
    /// collide when their test binaries run in parallel.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let crate_name = env!("CARGO_PKG_NAME");
        let dir = std::env::temp_dir().join(format!("{crate_name}-invcache-test-{tag}"));
        let _ignored = fs_err::remove_dir_all(&dir);
        dir
    }

    /// A folder key from a small constant.
    fn fk(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(id))
    }

    /// A skeleton-style folder under `parent` (`None` ⇒ root) at `version`.
    fn folder(id: u128, parent: Option<u128>, version: i32) -> InventoryFolder {
        InventoryFolder {
            folder_id: fk(id),
            parent_id: parent.map(fk),
            name: format!("folder-{id}"),
            folder_type: -1,
            version,
        }
    }

    /// A bare session (no live circuit) — enough to drive the held inventory
    /// model through the public cache surface (load / merge / snapshot).
    fn session() -> Result<Session, TestError> {
        let request = LoginRequest::new(
            "Test",
            "User",
            "",
            StartLocation::Last,
            "sl-client-invcache-test",
            "0",
        );
        let login_uri = url::Url::parse("http://127.0.0.1:9000/")?;
        Ok(Session::new(LoginParams { login_uri, request }))
    }

    /// The enabled configuration (the master switch on; Library cached too).
    fn enabled_config() -> InventoryCacheConfig {
        InventoryCacheConfig {
            enabled: true,
            cache_library: true,
        }
    }

    /// A minimal valid un-gzipped disk-cache blob holding one `Loaded` root
    /// folder `F0` at version 5 and no items, in the Firestorm
    /// `categories`/`items` shape (the 4-byte version header + binary LLSD).
    /// Loading it marks `F0` `Loaded` — the only no-circuit way to give a bare
    /// session cacheable content to round-trip through the shell.
    fn seed_blob() -> Vec<u8> {
        let category = Llsd::Map(HashMap::from([
            ("category_id".to_owned(), Llsd::Uuid(fk(0xF0).uuid())),
            ("parent_id".to_owned(), Llsd::Uuid(Uuid::nil())),
            ("name".to_owned(), Llsd::String("My Inventory".to_owned())),
            ("type_default".to_owned(), Llsd::Integer(-1)),
            ("version".to_owned(), Llsd::Integer(5)),
        ]));
        let map = Llsd::Map(HashMap::from([
            ("categories".to_owned(), Llsd::Array(vec![category])),
            ("items".to_owned(), Llsd::Array(Vec::new())),
        ]));
        let mut bytes = vec![0u8, 0, 0, 5];
        bytes.extend_from_slice(&map.to_llsd_binary());
        bytes
    }

    /// Whether `haystack` contains the contiguous byte sequence `needle`.
    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn round_trips_save_gunzip_header_and_load_merge() -> Result<(), TestError> {
        let dir = temp_dir("round-trip");
        let agent = AgentKey::from(Uuid::from_u128(1));
        let skeleton = vec![folder(0xF0, None, 5)];

        // Writer: seed `F0` Loaded@5 (no circuit), then reconcile against the
        // skeleton — the version matches, so `F0` stays Loaded and cacheable.
        let mut writer = session()?;
        let loaded = writer.load_inventory_cache(InventoryOwner::Agent, &seed_blob())?;
        assert_eq!(loaded, true);
        let _needing = writer.merge_inventory_skeleton(InventoryOwner::Agent, &skeleton);

        let cache = InventoryCache::new(
            enabled_config(),
            Some(dir.clone()),
            Some(agent),
            Instant::now(),
        );
        cache.save(&mut writer);

        // The file exists, gunzips, carries the version-`5` header, and holds the
        // Firestorm-shaped `category_id` key.
        let path = dir.join(format!("{}.inv.llsd.gz", agent.uuid()));
        let unzipped = read_gz(&path).ok_or("cache file gunzips")?;
        assert_eq!(unzipped.get(..4), Some(&[0, 0, 0, 5][..]));
        assert_eq!(contains(&unzipped, b"category_id"), true);

        // A fresh session loads the same file and reconciles it against the
        // skeleton: `F0`'s version matches, so it stays Loaded (no refetch) — the
        // model round-trips through the on-disk cache, and the baseline is clean.
        let mut reader = session()?;
        cache.load_agent(&mut reader, &skeleton);
        let restored = reader
            .inventory_folder(fk(0xF0))
            .ok_or("F0 present after load")?;
        assert_eq!(restored.version, 5);
        assert_eq!(
            reader.folder_fetch_state(fk(0xF0)),
            Some(FolderState::Loaded { version: 5 })
        );
        assert_eq!(reader.inventory_dirty(), false);
        Ok(())
    }

    #[test]
    fn disabled_config_writes_nothing() -> Result<(), TestError> {
        let dir = temp_dir("disabled");
        let agent = AgentKey::from(Uuid::from_u128(1));
        let mut writer = session()?;
        let _needing =
            writer.merge_inventory_skeleton(InventoryOwner::Agent, &[folder(0xF0, None, 5)]);
        let cache = InventoryCache::new(
            InventoryCacheConfig::default(),
            Some(dir.clone()),
            Some(agent),
            Instant::now(),
        );
        cache.save(&mut writer);
        assert_eq!(dir.exists(), false);
        Ok(())
    }

    #[test]
    fn library_toggle_off_skips_the_library_file() -> Result<(), TestError> {
        let dir = temp_dir("lib-toggle");
        let agent = AgentKey::from(Uuid::from_u128(1));
        let mut writer = session()?;
        let _needing =
            writer.merge_inventory_skeleton(InventoryOwner::Agent, &[folder(0xF0, None, 5)]);
        let cache = InventoryCache::new(
            InventoryCacheConfig {
                enabled: true,
                cache_library: false,
            },
            Some(dir.clone()),
            Some(agent),
            Instant::now(),
        );
        cache.save(&mut writer);
        assert_eq!(
            dir.join(format!("{}.inv.llsd.gz", agent.uuid())).exists(),
            true
        );
        assert_eq!(
            dir.join(format!("{}.lib.inv.llsd.gz", agent.uuid()))
                .exists(),
            false
        );
        Ok(())
    }
}
