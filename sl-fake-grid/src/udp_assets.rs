//! Fixtures for the legacy UDP asset paths: named `Xfer` files, UDP
//! `Transfer` sources (task-item assets, the estate covenant), and the estate
//! terrain RAW heightmap.
//!
//! An object's task inventory used to live here too. It does not any more: a
//! contents *serial* is only meaningful if the store that answers it is the
//! store a write advances, so the listings moved to the region's own world
//! ([`SceneFixtures::task_inventories`](crate::SceneFixtures::task_inventories))
//! where the writes land. What is left here is genuinely fixture — bytes
//! stated up front and served back unchanged.
//!
//! `SimSession` implements the server half of every one of these flows but
//! keeps no content of its own; the driver answers the corresponding
//! [`ServerEvent`]s from a per-session copy of these fixtures (the
//! crate-private `answer_from_fixtures`). Everything here is plain data so
//! a [`Scenario`](crate::Scenario) stays a value that can be cloned per
//! session and per region.

use std::collections::HashMap;
use std::time::Instant;

use sl_proto::{ServerEvent, SimSession, TransferRequestSource, TransferStatus, Uuid};
use sl_types::key::{InventoryKey, ObjectKey};

/// The scripted content behind the legacy UDP asset paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UdpAssetFixtures {
    /// Named files served over `RequestXfer`. Registered on every fresh
    /// session and re-armed after each serve (a `SimSession` registration
    /// is consumed by the request that names it).
    pub xfer_files: HashMap<String, Vec<u8>>,
    /// Task-item asset bodies by `(task, item)`, answered on a
    /// task-inventory-item `TransferRequest`. A miss is refused with
    /// [`TransferStatus::UnknownSource`].
    pub task_item_assets: HashMap<(ObjectKey, InventoryKey), Vec<u8>>,
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

    /// Adds the asset body of a task-inventory item.
    #[must_use]
    pub fn with_task_item_asset(
        mut self,
        task: ObjectKey,
        item: InventoryKey,
        data: impl Into<Vec<u8>>,
    ) -> Self {
        let _prev = self.task_item_assets.insert((task, item), data.into());
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

    /// Resolves a UDP `Transfer` request source to the bytes to serve, or
    /// `None` for a source these fixtures do not hold.
    #[must_use]
    pub fn resolve_transfer(&self, source: &TransferRequestSource) -> Option<&[u8]> {
        match source {
            TransferRequestSource::TaskInventoryItem(params) => self
                .task_item_assets
                .get(&(
                    ObjectKey::from(params.task_id),
                    InventoryKey::from(params.item_id),
                ))
                .map(Vec::as_slice),
            TransferRequestSource::Estate(params) => {
                if params.estate_asset_type == sl_wire::ESTATE_ASSET_COVENANT {
                    self.estate_covenant.as_deref()
                } else {
                    None
                }
            }
        }
    }
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
            let result = match fixtures.resolve_transfer(source) {
                Some(data) => sim.send_transfer_asset(*transfer_id, data, now),
                None => {
                    tracing::debug!("no transfer fixture for {source:?}; refusing");
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
    use sl_wire::{TransferSourceParamsEstate, TransferSourceParamsInvItem};

    use super::*;

    /// A task-item request for the given task/item pair.
    fn item_source(task: u128, item: u128) -> TransferRequestSource {
        TransferRequestSource::TaskInventoryItem(TransferSourceParamsInvItem {
            agent_id: uuid::Uuid::nil(),
            session_id: uuid::Uuid::nil(),
            owner_id: uuid::Uuid::nil(),
            task_id: uuid::Uuid::from_u128(task),
            item_id: uuid::Uuid::from_u128(item),
            asset_id: uuid::Uuid::nil(),
            asset_type: 10,
        })
    }

    /// An estate request of the given estate asset type.
    fn estate_source(estate_asset_type: i32) -> TransferRequestSource {
        TransferRequestSource::Estate(TransferSourceParamsEstate {
            agent_id: uuid::Uuid::nil(),
            session_id: uuid::Uuid::nil(),
            estate_asset_type,
        })
    }

    #[test]
    fn transfer_sources_resolve_against_the_maps() {
        let fixtures = UdpAssetFixtures::new()
            .with_task_item_asset(
                ObjectKey::from(uuid::Uuid::from_u128(1)),
                InventoryKey::from(uuid::Uuid::from_u128(2)),
                b"script body".to_vec(),
            )
            .with_estate_covenant(b"covenant".to_vec());
        assert_eq!(
            fixtures.resolve_transfer(&item_source(1, 2)),
            Some(b"script body".as_slice())
        );
        assert_eq!(fixtures.resolve_transfer(&item_source(1, 3)), None);
        assert_eq!(
            fixtures.resolve_transfer(&estate_source(sl_wire::ESTATE_ASSET_COVENANT)),
            Some(b"covenant".as_slice())
        );
        assert_eq!(fixtures.resolve_transfer(&estate_source(7)), None);
        assert_eq!(
            UdpAssetFixtures::new()
                .resolve_transfer(&estate_source(sl_wire::ESTATE_ASSET_COVENANT)),
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
