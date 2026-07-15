//! The cross-restart disk cache for the grid's `LSLSyntax` document, keyed by
//! the `LSLSyntaxId` the region advertises.
//!
//! The document is large (a few hundred KiB) and changes rarely — that is
//! exactly what the syntax *id* is for — so caching it by id makes a restart on
//! the same grid free (Firestorm caches `keywords_lsl_<id>.llsd.xml` the same
//! way, in `llsyntaxid.cpp`). This cache lives under the **shared** cache
//! directory rather than the per-account one: the language definition is a
//! property of the grid/region, not the logged-in avatar, so every account on
//! the machine shares one copy.
//!
//! The stored bytes are the **raw fetched LLSD-XML**, gzipped, so a load
//! round-trips through the same [`parse_llsd_xml`] decode a fresh fetch takes.
//! Writes are crash-safe (temp file + `fsync` + atomic rename) and every
//! operation is best-effort: a cold, absent or corrupt file simply misses and
//! the runtime refetches.
//!
//! Caveat (documented, not worked around): OpenSim's syntax id is a **hardcoded
//! UUID in `ScriptSyntax.xml`**, not a content hash, so a grid operator who
//! edits the file without bumping the id will serve content a by-id cache treats
//! as unchanged. This matches Firestorm's behaviour; a cache miss only ever
//! costs one refetch, and deleting the cache directory forces a refresh.

use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read as _, Write as _};

use sl_proto::{Llsd, parse_llsd_xml};
use uuid::Uuid;

/// The subdirectory (under the shared cache dir) the syntax files live in, so
/// they do not clutter the shared cache root alongside other shared-cache
/// features.
const SUBDIR: &str = "lsl-syntax";

/// The runtime `LSLSyntax` disk-cache reader/writer: just the shared cache
/// directory (or `None` to disable). Cheaply cloneable so a fetch task can carry
/// its own handle.
#[derive(Debug, Clone)]
pub(crate) struct LslSyntaxCache {
    /// The shared cache directory the `lsl-syntax/` subdirectory is created
    /// under, or `None` to disable the cache (every load/store short-circuits).
    shared_cache_dir: Option<PathBuf>,
}

impl LslSyntaxCache {
    /// Builds the cache over the runtime's shared cache directory. `None`
    /// disables the feature (loads miss, stores are no-ops).
    pub(crate) const fn new(shared_cache_dir: Option<PathBuf>) -> Self {
        Self { shared_cache_dir }
    }

    /// The cache-file path for syntax `id` (`<shared>/lsl-syntax/keywords_lsl_<id>.llsd.xml.gz`),
    /// or `None` when the cache is disabled.
    fn path(&self, id: Uuid) -> Option<PathBuf> {
        self.shared_cache_dir.as_ref().map(|dir| {
            dir.join(SUBDIR)
                .join(format!("keywords_lsl_{id}.llsd.xml.gz"))
        })
    }

    /// Loads and decodes the cached document for `id`, or `None` on a miss
    /// (disabled cache, absent file, or unreadable / undecodable content — all
    /// treated as a cold cache the caller refetches).
    pub(crate) fn load(&self, id: Uuid) -> Option<Llsd> {
        let path = self.path(id)?;
        let bytes = read_gz(&path)?;
        let text = String::from_utf8(bytes).ok()?;
        parse_llsd_xml(&text).ok()
    }

    /// Caches the raw fetched LLSD-XML `xml` for syntax `id`, crash-safely.
    /// Best-effort: a disabled cache is a no-op, and an I/O failure is logged and
    /// leaves any previous file intact rather than failing the fetch.
    pub(crate) fn store(&self, id: Uuid, xml: &str) {
        let Some(path) = self.path(id) else {
            return;
        };
        if let Err(error) = write_gz_atomic(&path, xml.as_bytes()) {
            tracing::warn!(path = %path.display(), %error, "LSLSyntax cache write failed");
        }
    }
}

/// Reads and gunzips the cache file at `path`, or `None` if it is absent or
/// unreadable/undecodable (a cold cache — the caller refetches).
fn read_gz(path: &Path) -> Option<Vec<u8>> {
    let file = fs_err::File::open(path).ok()?;
    let mut decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The same-directory temp path a crash-safe write streams to before the atomic
/// rename — the target path with a `.<pid>.tmp` suffix, so concurrent processes
/// never clash on one temp name.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

/// Crash-safely writes the gzip of `bytes` to `path`: streams the gzip to a
/// same-directory temp file, `fsync`s it, then atomically renames it over the
/// live file. On any error the temp file is removed and the previous file (if
/// any) is left untouched.
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
/// and `fsync`ing before close so the rename publishes a fully durable file.
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
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::LslSyntaxCache;

    /// A store-then-load round-trips the cached document, and a load for an
    /// unknown id (or from a disabled cache) misses cleanly.
    #[test]
    fn store_then_load_round_trips() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("sl-lsl-syntax-cache-{}", std::process::id()));
        let cache = LslSyntaxCache::new(Some(dir.clone()));
        let id = Uuid::parse_str("4b833b57-d52b-5503-85a4-76754ac3b8ff")
            .map_err(|error| error.to_string())?;
        let xml = concat!(
            "<llsd><map>",
            "<key>llsd-lsl-syntax-version</key><integer>2</integer>",
            "<key>functions</key><map></map>",
            "</map></llsd>"
        );

        assert!(cache.load(id).is_none(), "cold cache must miss");
        cache.store(id, xml);
        let loaded = cache.load(id).ok_or("expected a cache hit after store")?;
        // The decoded document carries the version key we stored.
        assert_eq!(
            loaded
                .field_i32("llsd-lsl-syntax-version", "version")
                .map_err(|error| format!("{error:?}"))?,
            Some(2)
        );

        // An unknown id misses.
        let other = Uuid::parse_str("11111111-1111-1111-1111-111111111111")
            .map_err(|error| error.to_string())?;
        assert!(cache.load(other).is_none());

        // A disabled cache never hits, even after a (no-op) store.
        let disabled = LslSyntaxCache::new(None);
        disabled.store(id, xml);
        assert!(disabled.load(id).is_none());

        let _removed = fs_err::remove_dir_all(&dir);
        Ok(())
    }
}
