//! Fixtures for the legacy UDP asset paths: named `Xfer` files, the estate
//! covenant, and the estate terrain RAW heightmap.
//!
//! An object's task inventory used to live here too. It does not any more: a
//! contents *serial* is only meaningful if the store that answers it is the
//! store a write advances, so the listings moved to the region's own world
//! ([`SceneFixtures::task_inventories`](crate::SceneFixtures::task_inventories))
//! where the writes land. What is left here is genuinely fixture — bytes
//! stated up front and served back unchanged.
//!
//! The **bodies** of those task items followed, for the same reason one step
//! further out. They were a `(task, item)` map stated up front, which no
//! fixture could extend: an item dropped into a prim is minted a fresh task
//! item id, so its bytes could never have been stated, and the `TransferRequest`
//! for it was refused with `UnknownSource` — the item a test had just watched
//! the contents serial advance for was the one item whose asset could not be
//! read back. A task item now resolves the way every other asset fetch does:
//! through the item's own `asset_id`, against the one grid-wide store
//! (`assets.rs`). One place an asset id means something, rather than two
//! that can disagree about the same item.
//!
//! `SimSession` implements the server half of every one of these flows but
//! keeps no content of its own; the driver answers the corresponding
//! [`ServerEvent`]s from a per-session copy of these fixtures (the
//! crate-private `answer_from_fixtures`). Everything here is plain data so
//! a [`Scenario`](crate::Scenario) stays a value that can be cloned per
//! session and per region.

use std::collections::HashMap;
use std::time::Instant;

use sl_proto::{
    AssetSource as _, ServerEvent, SimSession, TransferRequestSource, TransferStatus, Uuid,
};
use sl_types::key::{InventoryKey, ObjectKey};
use sl_wire::{TransferSourceParamsEstate, TransferSourceParamsInvItem};

/// The scripted content behind the legacy UDP asset paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UdpAssetFixtures {
    /// Named files served over `RequestXfer`. Registered on every fresh
    /// session and re-armed after each serve (a `SimSession` registration
    /// is consumed by the request that names it).
    pub xfer_files: HashMap<String, Vec<u8>>,
    /// The estate covenant notecard body, answered on the estate-covenant
    /// `TransferRequest`. `None` is refused with
    /// [`TransferStatus::UnknownSource`] (an estate without a covenant).
    pub estate_covenant: Option<Vec<u8>>,
    /// The region's terrain RAW heightmap, offered over `InitiateDownload`
    /// on an estate terrain download request. A completed terrain upload
    /// replaces it, so a following download round-trips the uploaded bytes.
    /// `None` leaves a download request unanswered.
    pub terrain_raw: Option<Vec<u8>>,
}

impl UdpAssetFixtures {
    /// No fixtures at all.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a named `Xfer` file.
    #[must_use]
    pub fn with_xfer_file(mut self, filename: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        let _prev = self.xfer_files.insert(filename.into(), data.into());
        self
    }

    /// Sets the estate covenant notecard body.
    #[must_use]
    pub fn with_estate_covenant(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.estate_covenant = Some(data.into());
        self
    }

    /// Sets the terrain RAW heightmap.
    #[must_use]
    pub fn with_terrain_raw(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.terrain_raw = Some(data.into());
        self
    }

    /// Registers every named `Xfer` file on a fresh session.
    pub(crate) fn register_xfer_files(&self, sim: &mut SimSession) {
        for (filename, data) in &self.xfer_files {
            sim.register_xfer_file(filename.clone(), data.clone());
        }
    }

    /// Resolves the estate half of a UDP `Transfer` request to the bytes to
    /// serve, or `None` for an estate asset this estate has none of.
    ///
    /// The task-item half is not here: it resolves against the region's world
    /// and the grid store (`task_item_asset`), neither of which is a fixture.
    #[must_use]
    pub fn resolve_estate_transfer(&self, params: &TransferSourceParamsEstate) -> Option<&[u8]> {
        if params.estate_asset_type == sl_wire::ESTATE_ASSET_COVENANT {
            self.estate_covenant.as_deref()
        } else {
            None
        }
    }
}

/// The bytes behind a task-inventory item, resolved the way every other asset
/// fetch resolves: the item's own `asset_id`, looked up in the grid-wide store.
///
/// The request's own `asset_id` field is **not** trusted — a client may send
/// nil, or a stale id from a listing it fetched before somebody saved over the
/// item. What the region holds is the answer, which is also what makes a save
/// observable: the fetch that follows one returns the new bytes because the
/// item now names them.
#[must_use]
pub(crate) fn task_item_asset(
    world: &crate::world::RegionWorld,
    assets: &crate::assets::GridAssets,
    params: &TransferSourceParamsInvItem,
) -> Option<Vec<u8>> {
    let task = ObjectKey::from(params.task_id);
    let item_id = InventoryKey::from(params.item_id);
    let asset_id = {
        let world = world.lock();
        let local_id = world.local_id_of(task)?;
        world
            .task_inventories
            .get(&local_id)?
            .items
            .iter()
            .find(|held| held.item_id == item_id)?
            .asset_id?
    };
    assets.read().get(asset_id).map(<[u8]>::to_vec)
}

