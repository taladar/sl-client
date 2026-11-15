//! OpenSim's land, terrain, wind and environment writers
//! (`server-lsl-lib-ossl-land-writers`): the host half, making the changes
//! the library checked — the ground's heights and textures, the water line
//! and the sun, the wind models' parameters, environments and LightShare's
//! view of them, notices and restarts, parcel joins, cuts and details — and
//! telling the region's avatars what OpenSim tells them. Which rule decides
//! whether a script may is the runtime's (`library::ossl_land`).

use sl_lsl_runtime::vm::land::{
    DayCycle, EnvironmentChange, EnvironmentScope, LightShare, ParcelDetails, RegionEdit,
    WindParamError,
};
use sl_proto::AssetSource as _;
use sl_proto::{
    AgentKey, EnvironmentAsset, EnvironmentSettings, GroupKey, LandArea, LandingType, OwnerKey,
    ParcelInfo, RegionIdentity, RegionLocalParcelId,
};
use sl_types::environment::{CloudPosDensity, Color, ColorAlpha, Glow};
use sl_types::lsl::{Rotation, Vector};
use sl_types::map::{Direction, RegionCoordinates};

use super::{Delivery, Effect, RegionHost, narrow};
use crate::estate::Notice;
use crate::world::RegionChange;

/// The blocks along each edge of a region's parcel bitmap.
const BLOCKS: usize = 64;

/// The edge of one parcel block, in metres.
const BLOCK_METRES: f32 = 4.0;

/// `ParcelFlags.ShowDirectory`, which a cut-off parcel loses.
const PARCEL_FLAG_SHOW_DIRECTORY: u32 = 0x400;

/// How many statistics OpenSim keeps (`StatsIndex.ArraySize`).
const STATS: usize = 48;

