//! Pure inventory disk-cache (de)serialisation and skeleton-merge plumbing.
//!
//! The held [`Inventory`] model is persisted between runs in the
//! Firestorm-compatible `<agent-uuid>.inv.llsd.gz` shape: a 4-byte big-endian
//! version header (= [`INVENTORY_CACHE_VERSION`], matching Firestorm's
//! `sCurrentInvCacheVersion`), followed by a binary-LLSD map
//! `{ "categories": [...], "items": [...] }`. This module owns the **pure** core
//! of that scheme — no filesystem, no gzip, no clock: it turns the cacheable
//! snapshot of a tree into those bytes ([`inventory_to_cache_bytes`]) and parses
//! such bytes back into a [`CachedInventory`] ([`inventory_from_cache_bytes`],
//! version-gated), then folds a parsed cache into a model
//! ([`load_cached_into`]). The runtime shells (`sl-client-tokio` /
//! `sl-client-bevy`) wrap these with the gzip envelope and the atomic
//! temp+rename file write.
//!
//! The skeleton-merge step (reconciling a loaded cache against the authoritative
//! login skeleton) lives on the model itself as
//! [`Inventory::merge_skeleton`](super::inventory::Inventory::merge_skeleton).

use sl_wire::{Llsd, LlsdError, WireError, parse_llsd_binary};

use super::conversions::{
    inventory_folder_from_llsd, inventory_folder_to_llsd, inventory_item_from_llsd,
    inventory_item_to_llsd, llsd_map, u32_be_bytes,
};
use super::inventory::{Inventory, InventoryOwner};
use crate::types::{InventoryFolder, InventoryItem};

/// The inventory disk-cache format version, written as the 4-byte big-endian
/// header and required (exactly) on load — matching Firestorm's
/// `LLInventoryModel::sCurrentInvCacheVersion`. A file with any other header is
/// treated as cold (ignored), forcing a full refetch.
pub const INVENTORY_CACHE_VERSION: u32 = 5;

/// The folders and items parsed out of a version-valid cache file, ready to fold
/// into the held model via [`load_cached_into`]. Every folder is taken to be
/// fully fetched (its cached contents are present), so loading marks it
/// [`Loaded`](super::inventory::FolderState::Loaded) at its stored version before
/// the login skeleton arrives to confirm or invalidate it.
pub(crate) struct CachedInventory {
    /// The cached folders, each carrying its stored authoritative version.
    pub(crate) folders: Vec<InventoryFolder>,
    /// The cached items, each filed under one of the cached folders.
    pub(crate) items: Vec<InventoryItem>,
}

/// Serialises the cacheable snapshot of one tree (`owner`) to the un-gzipped
/// cache bytes: the 4-byte big-endian [`INVENTORY_CACHE_VERSION`] header followed
/// by the binary-LLSD map `{ "categories": [...], "items": [...] }`. Only
/// [`Loaded`](super::inventory::FolderState::Loaded) folders and the items filed
/// in them are written (see [`Inventory::cacheable_snapshot`]). The runtime shell
/// gzips the result before writing it to disk.
///
/// # Errors
///
/// Returns [`WireError`] if an item fails to serialise (an out-of-range sale
/// price).
pub(crate) fn inventory_to_cache_bytes(
    inventory: &Inventory,
    owner: InventoryOwner,
) -> Result<Vec<u8>, WireError> {
    let (folders, items) = inventory.cacheable_snapshot(owner);
    let categories = folders
        .iter()
        .map(|folder| inventory_folder_to_llsd(folder));
    let item_entries = items
        .iter()
        .map(|item| inventory_item_to_llsd(item))
        .collect::<Result<Vec<_>, _>>()?;
    let map = llsd_map(vec![
        ("categories", Llsd::Array(categories.collect())),
        ("items", Llsd::Array(item_entries)),
    ]);
    let mut bytes = u32_be_bytes(INVENTORY_CACHE_VERSION).to_vec();
    bytes.extend_from_slice(&map.to_llsd_binary());
    Ok(bytes)
}