/// The side length of a legacy-sized region's heightmap, in samples.
pub const TERRAIN_RAW_SIDE: usize = 256;

/// The number of byte channels per sample in the terrain RAW format
/// (height, height multiplier, and eleven land-data channels).
pub const TERRAIN_RAW_CHANNELS: usize = 13;

/// The height-multiplier divisor of the terrain RAW format: a sample's
/// height in metres is `height_byte * multiplier_byte / 128`.
const TERRAIN_RAW_MULTIPLIER_DIVISOR: u8 = 128;

/// A flat terrain RAW heightmap (the estate "download RAW terrain" file
/// format: `256 × 256` samples of 13 byte channels, height in channel 0
/// scaled by channel 1 over 128) at `height_m` metres everywhere.
///
/// Heights above 255 m saturate; the land-data channels are zero.
#[must_use]
pub fn flat_terrain_raw(height_m: u8) -> Vec<u8> {
    let mut sample = [0_u8; TERRAIN_RAW_CHANNELS];
    if let Some(height) = sample.first_mut() {
        *height = height_m;
    }
    if let Some(multiplier) = sample.get_mut(1) {
        *multiplier = TERRAIN_RAW_MULTIPLIER_DIVISOR;
    }
    sample.repeat(TERRAIN_RAW_SIDE * TERRAIN_RAW_SIDE)
}