/// `(x, y, z)` as an LSL vector.
const fn vector(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

/// `(x, y, z, s)` as an LSL rotation.
const fn rotation(x: f32, y: f32, z: f32, s: f32) -> Rotation {
    Rotation { x, y, z, s }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// The region's identity, as its scripts' records are stamped.
    fn identity(&self) -> Option<RegionIdentity> {
        self.core
            .region(self.index)
            .map(|entry| entry.script_identity(self.core))
    }

    /// Tells the region's sessions of `changes` after the tick, in order with
    /// the tick's other effects.
    fn tell(&mut self, changes: Vec<RegionChange>) {
        if !changes.is_empty() {
            self.effects.push(Effect::Tell(changes));
        }
    }

    /// Makes `edit` to the region.
    pub(super) fn region_edit(&mut self, edit: &RegionEdit) -> bool {
        let (Some(entry), Some(identity)) = (self.core.region(self.index), self.identity()) else {
            return false;
        };
        match edit {
            RegionEdit::TerrainHeight { x, y, height } => {
                self.world
                    .edits
                    .set_height(&entry.config.terrain, *x, *y, *height);
            }
            RegionEdit::TerrainFlush => self.world.edits.flush = true,
            // OpenSim's estate module saves the corner and sends everybody
            // the region's record, then a handshake carrying the textures.
            RegionEdit::TerrainTexture { corner, texture } => {
                let Ok(texture) = texture.parse() else {
                    return false;
                };
                let composition = self.world.terrain_composition_mut(&identity);
                let Some(slot) = composition.detail_textures.get_mut(*corner) else {
                    return false;
                };
                *slot = texture;
                let limits = self.world.limits(&identity);
                let composition = self.world.terrain_composition(&identity);
                self.tell(vec![
                    RegionChange::RegionConfigured(Box::new(limits)),
                    RegionChange::TerrainRetextured(Box::new(composition)),
                ]);
            }
            // A handshake alone: OpenSim's region-info push is commented out.
            RegionEdit::TerrainTextureHeights { corner, low, high } => {
                let composition = self.world.terrain_composition_mut(&identity);
                let (Some(start), Some(range)) = (
                    composition.start_heights.get_mut(*corner),
                    composition.height_ranges.get_mut(*corner),
                ) else {
                    return false;
                };
                *start = *low;
                *range = *high;
                let composition = self.world.terrain_composition(&identity);
                self.tell(vec![RegionChange::TerrainRetextured(Box::new(composition))]);
            }
            RegionEdit::WaterHeight(height) => {
                self.world.limits_mut(&identity).water_height = *height;
                let limits = self.world.limits(&identity);
                self.tell(vec![RegionChange::RegionConfigured(Box::new(limits))]);
            }
            // Kept and told to nobody: the sun update OpenSim raises has no
            // listener since its day cycles came.
            RegionEdit::Sun {
                estate_sun,
                fixed,
                hour,
            } => {
                let limits = self.world.limits_mut(&identity);
                limits.use_estate_sun = *estate_sun;
                limits.sun_hour = *hour;
                let bits = limits.flags().bits();
                let sun = sl_proto::RegionFlags::SUN_FIXED.bits();
                let flags =
                    sl_proto::RegionFlags::from_bits(if *fixed { bits | sun } else { bits & !sun });
                crate::estate::set_flags(limits, flags);
            }
            RegionEdit::WindParam {
                plugin,
                param,
                value,
            } => self.world.edits.wind.set(plugin, param, *value),
            RegionEdit::Notice { agent: None, text } => {
                self.tell(vec![RegionChange::Notice(Box::new(Notice::Alert {
                    message: text.clone(),
                    info: Vec::new(),
                }))]);
            }
            RegionEdit::Notice {
                agent: Some(agent),
                text,
            } => {
                let Some(agent) = Self::agent_named(agent) else {
                    return false;
                };
                self.deliveries.push(Delivery::Alert {
                    agent,
                    text: text.clone(),
                    modal: false,
                });
            }
            RegionEdit::Restart { alerts, notice } => {
                self.world.edits.restart = Some(alerts.iter().skip(1).copied().collect());
                self.tell(vec![RegionChange::Notice(Box::new(Notice::Alert {
                    message: notice.clone(),
                    info: Vec::new(),
                }))]);
            }
            RegionEdit::AbortRestart => {
                if self.world.edits.restart.take().is_some() {
                    self.tell(vec![RegionChange::Notice(Box::new(Notice::AgentAlert(
                        "Region restart has been aborted\n".to_owned(),
                    )))]);
                }
            }
            RegionEdit::LightShare(scene) => {
                let current = self.region_settings();
                let settings = from_lightshare(scene, &current);
                self.store_region_environment(Some(settings));
            }
            RegionEdit::ClearLightShare => self.store_region_environment(None),
        }
        true
    }

    /// The parameter `param` of the wind model `plugin`.
    pub(super) fn wind_param_of(&self, plugin: &str, param: &str) -> Result<f32, WindParamError> {
        self.world.edits.wind.get(plugin, param)
    }

    /// OpenSim's statistics of the region, in its `StatsIndex` order: the
    /// frame rate, the avatars, the prims and the scripts the fake grid
    /// knows, the physics step OpenSim states as 20 ms, and 0 for the times
    /// it does not measure.
    pub(super) fn opensim_stats(&self) -> Vec<f32> {
        let step = self
            .core
            .region(self.index)
            .map_or(0.0, |region| region.config.heartbeat.step.as_secs_f64());
        let fps = if step > 0.0 { narrow(1.0 / step) } else { 0.0 };
        let count = |value: usize| narrow(f64::from(u32::try_from(value).unwrap_or(u32::MAX)));
        let prims = self.world.objects.len();
        let scripts = self
            .scripts_read
            .values()
            .filter(|(running, _memory)| *running)
            .count();
        let mut stats = vec![0.0_f32; STATS];
        for (index, value) in [
            (0, 1.0),
            (1, fps),
            (2, fps),
            (4, count(self.present.len())),
            (5, count(self.children.len())),
            (6, count(prims)),
            (8, narrow(step * 1000.0)),
            (19, count(scripts)),
            (22, 20.0),
            (42, 1.0),
            (44, count(prims)),
            (47, count(self.world.npcs.len())),
        ] {
            if let Some(slot) = stats.get_mut(index) {
                *slot = value;
            }
        }
        stats
    }

    /// The region's own environment, if it keeps one: what a script, a
    /// viewer or the scene set, `None` where it was dropped or never set.
    fn stored_region_environment(&self) -> Option<EnvironmentSettings> {
        match &self.world.edits.environments.region {
            Some(edited) => edited.settings().cloned(),
            None => self
                .core
                .region(self.index)
                .and_then(|region| region.config.environment.clone()),
        }
    }

    /// The environment the region serves: its own, or the grid's default day.
    fn region_settings(&self) -> EnvironmentSettings {
        let region_id = self
            .core
            .region(self.index)
            .map_or_else(uuid::Uuid::nil, |region| region.region_id);
        let mut settings = self
            .stored_region_environment()
            .unwrap_or_else(|| self.core.stock_environment(region_id));
        if settings.region_id.is_nil() {
            settings.region_id = region_id;
        }
        settings
    }

    /// The region's environment in LightShare's terms: OpenSim's defaults
    /// where it keeps none of its own.
    pub(super) fn lightshare_of(&self) -> LightShare {
        self.stored_region_environment()
            .map_or_else(LightShare::default, |settings| to_lightshare(&settings))
    }

    /// Stores `settings` as the region's environment — `None` drops it, and
    /// the grid's default day shows — with the next version, and refreshes
    /// every viewer as OpenSim refreshes an environment-aware one: a
    /// `RegionInfo`.
    fn store_region_environment(&mut self, settings: Option<EnvironmentSettings>) {
        let Some(identity) = self.identity() else {
            return;
        };
        let version = self.region_settings().env_version.saturating_add(1);
        let settings = settings.map(|mut settings| {
            settings.parcel_id = -1;
            settings.env_version = version;
            settings
        });
        self.world.edits.keep_environment(-1, settings.clone());
        let served = settings.unwrap_or_else(|| self.region_settings());
        let limits = self.world.limits(&identity);
        self.tell(vec![RegionChange::EnvironmentChanged {
            parcel_id: -1,
            settings: Some(Box::new(served)),
            told: Some(Box::new(limits)),
        }]);
    }

    /// The bytes of the asset keyed `asset`, if the grid has it.
    fn asset_bytes(&self, asset: &str) -> Option<Vec<u8>> {
        let key = sl_proto::AssetKey::from(uuid::Uuid::parse_str(asset).ok()?);
        self.core.assets.read().get(key).map(<[u8]>::to_vec)
    }

    /// What the asset keyed `asset` is to a day-cycle reader: a settings
    /// asset is LLSD behind its header, which no other kind of asset is.
    pub(super) fn day_cycle_kind(&self, asset: &str) -> DayCycle {
        let Some(bytes) = self.asset_bytes(asset) else {
            return DayCycle::Missing;
        };
        if !bytes.starts_with(b"<? llsd/") && !bytes.starts_with(b"<?xml") {
            return DayCycle::Missing;
        }
        match sl_proto::environment_asset_from_bytes("", &bytes) {
            Some(EnvironmentAsset::DayCycle(_)) => DayCycle::DayCycle,
            Some(EnvironmentAsset::Sky(_) | EnvironmentAsset::Water(_)) => DayCycle::NotDayCycle,
            None => DayCycle::Unreadable,
        }
    }

    /// The day cycle of the settings asset keyed `asset`.
    fn read_day_cycle(&self, asset: &str) -> Option<sl_proto::DayCycle> {
        let bytes = self.asset_bytes(asset)?;
        match sl_proto::environment_asset_from_bytes("", &bytes)? {
            EnvironmentAsset::DayCycle(cycle) => Some(*cycle),
            EnvironmentAsset::Sky(_) | EnvironmentAsset::Water(_) => None,
        }
    }

    /// Makes `change` to the environment `scope` names. A viewer blends into
    /// it as OpenSim's environment-aware viewers do, from the refresh alone,
    /// so the transition a script asked for travels nowhere.
    pub(super) fn environment_change(
        &mut self,
        scope: &EnvironmentScope,
        change: &EnvironmentChange,
    ) -> bool {
        let Some(identity) = self.identity() else {
            return false;
        };
        match (scope, change) {
            (
                EnvironmentScope::Region,
                EnvironmentChange::Set {
                    day_cycle,
                    day_length,
                    day_offset,
                    altitudes,
                },
            ) => {
                let mut settings = self.region_settings();
                if let Some(asset) = day_cycle {
                    let Some(cycle) = self.read_day_cycle(asset) else {
                        return false;
                    };
                    settings.day_cycle = cycle;
                    settings.day_asset = None;
                }
                if let Some(length) = day_length {
                    settings.day_length = *length;
                }
                if let Some(offset) = day_offset {
                    settings.day_offset = *offset;
                }
                if let Some(altitudes) = altitudes {
                    settings.track_altitudes = *altitudes;
                }
                self.store_region_environment(Some(settings));
            }
            (EnvironmentScope::Region, EnvironmentChange::Reset) => {
                self.store_region_environment(None);
            }
            (EnvironmentScope::Parcel(at), change) => {
                return self.parcel_environment_change(at, change);
            }
            (EnvironmentScope::Agent(agent), change) => {
                let Some(agent) = Self::agent_named(agent) else {
                    return false;
                };
                let settings = match change {
                    EnvironmentChange::Set { day_cycle, .. } => {
                        let mut settings = self.region_settings();
                        if let Some(asset) = day_cycle {
                            let Some(cycle) = self.read_day_cycle(asset) else {
                                return false;
                            };
                            settings.day_cycle = cycle;
                            settings.day_asset = None;
                        }
                        let _previous = self
                            .world
                            .edits
                            .environments
                            .agents
                            .insert(agent, settings.clone());
                        Some(settings)
                    }
                    EnvironmentChange::Reset => {
                        let _dropped = self.world.edits.environments.agents.remove(&agent);
                        None
                    }
                };
                let limits = self.world.limits(&identity);
                self.tell(vec![RegionChange::AgentEnvironment {
                    agent,
                    settings: settings.map(Box::new),
                    told: Box::new(limits),
                }]);
            }
        }
        true
    }

    /// Makes `change` to the environment of the parcel at `at`: its record's
    /// environment version moves, and the avatars on it are sent the record
    /// (`SendLandUpdateToAvatarsOverMe`).
    fn parcel_environment_change(&mut self, at: &Vector, change: &EnvironmentChange) -> bool {
        let Some(parcel) = self.world.parcel_at(at.x, at.y).cloned() else {
            return false;
        };
        let id = parcel.local_id.0;
        let settings = match change {
            EnvironmentChange::Set { day_cycle, .. } => {
                let mut settings = self
                    .world
                    .edits
                    .environments
                    .parcels
                    .get(&id)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| self.region_settings());
                if let Some(asset) = day_cycle {
                    let Some(cycle) = self.read_day_cycle(asset) else {
                        return false;
                    };
                    settings.day_cycle = cycle;
                    settings.day_asset = None;
                }
                settings.parcel_id = id;
                settings.env_version = parcel.parcel_environment_version.saturating_add(1);
                Some(settings)
            }
            EnvironmentChange::Reset => {
                if parcel.parcel_environment_version == -1 {
                    return true;
                }
                None
            }
        };
        let version = settings
            .as_ref()
            .map_or(-1, |settings| settings.env_version);
        self.world.edits.keep_environment(id, settings.clone());
        let Some(record) = self.world.parcel_mut(parcel.local_id) else {
            return false;
        };
        record.parcel_environment_version = version;
        let record = record.clone();
        self.tell(vec![
            RegionChange::EnvironmentChanged {
                parcel_id: id,
                settings: settings.map(Box::new),
                told: None,
            },
            RegionChange::ParcelChanged(Box::new(record)),
        ]);
        true
    }

    /// `osParcelJoin`: the parcels at `points` merged into the first, which
    /// takes their blocks and their dwell; every avatar on it is sent its
    /// record and every viewer the overlay.
    pub(super) fn join_parcels(&mut self, points: &[(i32, i32)]) -> bool {
        let ids: Vec<RegionLocalParcelId> = points
            .iter()
            .filter_map(|(x, y)| {
                let metres = |value: i32| i16::try_from(value).map_or(-1.0, f32::from);
                self.world
                    .parcel_at(metres(*x), metres(*y))
                    .map(|parcel| parcel.local_id)
            })
            .collect();
        let Some((master, others)) = ids.split_first() else {
            return false;
        };
        let mut blocks = Vec::new();
        let mut dwell = 0.0;
        for other in others {
            let Some(index) = self
                .world
                .parcels
                .iter()
                .position(|parcel| parcel.local_id == *other)
            else {
                continue;
            };
            let slave = self.world.parcels.remove(index);
            blocks.push(slave.bitmap);
            if let Some(listing) = self
                .world
                .listings
                .iter()
                .position(|listing| listing.local_id == *other)
            {
                dwell += self.world.listings.remove(listing).dwell;
            }
        }
        if let Some(listing) = self
            .world
            .listings
            .iter_mut()
            .find(|listing| listing.local_id == *master)
        {
            listing.dwell += dwell;
        }
        let Some(parcel) = self.world.parcel_mut(*master) else {
            return false;
        };
        for bitmap in blocks {
            for (into, from) in parcel.bitmap.iter_mut().zip(bitmap) {
                *into |= from;
            }
        }
        recount(parcel);
        let record = parcel.clone();
        self.tell(vec![
            RegionChange::ParcelChanged(Box::new(record)),
            RegionChange::OverlayChanged,
        ]);
        true
    }

    /// `osParcelSubdivide`: the whole blocks of the box from `start` to `end`
    /// cut off the parcel `local_id` as a copy of it with the next local id,
    /// a new key, no dwell, no search listing and no landing point; a landing
    /// point the parcel lost goes too. Both are sent to the avatars on them
    /// and every viewer the overlay.
    pub(super) fn subdivide_parcel(
        &mut self,
        local_id: RegionLocalParcelId,
        (start, end): ((i32, i32), (i32, i32)),
    ) -> bool {
        let Some(original) = self.world.parcel_by_local_id(local_id).cloned() else {
            return false;
        };
        let block = |metre: i32| {
            usize::try_from(metre.div_euclid(4))
                .unwrap_or(0)
                .min(BLOCKS)
        };
        let (west, south) = (block(start.0), block(start.1));
        let (east, north) = (block(end.0), block(end.1));
        let next = self
            .world
            .parcels
            .iter()
            .map(|parcel| parcel.local_id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let mut cut = original.clone();
        cut.local_id = RegionLocalParcelId(next);
        cut.bitmap = vec![0; BLOCKS * BLOCKS / 8];
        for block_y in south..north {
            for block_x in west..east {
                set_block(&mut cut.bitmap, block_x, block_y, true);
            }
        }
        cut.raw_parcel_flags &= !PARCEL_FLAG_SHOW_DIRECTORY;
        cut.landing_type = LandingType::Anywhere;
        cut.user_location = RegionCoordinates::new(0.0, 0.0, 0.0);
        cut.user_look_at = Direction::new(0.0, 0.0, 0.0);
        recount(&mut cut);
        let Some(parcel) = self.world.parcel_mut(local_id) else {
            return false;
        };
        for block_y in south..north {
            for block_x in west..east {
                set_block(&mut parcel.bitmap, block_x, block_y, false);
            }
        }
        recount(parcel);
        if parcel.landing_type == LandingType::LandingPoint {
            let (x, y) = (
                parcel.user_location.x().trunc(),
                parcel.user_location.y().trunc(),
            );
            if !parcel.contains_point(x, y) {
                parcel.landing_type = LandingType::Anywhere;
                parcel.user_location = RegionCoordinates::new(0.0, 0.0, 0.0);
                parcel.user_look_at = Direction::new(0.0, 0.0, 0.0);
            }
        }
        let original = parcel.clone();
        let key = sl_proto::ParcelKey::from(self.world.script_instances.mint_id());
        self.world.listings.push(crate::world::ParcelListing {
            local_id: cut.local_id,
            parcel_id: key,
            dwell: 0.0,
        });
        self.world.parcels.push(cut.clone());
        self.tell(vec![
            RegionChange::ParcelChanged(Box::new(cut)),
            RegionChange::ParcelChanged(Box::new(original)),
            RegionChange::OverlayChanged,
        ]);
        true
    }

    /// `osSetParcelDetails`: the parcel `local_id`'s fields set, kept without
    /// a word to its avatars, as OpenSim's `UpdateLandObject` keeps them;
    /// whether those outside see its avatars changes every viewer's overlay.
    pub(super) fn set_parcel_details(
        &mut self,
        local_id: RegionLocalParcelId,
        details: &ParcelDetails,
    ) -> bool {
        let Some(parcel) = self.world.parcel_mut(local_id) else {
            return false;
        };
        if let Some(name) = &details.name {
            parcel.name.clone_from(name);
        }
        if let Some(description) = &details.description {
            parcel.description.clone_from(description);
        }
        if let Some(owner) = details
            .owner
            .as_deref()
            .and_then(|owner| uuid::Uuid::parse_str(owner).ok())
        {
            parcel.owner = OwnerKey::Agent(AgentKey::from(owner));
        }
        if let Some(group) = details
            .group
            .as_deref()
            .and_then(|group| uuid::Uuid::parse_str(group).ok())
        {
            parcel.group = (!group.is_nil()).then(|| GroupKey::from(group));
        }
        if let Some(date) = details.claim_date {
            parcel.claim_date = date;
        }
        if let Some(see) = details.see_avatars {
            parcel.see_avs = Some(see);
        }
        if let Some(heard) = details.any_sounds {
            parcel.any_av_sounds = Some(heard);
        }
        if let Some(heard) = details.group_sounds {
            parcel.group_av_sounds = Some(heard);
        }
        if details.see_avatars.is_some() {
            self.tell(vec![RegionChange::OverlayChanged]);
        }
        true
    }
}

/// Sets or clears the block `(x, y)` of a parcel bitmap.
fn set_block(bitmap: &mut [u8], x: usize, y: usize, on: bool) {
    let bit = y.saturating_mul(BLOCKS).saturating_add(x);
    if let Some(byte) = bitmap.get_mut(bit / 8) {
        let mask = 1_u8 << (bit % 8);
        if on {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

/// Recounts a parcel's area and bounding box from its bitmap, as OpenSim's
/// `ForceUpdateLandInfo` does: a parcel holding no block has no area and a
/// box at the origin.
fn recount(parcel: &mut ParcelInfo) {
    let mut blocks = 0_u32;
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for y in 0..BLOCKS {
        for x in 0..BLOCKS {
            let bit = y.saturating_mul(BLOCKS).saturating_add(x);
            let set = parcel
                .bitmap
                .get(bit / 8)
                .is_some_and(|byte| byte & (1 << (bit % 8)) != 0);
            if !set {
                continue;
            }
            blocks = blocks.saturating_add(1);
            bounds = Some(bounds.map_or((x, y, x, y), |(west, south, east, north)| {
                (west.min(x), south.min(y), east.max(x), north.max(y))
            }));
        }
    }
    let edge = |block: usize| f32::from(u16::try_from(block).unwrap_or(u16::MAX)) * BLOCK_METRES;
    let (west, south, east, north) = bounds.map_or((0.0, 0.0, 0.0, 0.0), |(w, s, e, n)| {
        (
            edge(w),
            edge(s),
            edge(e.saturating_add(1)),
            edge(n.saturating_add(1)),
        )
    });
    parcel.aabb_min = RegionCoordinates::new(west, south, 0.0);
    parcel.aabb_max = RegionCoordinates::new(east, north, 0.0);
    parcel.area = LandArea(blocks.saturating_mul(16));
}

/// `Quaternion.Normalize` then OpenSim's `Xrot`: the way the rotation turns
/// the x axis.
fn x_axis(turn: &Rotation) -> (f32, f32, f32) {
    let length = (turn.x * turn.x + turn.y * turn.y + turn.z * turn.z + turn.s * turn.s).sqrt();
    let (x, y, z, w) = if length > 0.0 {
        (
            turn.x / length,
            turn.y / length,
            turn.z / length,
            turn.s / length,
        )
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    (
        2.0 * (x * x + w * w) - 1.0,
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
    )
}

/// OpenSim's `convertToAngles`: the sun's altitude and the east angle, each
/// in `0..2π`, of a sky turned `sun`.
fn to_angles(sun: &Rotation) -> (f32, f32) {
    let (x, y, z) = x_axis(sun);
    let length = (x * x + y * y + z * z).sqrt();
    let (x, y, z) = if length > 0.0 {
        (x / length, y / length, z / length)
    } else {
        (x, y, z)
    };
    let tau = core::f32::consts::TAU;
    let mut sun_angle = z.asin();
    let mut east_angle = -y.atan2(x);
    if east_angle.abs() < 1e-6 {
        east_angle = 0.0;
    } else if east_angle < 0.0 {
        east_angle += tau;
    }
    if sun_angle.abs() < 1e-6 {
        sun_angle = 0.0;
    } else if sun_angle < 0.0 {
        sun_angle += tau;
    }
    (sun_angle, east_angle)
}

/// OpenSim's `AzAlToRot`: a turn to azimuth `az` and altitude `al`.
fn az_al_to_rotation(az: f32, al: f32) -> Rotation {
    let tau = core::f32::consts::TAU;
    let near = |value: f32, to: f32| (value - to).abs() <= 1e-3;
    if near(al, 0.0) || near(al.abs(), tau) {
        let (sin, cos) = (az * 0.5).sin_cos();
        return rotation(0.0, 0.0, sin, cos);
    }
    if near(az, 0.0) || near(az.abs(), tau) {
        let (sin, cos) = (al * 0.5).sin_cos();
        return rotation(0.0, -sin, 0.0, cos);
    }
    let (sz, cz) = (az * 0.5).sin_cos();
    let (sl, cl) = (al * 0.5).sin_cos();
    let (x, y, z, s) = (sl * sz, -sl * cz, cl * sz, cl * cz);
    let length = (x * x + y * y + z * z + s * s).sqrt();
    if length > 0.0 {
        rotation(x / length, y / length, z / length, s / length)
    } else {
        rotation(0.0, 0.0, 0.0, 1.0)
    }
}

/// OpenSim's `ViewerEnvironment.ToLightShare`: the first water frame and the
/// first ground-level sky frame of `settings`' day cycle in LightShare's
/// terms, its defaults for a cycle missing either.
fn to_lightshare(settings: &EnvironmentSettings) -> LightShare {
    let mut scene = LightShare::default();
    let cycle = &settings.day_cycle;
    if let Some(water) = cycle
        .water_track
        .first()
        .and_then(|frame| cycle.water_frames.get(&frame.name))
    {
        let colour = &water.water_fog_color;
        scene.water_color = vector(
            colour.red() * 256.0,
            colour.green() * 256.0,
            colour.blue() * 256.0,
        );
        // OpenSim's inverse of its own `2 ^ exponent` is a square root.
        scene.water_fog_density_exponent = water.water_fog_density.sqrt();
        scene.underwater_fog_modifier = water.underwater_fog_mod;
        scene.reflection_wavelet_scale = vector(
            water.normal_scale.x(),
            water.normal_scale.y(),
            water.normal_scale.z(),
        );
        scene.fresnel_scale = water.fresnel_scale;
        scene.fresnel_offset = water.fresnel_offset;
        scene.refract_scale_above = water.scale_above;
        scene.refract_scale_below = water.scale_below;
        scene.blur_multiplier = water.blur_multiplier;
        scene.little_wave_direction = (water.wave1_direction[0], water.wave1_direction[1]);
        scene.big_wave_direction = (water.wave2_direction[0], water.wave2_direction[1]);
        scene.normal_map_texture = water.normal_map.map_or_else(
            || uuid::Uuid::nil().to_string(),
            |texture| texture.uuid().to_string(),
        );
    }
    if let Some(sky) = cycle
        .sky_tracks
        .first()
        .and_then(|track| track.first())
        .and_then(|frame| cycle.sky_frames.get(&frame.name))
    {
        let (sun_angle, east_angle) = to_angles(&sky.sun_rotation);
        let half_turns = 0.5 / core::f32::consts::PI;
        scene.sun_moon_position = sun_angle * half_turns;
        scene.east_angle = east_angle * half_turns;
        let sun = &sky.sunlight_color;
        scene.sun_moon_color = rotation(
            sun.red() / 3.0,
            sun.green() / 3.0,
            sun.blue() / 3.0,
            sun.alpha() / 3.0,
        );
        let quarter = |colour: &Color, by: f32| {
            rotation(
                colour.red() / by,
                colour.green() / by,
                colour.blue() / by,
                1.0,
            )
        };
        scene.ambient = quarter(&sky.ambient, 3.0);
        scene.horizon = quarter(&sky.blue_horizon, 2.0);
        scene.blue_density = quarter(&sky.blue_density, 2.0);
        scene.haze_horizon = sky.haze_horizon;
        scene.haze_density = sky.haze_density;
        scene.cloud_coverage = sky.cloud_shadow;
        scene.density_multiplier = 1000.0 * sky.density_multiplier;
        scene.distance_multiplier = sky.distance_multiplier;
        scene.max_altitude = altitude(sky.max_y);
        scene.cloud_color = quarter(&sky.cloud_color, 1.0);
        let density = |cloud: &CloudPosDensity| {
            vector(cloud.position_x(), cloud.position_y(), cloud.density())
        };
        scene.cloud_xy_density = density(&sky.cloud_pos_density1);
        scene.cloud_detail_xy_density = density(&sky.cloud_pos_density2);
        scene.cloud_scale = sky.cloud_scale;
        scene.scene_gamma = sky.gamma;
        scene.sun_glow_size = (2.0 - sky.glow.size()) / 20.0;
        scene.sun_glow_focus = -sky.glow.focus() / 5.0;
        scene.cloud_scroll_x = sky.cloud_scroll_rate[0];
        scene.cloud_scroll_y = sky.cloud_scroll_rate[1];
        scene.cloud_scroll_x_lock = scene.cloud_scroll_x == 0.0;
        scene.cloud_scroll_y_lock = scene.cloud_scroll_y == 0.0;
        scene.star_brightness = sky.star_brightness / 250.0;
    }
    scene
}

/// `(ushort)` of a sky's height: truncated into a `u16`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into the u16 range before the cast"
)]
fn altitude(max_y: f32) -> u16 {
    max_y.trunc().clamp(0.0, f32::from(u16::MAX)) as u16
}

/// OpenSim's `ViewerEnvironment.FromLightShare`: a fresh environment — the
/// stock day's length, offset and altitudes — holding one water frame and one
/// sky frame made of `scene`, whatever LightShare does not name taken from the
/// stock frames. `current` lends the region its key.
fn from_lightshare(scene: &LightShare, current: &EnvironmentSettings) -> EnvironmentSettings {
    let mut settings = EnvironmentSettings::default_region();
    settings.region_id = current.region_id;
    let stock = settings.day_cycle.clone();
    let (Some(mut water), Some(mut sky)) = (
        stock
            .water_track
            .first()
            .and_then(|frame| stock.water_frames.get(&frame.name))
            .cloned(),
        stock
            .sky_tracks
            .first()
            .and_then(|track| track.first())
            .and_then(|frame| stock.sky_frames.get(&frame.name))
            .cloned(),
    ) else {
        return settings;
    };
    "LightshareWater".clone_into(&mut water.name);
    water.water_fog_color = Color::new(
        scene.water_color.x / 256.0,
        scene.water_color.y / 256.0,
        scene.water_color.z / 256.0,
    );
    water.water_fog_density = 2.0_f32.powf(scene.water_fog_density_exponent);
    water.underwater_fog_mod = scene.underwater_fog_modifier;
    let wavelets = &scene.reflection_wavelet_scale;
    water.normal_scale = sl_types::map::Scale::new(wavelets.x, wavelets.y, wavelets.z);
    water.fresnel_scale = scene.fresnel_scale;
    water.fresnel_offset = scene.fresnel_offset;
    water.scale_above = scene.refract_scale_above;
    water.scale_below = scene.refract_scale_below;
    water.blur_multiplier = scene.blur_multiplier;
    water.wave1_direction = [scene.little_wave_direction.0, scene.little_wave_direction.1];
    water.wave2_direction = [scene.big_wave_direction.0, scene.big_wave_direction.1];
    water.normal_map = uuid::Uuid::parse_str(&scene.normal_map_texture)
        .ok()
        .filter(|texture| !texture.is_nil())
        .map(sl_types::key::TextureKey::from);
    "LightshareSky".clone_into(&mut sky.name);
    let tau = core::f32::consts::TAU;
    let (az, al) = (-(tau * scene.east_angle), tau * scene.sun_moon_position);
    sky.sun_rotation = az_al_to_rotation(az, al);
    sky.moon_rotation = az_al_to_rotation(az, al + core::f32::consts::PI);
    let sun = &scene.sun_moon_color;
    sky.sunlight_color = ColorAlpha::new(sun.x * 3.0, sun.y * 3.0, sun.z * 3.0, sun.s * 3.0);
    let times =
        |colour: &Rotation, by: f32| Color::new(colour.x * by, colour.y * by, colour.z * by);
    sky.ambient = times(&scene.ambient, 3.0);
    sky.blue_horizon = times(&scene.horizon, 2.0);
    sky.blue_density = times(&scene.blue_density, 2.0);
    sky.haze_horizon = scene.haze_horizon;
    sky.haze_density = scene.haze_density;
    sky.cloud_shadow = scene.cloud_coverage;
    sky.density_multiplier = scene.density_multiplier / 1000.0;
    sky.distance_multiplier = scene.distance_multiplier;
    sky.max_y = f32::from(scene.max_altitude);
    sky.cloud_color = times(&scene.cloud_color, 1.0);
    let density = |cloud: &Vector| CloudPosDensity::new(cloud.x, cloud.y, cloud.z);
    sky.cloud_pos_density1 = density(&scene.cloud_xy_density);
    sky.cloud_pos_density2 = density(&scene.cloud_detail_xy_density);
    sky.cloud_scale = scene.cloud_scale;
    sky.gamma = scene.scene_gamma;
    sky.glow = Glow::new(
        (2.0 - scene.sun_glow_size) * 20.0,
        0.0,
        -scene.sun_glow_focus * 5.0,
    );
    sky.cloud_scroll_rate = [
        if scene.cloud_scroll_x_lock {
            0.0
        } else {
            scene.cloud_scroll_x
        },
        if scene.cloud_scroll_y_lock {
            0.0
        } else {
            scene.cloud_scroll_y
        },
    ];
    sky.star_brightness = scene.star_brightness * 250.0;
    let frame = |name: &str| sl_proto::DayCycleFrame {
        keyframe: 0.0,
        name: name.to_owned(),
    };
    let mut cycle = stock;
    "Lightshare".clone_into(&mut cycle.name);
    cycle.water_track = vec![frame(&water.name)];
    cycle.water_frames = core::iter::once((water.name.clone(), water)).collect();
    if let Some(track) = cycle.sky_tracks.first_mut() {
        *track = vec![frame(&sky.name)];
    }
    for track in cycle.sky_tracks.iter_mut().skip(1) {
        track.clear();
    }
    cycle.sky_frames = core::iter::once((sky.name.clone(), sky)).collect();
    settings.day_cycle = cycle;
    settings
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A float equal to `want` within a hair of float rounding.
    fn close(got: f32, want: f32) -> bool {
        (got - want).abs() < 1e-4
    }

    /// OpenSim's LightShare defaults, turned into an environment and read
    /// back, are the defaults again — but for the water fog's exponent,
    /// whose square root undoes OpenSim's power of two only at 4, and the
    /// glow's size, which OpenSim's two formulas do not undo — and the way
    /// round from a sky its angles keep.
    #[test]
    fn a_lightshare_scene_survives_the_round_trip_as_opensim_converts_it() {
        let stock = EnvironmentSettings::default_region();
        let mut scene = LightShare {
            water_fog_density_exponent: 3.0,
            sun_moon_position: 0.2,
            east_angle: 0.1,
            ..LightShare::default()
        };
        scene.water_color = vector(32.0, 64.0, 128.0);
        let back = to_lightshare(&from_lightshare(&scene, &stock));
        assert!(close(back.water_color.y, 64.0), "{:?}", back.water_color);
        assert!(close(back.water_fog_density_exponent, 8.0_f32.sqrt()));
        assert!(
            close(back.sun_moon_position, 0.2),
            "{}",
            back.sun_moon_position
        );
        assert!(close(back.east_angle, 0.1), "{}", back.east_angle);
        assert!(close(back.ambient.x, scene.ambient.x));
        assert!(close(back.ambient.s, 1.0));
        // OpenSim's glow size goes out as `(2 - s) * 20` and back as
        // `(2 - x) / 20`, which is no inverse.
        assert!(close(
            back.sun_glow_size,
            (2.0 - (2.0 - scene.sun_glow_size) * 20.0) / 20.0
        ));
        assert!(close(back.density_multiplier, scene.density_multiplier));
        assert_eq!(back.max_altitude, scene.max_altitude);
        assert_eq!(back.normal_map_texture, scene.normal_map_texture);
        assert!(!back.cloud_scroll_x_lock);
    }

    /// A cut and a join recount the area and the box from the bitmap.
    #[test]
    fn a_recount_reads_the_blocks_the_bitmap_holds() {
        let mut parcel = crate::world::rect_parcel(
            RegionLocalParcelId(1),
            OwnerKey::Agent(AgentKey::from(uuid::Uuid::nil())),
            "box",
            8.0,
            4.0,
            16.0,
            12.0,
        );
        set_block(&mut parcel.bitmap, 2, 1, false);
        recount(&mut parcel);
        assert_eq!(parcel.area, LandArea(3 * 16));
        assert_eq!(parcel.aabb_min, RegionCoordinates::new(8.0, 4.0, 0.0));
        assert_eq!(parcel.aabb_max, RegionCoordinates::new(16.0, 12.0, 0.0));
    }
}