/// Parses un-gzipped cache bytes back into a [`CachedInventory`], gated on the
/// version header. Returns `Ok(None)` when the file is **cold** — too short to
/// hold the header, or carrying a version other than [`INVENTORY_CACHE_VERSION`]
/// (a stale or foreign format) — so the caller treats it as no cache and refetches
/// the whole tree. Returns `Err` only when a version-`5` payload fails to decode
/// as binary LLSD.
///
/// # Errors
///
/// Returns [`LlsdError`] if a version-valid payload is not decodable binary LLSD.
pub(crate) fn inventory_from_cache_bytes(
    bytes: &[u8],
) -> Result<Option<CachedInventory>, LlsdError> {
    let Some((header, payload)) = bytes.split_first_chunk::<4>() else {
        return Ok(None);
    };
    let version = (u32::from(header[0]) << 24)
        | (u32::from(header[1]) << 16)
        | (u32::from(header[2]) << 8)
        | u32::from(header[3]);
    if version != INVENTORY_CACHE_VERSION {
        return Ok(None);
    }
    let map = parse_llsd_binary(payload)?;
    let folders = map
        .get("categories")
        .and_then(Llsd::as_array)
        .unwrap_or_default()
        .iter()
        .map(inventory_folder_from_llsd)
        .collect();
    let items = map
        .get("items")
        .and_then(Llsd::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(inventory_item_from_llsd)
        .collect();
    Ok(Some(CachedInventory { folders, items }))
}

/// Folds a parsed cache into the held model under `owner`, marking every cached
/// folder [`Loaded`](super::inventory::FolderState::Loaded) at its stored version
/// and filing its cached items. This is the "load the disk cache into the model"
/// step that runs **before** the login skeleton arrives; the subsequent
/// [`Inventory::merge_skeleton`](super::inventory::Inventory::merge_skeleton)
/// then confirms each loaded folder (version match) or invalidates it (mismatch /
/// server-deleted).
pub(crate) fn load_cached_into(
    inventory: &mut Inventory,
    cached: &CachedInventory,
    owner: InventoryOwner,
) {
    for folder in &cached.folders {
        let key = folder.folder_id;
        let version = folder.version;
        inventory.cache_folder(folder.clone(), owner);
        inventory.mark_folder_loaded(key, version, owner);
    }
    for item in &cached.items {
        inventory.cache_item(item.clone(), owner);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, OwnerKey};
    use sl_wire::Permissions5;
    use uuid::Uuid;

    use super::{
        CachedInventory, INVENTORY_CACHE_VERSION, inventory_from_cache_bytes,
        inventory_to_cache_bytes, load_cached_into,
    };
    use crate::session::inventory::{FolderState, Inventory, InventoryOwner};
    use crate::types::{InventoryFolder, InventoryItem};

    /// Boxed error so a test can `?` both [`sl_wire::WireError`] (serialise) and
    /// [`sl_wire::LlsdError`] (parse) — the strict lints forbid `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A folder key from a small constant.
    fn fk(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(id))
    }

    /// An item key from a small constant.
    fn ik(id: u128) -> InventoryKey {
        InventoryKey::from(Uuid::from_u128(id))
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

    /// A minimal item filed in `folder`.
    fn item(id: u128, folder: u128) -> InventoryItem {
        InventoryItem {
            item_id: ik(id),
            folder_id: fk(folder),
            name: format!("item-{id}"),
            description: String::new(),
            asset_id: Uuid::from_u128(0xA55E7),
            item_type: 0,
            inv_type: 0,
            flags: 0,
            sale_type: 0,
            sale_price: None,
            creation_date: 1700,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(1))),
            last_owner_id: Uuid::nil(),
            creator_id: AgentKey::from(Uuid::from_u128(1)),
            group: None,
            permissions: Permissions5::empty(),
        }
    }

    /// A model with a `Loaded` root holding one sub-folder and two items, plus an
    /// `Unknown` sub-folder that must NOT be written to the cache.
    fn seeded() -> Inventory {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 5), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 3), InventoryOwner::Agent);
        inv.cache_item(item(0xD1, 0xF0), InventoryOwner::Agent);
        inv.cache_item(item(0xD2, 0xF0), InventoryOwner::Agent);
        inv.mark_folder_loaded(fk(0xF0), 5, InventoryOwner::Agent);
        // F1 stays `Unknown` (never fetched) — excluded from the snapshot.
        inv
    }

    /// The cache bytes start with the 4-byte big-endian version header and
    /// round-trip through `inventory_from_cache_bytes` to an equal set of
    /// `Loaded` folders + their items.
    #[test]
    fn cache_bytes_round_trip() -> Result<(), TestError> {
        let inv = seeded();
        let bytes = inventory_to_cache_bytes(&inv, InventoryOwner::Agent)?;
        assert_eq!(bytes.get(..4), Some(&[0, 0, 0, 5][..]));

        let cached = inventory_from_cache_bytes(&bytes)?.ok_or("version valid")?;
        // Only the `Loaded` F0 is written, not the `Unknown` F1.
        let mut folder_ids: Vec<u128> = cached
            .folders
            .iter()
            .map(|f| f.folder_id.uuid().as_u128())
            .collect();
        folder_ids.sort_unstable();
        assert_eq!(folder_ids, vec![0xF0]);
        let mut item_ids: Vec<u128> = cached
            .items
            .iter()
            .map(|i| i.item_id.uuid().as_u128())
            .collect();
        item_ids.sort_unstable();
        assert_eq!(item_ids, vec![0xD1, 0xD2]);
        // Field-level fidelity of a round-tripped item.
        let d1 = cached
            .items
            .iter()
            .find(|i| i.item_id == ik(0xD1))
            .ok_or("D1 present")?;
        assert_eq!(d1.folder_id, fk(0xF0));
        assert_eq!(d1.asset_id, Uuid::from_u128(0xA55E7));
        assert_eq!(d1.creation_date, 1700);
        Ok(())
    }

    /// Loading the parsed cache back into a fresh model restores the `Loaded`
    /// folders (at their stored versions) and the items.
    #[test]
    fn load_cached_restores_loaded_folders() -> Result<(), TestError> {
        let bytes = inventory_to_cache_bytes(&seeded(), InventoryOwner::Agent)?;
        let cached = inventory_from_cache_bytes(&bytes)?.ok_or("version valid")?;

        let mut restored = Inventory::new();
        load_cached_into(&mut restored, &cached, InventoryOwner::Agent);
        assert_eq!(
            restored.folder_state(fk(0xF0)),
            Some(FolderState::Loaded { version: 5 })
        );
        assert!(restored.item(ik(0xD1)).is_some());
        assert!(restored.item(ik(0xD2)).is_some());
        Ok(())
    }

    /// A header other than `5` is cold: `inventory_from_cache_bytes` returns
    /// `Ok(None)`, and merging that empty result against the skeleton marks every
    /// skeleton folder `Unknown` (a full refetch).
    #[test]
    fn version_mismatch_is_cold() -> Result<(), TestError> {
        let mut bytes = inventory_to_cache_bytes(&seeded(), InventoryOwner::Agent)?;
        *bytes.get_mut(3).ok_or("header present")? = 4; // header now reads version 4
        assert!(inventory_from_cache_bytes(&bytes)?.is_none());
        // A truncated file (no full header) is cold too.
        assert!(inventory_from_cache_bytes(&[0, 0])?.is_none());

        // Merging against the (empty) loaded model ⇒ every skeleton folder fetched.
        let mut inv = Inventory::new();
        let skeleton = vec![folder(0xF0, None, 5), folder(0xF1, Some(0xF0), 3)];
        let needing = inv.merge_skeleton(&skeleton, InventoryOwner::Agent);
        assert_eq!(needing.len(), 2);
        assert_eq!(inv.folder_state(fk(0xF0)), Some(FolderState::Unknown));
        Ok(())
    }

    /// Merge keeps a version-matching folder (absent from the fetch list, contents
    /// retained) and invalidates a stale one (present in the fetch list, contents
    /// dropped), drops a server-deleted folder, and purges items under a
    /// now-`Unknown` folder while keeping those under a still-`Loaded` one.
    #[test]
    fn merge_keeps_matching_drops_stale_and_deleted() {
        // Cache: F0 v5 (loaded), F1 v3 (loaded, will go stale), F9 (server-deleted).
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 5), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF1, Some(0xF0), 3), InventoryOwner::Agent);
        inv.cache_folder(folder(0xF9, Some(0xF0), 1), InventoryOwner::Agent);
        inv.cache_item(item(0xD1, 0xF0), InventoryOwner::Agent); // under a kept folder
        inv.cache_item(item(0xD2, 0xF1), InventoryOwner::Agent); // under a stale folder
        inv.mark_folder_loaded(fk(0xF0), 5, InventoryOwner::Agent);
        inv.mark_folder_loaded(fk(0xF1), 3, InventoryOwner::Agent);
        inv.mark_folder_loaded(fk(0xF9), 1, InventoryOwner::Agent);

        // Skeleton: F0 still v5 (match), F1 now v4 (mismatch), F9 gone, F2 new.
        let skeleton = vec![
            folder(0xF0, None, 5),
            folder(0xF1, Some(0xF0), 4),
            folder(0xF2, Some(0xF0), 1),
        ];
        let mut needing = inv.merge_skeleton(&skeleton, InventoryOwner::Agent);
        needing.sort_unstable();

        // F0 kept Loaded (not refetched); F1 & F2 need fetch.
        assert_eq!(needing, vec![fk(0xF1), fk(0xF2)]);
        assert_eq!(
            inv.folder_state(fk(0xF0)),
            Some(FolderState::Loaded { version: 5 })
        );
        assert_eq!(inv.folder_state(fk(0xF1)), Some(FolderState::Unknown));
        // F9 dropped entirely (server-deleted).
        assert_eq!(inv.folder_state(fk(0xF9)), None);
        // Item under the kept F0 survives; item under the now-Unknown F1 is purged.
        assert!(inv.item(ik(0xD1)).is_some());
        assert!(inv.item(ik(0xD2)).is_none());
    }

    /// Merging the agent skeleton leaves library folders untouched (per-owner).
    #[test]
    fn merge_is_per_owner() {
        let mut inv = Inventory::new();
        inv.cache_folder(folder(0xF0, None, 5), InventoryOwner::Agent);
        inv.cache_folder(folder(0x71B, None, 9), InventoryOwner::Library);
        inv.mark_folder_loaded(fk(0x71B), 9, InventoryOwner::Library);

        // Agent skeleton lists only F0; the library folder must NOT be dropped.
        let _needing = inv.merge_skeleton(&[folder(0xF0, None, 5)], InventoryOwner::Agent);
        assert_eq!(
            inv.folder_state(fk(0x71B)),
            Some(FolderState::Loaded { version: 9 })
        );
    }

    /// A `CachedInventory` built by hand loads without panicking even when an item
    /// references a not-yet-present folder (robustness of `load_cached_into`).
    #[test]
    fn load_tolerates_orphan_item() {
        let cached = CachedInventory {
            folders: vec![folder(0xF0, None, INVENTORY_CACHE_VERSION.cast_signed())],
            items: vec![item(0xD1, 0xDEAD)],
        };
        let mut inv = Inventory::new();
        load_cached_into(&mut inv, &cached, InventoryOwner::Agent);
        assert!(inv.item(ik(0xD1)).is_some());
    }
}
