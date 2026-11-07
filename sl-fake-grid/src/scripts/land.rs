//! The land, the region and the environment as a script reads and changes
//! them (`server-lsl-lib-land-region-env`): the host half, describing the
//! region's parcels, limits, ground, wind and day cycle as the runtime's
//! records and applying the land changes the library checked. Which code
//! means what, and each grid's quirks, are the runtime's (`library::land`).

use sl_lsl_runtime::Element;
use sl_lsl_runtime::vm::land::{
    AccessEntry, Environment, LandChange, Parcel, ParcelMedia, PrimCounts, Region, SimData,
};
use sl_proto::{
    AgentKey, Maturity, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelInfo,
    ParcelMediaCommand, ParcelMediaUpdateInfo, RegionLocalParcelId,
};
use sl_types::environment::Color;
use sl_types::lsl::{Rotation, Vector};

use super::{DEFAULT_DAY_LENGTH, DEFAULT_DAY_OFFSET, Delivery, RegionHost, narrow};

/// The name a script is told the fake grid's simulator host has, on both
/// flavours: its loopback address, as the local OpenSim's
/// `ExternalHostNameForLSL` answers.
const HOSTNAME: &str = "127.0.0.1";

/// The release channel a region of `opensim`'s flavour reports
/// (`sim_channel`).
const fn channel(opensim: bool) -> &'static str {
    if opensim {
        "OpenSim"
    } else {
        "Second Life Server"
    }
}

/// A colour as an LSL vector.
const fn colour(colour: &Color) -> Vector {
    Vector {
        x: colour.red(),
        y: colour.green(),
        z: colour.blue(),
    }
}

/// `(x, y, z)` as an LSL vector.
const fn vector(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

/// The rating a script reads for `maturity`.
const fn rating(maturity: Maturity) -> &'static str {
    match maturity {
        Maturity::Adult => "ADULT",
        Maturity::Mature => "MATURE",
        _ => "PG",
    }
}

/// `value` as an `i32`, saturating.
fn saturate(value: impl TryInto<i32>) -> i32 {
    value.try_into().unwrap_or(i32::MAX)
}