/// Answers one drained [`ServerEvent`] from the fixtures, under the session
/// lock: answers `Transfer` requests (or refuses them), offers and captures
/// the terrain RAW file, and re-arms a served named `Xfer` file. Send
/// failures are logged, never fatal — the client's own timeouts report an
/// unanswered request.
pub(crate) fn answer_from_fixtures(
    fixtures: &mut UdpAssetFixtures,
    assets: &crate::assets::GridAssets,
    world: &crate::world::RegionWorld,
    sim: &mut SimSession,
    region_id: Uuid,
    event: &ServerEvent,
    now: Instant,
) {
    match event {
        ServerEvent::TransferRequested {
            transfer_id,
            source,
            ..
        } => {
            let data = match source {
                TransferRequestSource::TaskInventoryItem(params) => {
                    task_item_asset(world, assets, params)
                }
                TransferRequestSource::Estate(params) => {
                    fixtures.resolve_estate_transfer(params).map(<[u8]>::to_vec)
                }
            };
            let result = match data {
                Some(data) => sim.send_transfer_asset(*transfer_id, &data, now),
                None => {
                    tracing::debug!("nothing to serve for {source:?}; refusing");
                    sim.send_transfer_fail(*transfer_id, TransferStatus::UnknownSource, now)
                }
            };
            if let Err(error) = result {
                tracing::warn!("answering transfer {transfer_id:?} failed: {error}");
            }
        }
        ServerEvent::TerrainDownloadRequested { viewer_filename } => {
            match &fixtures.terrain_raw {
                Some(data) => {
                    // The shape OpenSim's estate module mints: a per-region
                    // unique name the viewer never sees.
                    let sim_filename = format!("{region_id}-{viewer_filename}");
                    if let Err(error) =
                        sim.send_initiate_download(sim_filename, viewer_filename, data.clone(), now)
                    {
                        tracing::warn!("offering the terrain RAW download failed: {error}");
                    }
                }
                None => tracing::warn!("terrain RAW download requested but no heightmap fixture"),
            }
        }
        ServerEvent::TerrainUploadRequested { viewer_filename } => {
            if let Err(error) = sim.request_xfer_upload(viewer_filename, now) {
                tracing::warn!("pulling the terrain RAW upload failed: {error}");
            }
        }
        ServerEvent::XferReceived { data, filename, .. } => {
            tracing::info!(
                "terrain RAW upload {filename} received ({} bytes); replacing the heightmap",
                data.len()
            );
            fixtures.terrain_raw = Some(data.clone());
        }
        ServerEvent::TerrainBakeRequested => {
            tracing::debug!("terrain bake requested; the fake grid keeps no revert baseline");
        }
        ServerEvent::XferRequested {
            filename,
            served: true,
            ..
        } => {
            if let Some(data) = fixtures.xfer_files.get(filename) {
                sim.register_xfer_file(filename.clone(), data.clone());
            }
        }
        _other => {}
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::RegionLocalObjectId;
    use sl_proto::TaskInventoryItem;

    use super::*;

    /// A task-item request for the given task/item pair, with a deliberately
    /// **nil** asset id — the field a client may leave empty, and which the
    /// resolver must not read.
    fn item_params(task: u128, item: u128) -> TransferSourceParamsInvItem {
        TransferSourceParamsInvItem {
            agent_id: uuid::Uuid::nil(),
            session_id: uuid::Uuid::nil(),
            owner_id: uuid::Uuid::nil(),
            task_id: uuid::Uuid::from_u128(task),
            item_id: uuid::Uuid::from_u128(item),
            asset_id: uuid::Uuid::nil(),
            asset_type: 10,
        }
    }

    /// An estate request of the given estate asset type.
    fn estate_params(estate_asset_type: i32) -> TransferSourceParamsEstate {
        TransferSourceParamsEstate {
            agent_id: uuid::Uuid::nil(),
            session_id: uuid::Uuid::nil(),
            estate_asset_type,
        }
    }

    /// A one-object region whose object holds one task item naming `asset`.
    fn region_with_task_item(
        task: u128,
        item: u128,
        asset: sl_proto::AssetKey,
    ) -> crate::world::RegionWorld {
        let mut fixtures = crate::world::SceneFixtures::new();
        let full_id = ObjectKey::from(uuid::Uuid::from_u128(task));
        let local_id = RegionLocalObjectId(9);
        let unit = sl_types::lsl::Vector {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        };
        fixtures.objects.push(crate::world::box_prim(
            local_id,
            full_id,
            sl_proto::AgentKey::from(uuid::Uuid::from_u128(1)),
            unit.clone(),
            unit,
        ));
        let held = TaskInventoryItem {
            item_id: InventoryKey::from(uuid::Uuid::from_u128(item)),
            parent_task: full_id,
            asset_id: Some(asset),
            ..crate::scenario::stock_script_item()
        };
        let _previous = fixtures
            .task_inventories
            .insert(local_id, crate::world::TaskInventory::stated(1, vec![held]));
        std::sync::Arc::new(parking_lot::Mutex::new(fixtures))
    }

    /// A task item resolves through the item's own asset id against the grid
    /// store — **not** through the request's `asset_id` field, which is nil
    /// here, and not through a stated `(task, item)` map, which no longer
    /// exists.
    #[test]
    fn a_task_item_resolves_through_its_own_asset_id() {
        let asset = sl_proto::AssetKey::from(uuid::Uuid::from_u128(0xA55E7));
        let world = region_with_task_item(1, 2, asset);
        let assets = crate::assets::GridAssets::default();
        assets.extend(
            &sl_proto::InMemoryAssetSource::new().with_asset(asset, b"script body".to_vec()),
        );
        assert_eq!(
            task_item_asset(&world, &assets, &item_params(1, 2)),
            Some(b"script body".to_vec())
        );
        // An item the object does not hold, and an object the region does not
        // have, both resolve to nothing rather than to somebody else's bytes.
        assert_eq!(task_item_asset(&world, &assets, &item_params(1, 3)), None);
        assert_eq!(task_item_asset(&world, &assets, &item_params(4, 2)), None);
    }

    /// An item whose asset id names nothing in the store resolves to nothing —
    /// the honest answer, which the caller turns into `UnknownSource`.
    #[test]
    fn a_task_item_with_no_bytes_resolves_to_nothing() {
        let asset = sl_proto::AssetKey::from(uuid::Uuid::from_u128(0xA55E8));
        let world = region_with_task_item(1, 2, asset);
        assert_eq!(
            task_item_asset(
                &world,
                &crate::assets::GridAssets::default(),
                &item_params(1, 2)
            ),
            None
        );
    }

    #[test]
    fn the_covenant_resolves_against_the_fixture() {
        let fixtures = UdpAssetFixtures::new().with_estate_covenant(b"covenant".to_vec());
        assert_eq!(
            fixtures.resolve_estate_transfer(&estate_params(sl_wire::ESTATE_ASSET_COVENANT)),
            Some(b"covenant".as_slice())
        );
        assert_eq!(fixtures.resolve_estate_transfer(&estate_params(7)), None);
        assert_eq!(
            UdpAssetFixtures::new()
                .resolve_estate_transfer(&estate_params(sl_wire::ESTATE_ASSET_COVENANT)),
            None
        );
    }

    #[test]
    fn flat_terrain_raw_has_the_raw32_layout() {
        let raw = flat_terrain_raw(25);
        assert_eq!(
            raw.len(),
            TERRAIN_RAW_SIDE * TERRAIN_RAW_SIDE * TERRAIN_RAW_CHANNELS
        );
        let last = raw.as_chunks::<TERRAIN_RAW_CHANNELS>().0.last().copied();
        assert_eq!(last, Some([25, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    }
}