/// A grid coordinate's metres.
fn metres(grid: u32) -> f64 {
    f64::from(grid) * 256.0
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's methods are split by library tranche across the `scripts` modules"
)]
impl RegionHost<'_> {
    /// The prims of the region a parcel's counts are made of, by its record.
    fn counts_of(parcel: &ParcelInfo, region: bool) -> PrimCounts {
        if region {
            return PrimCounts {
                total: parcel.sim_wide_total_prims,
                ..PrimCounts::default()
            };
        }
        PrimCounts {
            total: parcel.total_prims,
            owner: parcel.owner_prims,
            group: parcel.group_prims,
            other: parcel.other_prims,
            selected: parcel.selected_prims,
            temporary: 0,
        }
    }

    /// The parcel at `(x, y)`, as the runtime reads it.
    pub(super) fn parcel_record(&self, x: f32, y: f32) -> Option<Parcel> {
        let info = self.world.parcel_at(x, y)?;
        let listing = self
            .world
            .listings
            .iter()
            .find(|listing| listing.local_id == info.local_id);
        let nil = uuid::Uuid::nil().to_string();
        Some(Parcel {
            id: listing.map_or_else(
                || nil.clone(),
                |listing| listing.parcel_id.uuid().to_string(),
            ),
            name: info.name.clone(),
            description: info.description.clone(),
            owner: info.owner.uuid().to_string(),
            group: info
                .group
                .map_or_else(|| nil.clone(), |group| group.uuid().to_string()),
            group_owned: info.owner.is_group(),
            area: saturate(info.area.0),
            flags: info.raw_parcel_flags,
            see_avatars: info.see_avs.unwrap_or(true),
            landing: (
                vector(
                    info.user_location.x(),
                    info.user_location.y(),
                    info.user_location.z(),
                ),
                vector(
                    info.user_look_at.x(),
                    info.user_look_at.y(),
                    info.user_look_at.z(),
                ),
            ),
            tp_routing: i32::from(info.landing_type.to_u8()),
            dwell: listing.map_or(0.0, |listing| listing.dwell),
            claim_date: info.claim_date,
            sounds: (
                info.any_av_sounds.unwrap_or(true),
                info.group_av_sounds.unwrap_or(true),
            ),
            music_url: info
                .music_url
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            max_prims: (info.max_prims, info.sim_wide_max_prims),
            capacity: (info.max_prims, info.total_prims),
            counts: (Self::counts_of(info, false), Self::counts_of(info, true)),
            prim_owners: Vec::new(),
            passes: self.access_entries(info.local_id, ParcelAccessScope::Access),
            bans: self.access_entries(info.local_id, ParcelAccessScope::Ban),
        })
    }

    /// One of the parcel `local_id`'s access lists, as the runtime reads it.
    fn access_entries(
        &self,
        local_id: RegionLocalParcelId,
        scope: ParcelAccessScope,
    ) -> Vec<AccessEntry> {
        self.world
            .access_list(local_id, scope)
            .iter()
            .map(|entry| AccessEntry {
                agent: entry.id.to_string(),
                expires: i64::from(entry.time),
            })
            .collect()
    }

    /// The region, as the runtime reads it.
    pub(super) fn region_record(&self) -> Region {
        let Some(entry) = self.core.region(self.index) else {
            return Region::default();
        };
        let identity = entry.script_identity(self.core);
        let limits = self.world.limits(&identity);
        let opensim = self.opensim();
        let mut neighbours = [false; 4];
        for index in self.core.neighbours_of(self.index) {
            let Some(other) = self.core.region(index) else {
                continue;
            };
            let (x, y) = (entry.config.grid_x, entry.config.grid_y);
            let (ox, oy) = (other.config.grid_x, other.config.grid_y);
            let side = match (ox.cmp(&x), oy.cmp(&y)) {
                (core::cmp::Ordering::Greater, core::cmp::Ordering::Equal) => 0,
                (core::cmp::Ordering::Equal, core::cmp::Ordering::Greater) => 1,
                (core::cmp::Ordering::Less, core::cmp::Ordering::Equal) => 2,
                (core::cmp::Ordering::Equal, core::cmp::Ordering::Less) => 3,
                _ => continue,
            };
            if let Some(slot) = neighbours.get_mut(side) {
                *slot = true;
            }
        }
        Region {
            name: entry.config.name.clone(),
            channel: channel(opensim).to_owned(),
            corner: (metres(entry.config.grid_x), metres(entry.config.grid_y)),
            flags: u32::try_from(limits.region_flags.bits() & u64::from(u32::MAX)).unwrap_or(0),
            rating: rating(limits.maturity).to_owned(),
            estate: (saturate(limits.estate_id), self.world.estate.name.clone()),
            product: (limits.product_name.clone(), limits.product_sku.clone()),
            capacity: (
                saturate(limits.hard_max_objects),
                limits.object_bonus_factor,
            ),
            agent_limit: saturate(limits.max_agents),
            start_time: self
                .world
                .script_instances
                .epoch_micros
                .unwrap_or(0)
                .div_euclid(1_000_000),
            hostname: HOSTNAME.to_owned(),
            agents: saturate(self.present.len()),
            water: limits.water_height,
            neighbours,
        }
    }

    /// What `llRequestSimulatorData` reads of the grid's region `name`.
    pub(super) fn sim_record(&self, name: &str) -> Option<SimData> {
        // Region names are matched without regard to case, as both grids'
        // grid services match them.
        let entry = (0..)
            .map_while(|index| self.core.region(index))
            .find(|entry| entry.config.name.eq_ignore_ascii_case(name))?;
        Some(SimData {
            corner: (metres(entry.config.grid_x), metres(entry.config.grid_y)),
            status: "up".to_owned(),
            rating: rating(entry.config.maturity).to_owned(),
        })
    }

    /// The map texture of the region named or keyed `region`, or of this
    /// one for an empty `region`: the key the world map hands out for it,
    /// which is the region's own id.
    pub(super) fn map_texture_of(&self, region: &str) -> Option<String> {
        let index = if region.is_empty() {
            Some(self.index)
        } else if let Ok(id) = uuid::Uuid::parse_str(region) {
            self.core.region_by_id(id)
        } else {
            self.core.region_by_name(region)
        }?;
        Some(self.core.region(index)?.region_id.to_string())
    }

    /// The ground's height at `(x, y)`.
    pub(super) fn ground_at(&self, x: f32, y: f32) -> f32 {
        self.core
            .region(self.index)
            .map_or(0.0, |region| region.config.terrain.height_at(x, y))
    }

    /// The wind: the terrain fixture's, the same everywhere, or still air.
    pub(super) fn wind_at(&self) -> Vector {
        self.core
            .region(self.index)
            .and_then(|region| region.config.terrain.wind)
            .map_or(vector(0.0, 0.0, 0.0), |[east, north]| {
                vector(east, north, 0.0)
            })
    }

    /// The region's environment settings, if it keeps its own.
    fn settings(&self) -> Option<&sl_proto::EnvironmentSettings> {
        self.core
            .region(self.index)
            .and_then(|region| region.config.environment.as_ref())
    }

    /// The length of the region's day and its offset, in whole seconds.
    fn day_seconds(&self) -> (i32, i32) {
        self.settings()
            .map_or((DEFAULT_DAY_LENGTH, DEFAULT_DAY_OFFSET), |settings| {
                (settings.day_length.max(1), settings.day_offset)
            })
    }

    /// The length of the region's day and its offset, in seconds.
    pub(super) fn day(&self) -> (f32, f32) {
        let (length, offset) = self.day_seconds();
        (narrow(f64::from(length)), narrow(f64::from(offset)))
    }

    /// The fraction of its day the region is at, at UNIX time `unix`.
    fn day_fraction(&self, unix: i64) -> f32 {
        let (length, offset) = self.day_seconds();
        let into = unix
            .saturating_add(i64::from(offset))
            .rem_euclid(i64::from(length));
        narrow(f64::from(i32::try_from(into).unwrap_or(0)) / f64::from(length))
    }

    /// The environment over the region at UNIX time `unix`.
    pub(super) fn environment_at(&self, unix: i64) -> Environment {
        let (length, offset) = self.day();
        let fraction = self.day_fraction(unix);
        let sky = self
            .settings()
            .and_then(|settings| settings.blended_sky_settings(0.0, fraction));
        let (sun, moon) = match sky {
            Some(sky) => (sky.sun_rotation, sky.moon_rotation),
            None => {
                // No day cycle of its own: the sun turned about the north
                // axis through the day, the moon opposite.
                let angle = fraction * core::f32::consts::TAU;
                let (sin, cos) = (angle / 2.0).sin_cos();
                (
                    Rotation {
                        x: 0.0,
                        y: -sin,
                        z: 0.0,
                        s: cos,
                    },
                    Rotation {
                        x: 0.0,
                        y: cos,
                        z: 0.0,
                        s: sin,
                    },
                )
            }
        };
        Environment {
            day_length: length,
            day_offset: offset,
            sun,
            moon,
        }
    }

    /// The values `llGetEnvironment`'s rule `rule` reads of the sky and the
    /// water at noon of the region's day: the settings' fields in the
    /// rule's order.
    pub(super) fn environment_values(&self, rule: i32) -> Option<Vec<Element>> {
        let settings = self
            .settings()
            .cloned()
            .unwrap_or_else(sl_proto::EnvironmentSettings::default_region);
        let sky = settings.blended_sky_settings(0.0, 0.5)?;
        let water = settings.blended_water_settings(0.5)?;
        let float = Element::Float;
        let v = Element::Vector;
        let int = |value: bool| Element::Integer(i32::from(value));
        Some(match rule {
            0 => vec![v(colour(&sky.ambient))],
            1 => vec![
                int(sky.sun_texture.is_none()),
                int(sky.moon_texture.is_none()),
                int(sky.cloud_texture.is_none()),
            ],
            2 => vec![
                v(colour(&sky.cloud_color)),
                float(sky.cloud_shadow),
                float(sky.cloud_scale),
                float(sky.cloud_variance),
                v(vector(
                    sky.cloud_scroll_rate[0],
                    sky.cloud_scroll_rate[1],
                    0.0,
                )),
                v(vector(
                    sky.cloud_pos_density1.position_x(),
                    sky.cloud_pos_density1.position_y(),
                    sky.cloud_pos_density1.density(),
                )),
                v(vector(
                    sky.cloud_pos_density2.position_x(),
                    sky.cloud_pos_density2.position_y(),
                    sky.cloud_pos_density2.density(),
                )),
                int(true),
            ],
            4 => vec![
                float(sky.dome_offset.unwrap_or(0.96)),
                float(sky.dome_radius.unwrap_or(15_000.0)),
                float(sky.max_y),
            ],
            5 => vec![float(sky.gamma)],
            6 => vec![float(sky.glow.size()), float(sky.glow.focus())],
            8 => vec![
                v(vector(0.0, 0.0, 1.0)),
                v(vector(
                    sky.sunlight_color.red(),
                    sky.sunlight_color.green(),
                    sky.sunlight_color.blue(),
                )),
                v(colour(&sky.ambient)),
            ],
            9 => vec![
                Element::Rotation(sky.moon_rotation.clone()),
                float(sky.moon_scale),
                float(sky.moon_brightness),
                int(sky.moon_texture.is_none()),
                v(vector(0.0, 0.0, -1.0)),
                v(vector(0.0, 0.0, 0.0)),
                v(vector(0.0, 0.0, 0.0)),
            ],
            10 => vec![
                float(sky.planet_radius),
                float(sky.sky_bottom_radius),
                float(sky.sky_top_radius),
            ],
            11 => vec![
                float(sky.moisture_level),
                float(sky.droplet_radius),
                float(sky.ice_level),
            ],
            13 => vec![float(sky.star_brightness)],
            14 => vec![
                Element::Rotation(sky.sun_rotation.clone()),
                float(sky.sun_scale),
                v(vector(0.0, 0.0, 0.0)),
                int(sky.sun_texture.is_none()),
                v(vector(0.0, 0.0, 1.0)),
                v(vector(
                    sky.sunlight_color.red(),
                    sky.sunlight_color.green(),
                    sky.sunlight_color.blue(),
                )),
                v(colour(&sky.ambient)),
            ],
            15 => vec![
                float(settings.track_altitudes[0]),
                float(settings.track_altitudes[1]),
                float(settings.track_altitudes[2]),
            ],
            22 => vec![v(colour(&sky.blue_density)), v(colour(&sky.blue_horizon))],
            23 => vec![
                float(sky.haze_horizon),
                float(sky.haze_density),
                float(sky.density_multiplier),
                float(sky.distance_multiplier),
            ],
            24 => vec![float(sky.reflection_probe_ambiance)],
            100 => vec![float(water.blur_multiplier)],
            101 => vec![
                v(colour(&water.water_fog_color)),
                float(water.water_fog_density),
                float(water.underwater_fog_mod),
            ],
            102 => vec![float(water.fresnel_offset), float(water.fresnel_scale)],
            103 => vec![
                int(water.normal_map.is_none()),
                int(water.transparent_texture.is_none()),
            ],
            104 => vec![v(vector(
                water.normal_scale.x(),
                water.normal_scale.y(),
                water.normal_scale.z(),
            ))],
            105 => vec![float(water.scale_above), float(water.scale_below)],
            106 => vec![
                v(vector(
                    water.wave1_direction[0],
                    water.wave1_direction[1],
                    0.0,
                )),
                v(vector(
                    water.wave2_direction[0],
                    water.wave2_direction[1],
                    0.0,
                )),
            ],
            _ => return None,
        })
    }

    /// Makes `change` to the parcel at `at`, or to the estate.
    pub(super) fn land_change(&mut self, at: &Vector, change: &LandChange) -> bool {
        let Some(local_id) = self
            .world
            .parcel_at(at.x, at.y)
            .map(|parcel| parcel.local_id)
        else {
            return false;
        };
        let now = self.world.wall.unix_seconds_i32();
        let expiry = |hours: f32| {
            if hours <= 0.0 {
                return 0;
            }
            let seconds = core::time::Duration::try_from_secs_f32(hours * 3600.0)
                .map_or(i32::MAX, |span| saturate(span.as_secs()));
            now.saturating_add(seconds)
        };
        let entry = |agent: &str, time: i32| {
            uuid::Uuid::parse_str(agent)
                .ok()
                .map(|id| ParcelAccessEntry {
                    id,
                    time,
                    flags: ParcelAccessFlags::NONE,
                })
        };
        match change {
            LandChange::MusicUrl(url) => {
                let Some(parcel) = self.world.parcel_at_mut(at.x, at.y) else {
                    return false;
                };
                parcel.music_url = url::Url::parse(url).ok();
                let record = parcel.clone();
                for agent in self.on_parcel(local_id, None) {
                    self.deliveries.push(Delivery::Parcel {
                        agent,
                        parcel: Box::new(record.clone()),
                    });
                }
                true
            }
            LandChange::Pass(agent, hours) | LandChange::Ban(agent, hours) => {
                let scope = if matches!(change, LandChange::Pass(..)) {
                    ParcelAccessScope::Access
                } else {
                    ParcelAccessScope::Ban
                };
                let Some(added) = entry(agent, expiry(*hours)) else {
                    return false;
                };
                let list = self.world.access_list_mut(local_id, scope);
                list.retain(|held| held.id != added.id);
                list.push(added);
                true
            }
            LandChange::Unpass(agent) | LandChange::Unban(agent) => {
                let scope = if matches!(change, LandChange::Unpass(_)) {
                    ParcelAccessScope::Access
                } else {
                    ParcelAccessScope::Ban
                };
                let Ok(id) = uuid::Uuid::parse_str(agent) else {
                    return false;
                };
                self.world
                    .access_list_mut(local_id, scope)
                    .retain(|held| held.id != id);
                true
            }
            LandChange::ResetPasses => {
                self.world
                    .access_list_mut(local_id, ParcelAccessScope::Access)
                    .clear();
                true
            }
            LandChange::ResetBans => {
                self.world
                    .access_list_mut(local_id, ParcelAccessScope::Ban)
                    .clear();
                true
            }
            LandChange::Estate(action, who) => {
                let Ok(id) = uuid::Uuid::parse_str(who) else {
                    return false;
                };
                let estate = &mut self.world.estate;
                let (list, add) = match action {
                    4 => (&mut estate.allowed_agents, true),
                    8 => (&mut estate.allowed_agents, false),
                    16 => (&mut estate.allowed_groups, true),
                    32 => (&mut estate.allowed_groups, false),
                    64 => (&mut estate.banned_agents, true),
                    128 => (&mut estate.banned_agents, false),
                    _ => return false,
                };
                list.retain(|held| *held != id);
                if add {
                    list.push(id);
                }
                true
            }
            LandChange::Media {
                media,
                command,
                agent,
                time,
            } => {
                let Some(parcel) = self.world.parcel_at_mut(at.x, at.y) else {
                    return false;
                };
                let update = ParcelMediaUpdateInfo {
                    media_url: url::Url::parse(&media.url).ok(),
                    media_id: uuid::Uuid::parse_str(&media.texture)
                        .ok()
                        .filter(|texture| !texture.is_nil())
                        .map(sl_types::key::TextureKey::from),
                    media_auto_scale: parcel.media_auto_scale,
                    media_type: media.mime.clone(),
                    media_desc: media.description.clone(),
                    media_width: Some(media.size.0),
                    media_height: Some(media.size.1),
                    media_loop: media.looping,
                };
                let data = sl_proto::ParcelMediaData {
                    description: update.media_desc.clone(),
                    media_type: update.media_type.clone(),
                    width: media.size.0,
                    height: media.size.1,
                    looping: media.looping,
                };
                let changed = parcel.media_url != update.media_url
                    || parcel.media_id != update.media_id
                    || parcel.media_data.as_ref() != Some(&data);
                parcel.media_url.clone_from(&update.media_url);
                parcel.media_id = update.media_id;
                parcel.media_data = Some(data);
                let to = agent
                    .as_deref()
                    .and_then(|agent| uuid::Uuid::parse_str(agent).ok())
                    .map(AgentKey::from);
                for agent in self.on_parcel(local_id, to) {
                    self.deliveries.push(Delivery::Media {
                        agent,
                        update: changed.then(|| Box::new(update.clone())),
                        command: command.map(|code| {
                            let flags = u32::try_from(code)
                                .ok()
                                .and_then(|bit| 1_u32.checked_shl(bit))
                                .unwrap_or(0);
                            (
                                flags,
                                ParcelMediaCommand::from_u32(u32::try_from(code).unwrap_or(0)),
                                *time,
                            )
                        }),
                    });
                }
                true
            }
            // The fake grid moves no avatar off a parcel yet.
            LandChange::Eject(_) => false,
        }
    }

    /// The media of the parcel at `at`.
    pub(super) fn media_of(&self, at: &Vector) -> ParcelMedia {
        let Some(parcel) = self.world.parcel_at(at.x, at.y) else {
            return ParcelMedia::default();
        };
        let data = parcel.media_data.clone().unwrap_or_default();
        ParcelMedia {
            url: parcel
                .media_url
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            texture: parcel.media_id.map_or_else(
                || uuid::Uuid::nil().to_string(),
                |texture| texture.uuid().to_string(),
            ),
            mime: data.media_type,
            size: (data.width, data.height),
            description: data.description,
            looping: data.looping,
        }
    }

    /// The avatars standing on the parcel `local_id` — only `only`, when
    /// one is named.
    fn on_parcel(&self, local_id: RegionLocalParcelId, only: Option<AgentKey>) -> Vec<AgentKey> {
        self.present
            .iter()
            .filter(|present| only.is_none_or(|agent| agent == present.agent))
            .filter(|present| {
                self.world
                    .parcel_at(present.position.x, present.position.y)
                    .is_some_and(|parcel| parcel.local_id == local_id)
            })
            .map(|present| present.agent)
            .collect()
    }

    /// The region's statistics in `SIM_STAT_*` order: the frame rate, the
    /// avatars and the scripts it knows, and 0 for the times the fake grid
    /// does not measure.
    pub(super) fn stats(&self) -> Vec<f32> {
        let step = self
            .core
            .region(self.index)
            .map_or(0.0, |region| region.config.heartbeat.step.as_secs_f64());
        let fps = if step > 0.0 { narrow(1.0 / step) } else { 0.0 };
        let agents = narrow(f64::from(saturate(self.present.len())));
        let mut stats = vec![0.0_f32; 27];
        for (code, value) in [(1, fps), (10, agents)] {
            if let Some(slot) = stats.get_mut(code) {
                *slot = value;
            }
        }
        stats
    }
}
