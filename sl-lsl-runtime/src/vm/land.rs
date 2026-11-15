//! The land, the region and the estate as a script reads and changes them
//! (`server-lsl-lib-land-region-env`).
//!
//! The host describes what is there as plain records — a [`Parcel`], the
//! [`Region`], the [`Environment`] — and applies a [`LandChange`] the library
//! has checked; which code of `llGetParcelDetails` means what, which keys
//! `llGetEnv` answers, how a position off the region is read, are each grid's
//! and live in the library (`book/src/gridspec/lsl.md` § Parcel, region,
//! estate and environment queries).

use sl_types::lsl::{Rotation, Vector};

/// The prims a parcel, or the whole region, holds, by category
/// (`PARCEL_COUNT_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrimCounts {
    /// Every prim.
    pub total: i32,
    /// The parcel owner's.
    pub owner: i32,
    /// Set to the parcel's group.
    pub group: i32,
    /// Everybody else's.
    pub other: i32,
    /// Selected or sat on.
    pub selected: i32,
    /// Temporary.
    pub temporary: i32,
}

/// One parcel, as the library reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parcel {
    /// Its key (`PARCEL_DETAILS_ID`).
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its owner: an avatar, or the group that owns it.
    pub owner: String,
    /// Its group; `NULL_KEY` for none.
    pub group: String,
    /// Whether its group owns it.
    pub group_owned: bool,
    /// Its area, in square metres.
    pub area: i32,
    /// Its `PARCEL_FLAG_*` bits, as the grid keeps them.
    pub flags: u32,
    /// Whether avatars outside it see the avatars in it.
    pub see_avatars: bool,
    /// Where an avatar lands, and the way it looks.
    pub landing: (Vector, Vector),
    /// The teleport routing (`LANDING_TYPE_*`).
    pub tp_routing: i32,
    /// Its traffic.
    pub dwell: f32,
    /// When it was claimed, in seconds since the epoch.
    pub claim_date: i32,
    /// Whether anyone's avatar sounds are heard, and the group's.
    pub sounds: (bool, bool),
    /// Its music stream.
    pub music_url: String,
    /// The prims it may hold, and the prims the region may.
    pub max_prims: (i32, i32),
    /// Second Life's prim capacity and prims used (`PARCEL_DETAILS` 7 and 8).
    pub capacity: (i32, i32),
    /// Its prims, and the region's.
    pub counts: (PrimCounts, PrimCounts),
    /// Each owner of prims on it, with how many.
    pub prim_owners: Vec<(String, i32)>,
    /// Its access list: the avatars given a pass.
    pub passes: Vec<AccessEntry>,
    /// Its ban list.
    pub bans: Vec<AccessEntry>,
}

/// One avatar on a parcel's access or ban list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessEntry {
    /// The avatar's key.
    pub agent: String,
    /// When the entry lapses, in seconds since the epoch; 0 for never.
    pub expires: i64,
}

/// The region a script is in, as the library reads it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region {
    /// Its name.
    pub name: String,
    /// The simulator's release channel (`sim_channel`).
    pub channel: String,
    /// Its south-west corner on the grid, in metres.
    pub corner: (f64, f64),
    /// Its region flags, as the grid keeps them.
    pub flags: u32,
    /// Its maturity rating: `PG`, `MATURE` or `ADULT`.
    pub rating: String,
    /// The estate's id and name.
    pub estate: (i32, String),
    /// The region's product name and SKU.
    pub product: (String, String),
    /// The prims it may hold, and its object bonus.
    pub capacity: (i32, f32),
    /// How many avatars it admits.
    pub agent_limit: i32,
    /// When it started, in seconds since the epoch.
    pub start_time: i64,
    /// Its host's name, as scripts are told it.
    pub hostname: String,
    /// How many avatars are in it.
    pub agents: i32,
    /// Its water's height.
    pub water: f32,
    /// Whether a region adjoins it east, north, west and south.
    pub neighbours: [bool; 4],
    /// Whether the estate lets a parcel have an environment of its own
    /// (`AllowEnvironmentOverride`).
    pub environment_override: bool,
}

/// What `llRequestSimulatorData` reads of a region of the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct SimData {
    /// Its south-west corner, in metres.
    pub corner: (f64, f64),
    /// `up`, `down`, `starting`, `stopping`, `crashed` or `unknown`.
    pub status: String,
    /// `PG`, `MATURE` or `ADULT`.
    pub rating: String,
}

/// The environment over a point, as the library reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// The length of its day, in seconds.
    pub day_length: f32,
    /// Its day's offset from UTC, in seconds.
    pub day_offset: f32,
    /// How the sun is turned at the fraction of the day asked about.
    pub sun: Rotation,
    /// How the moon is turned then.
    pub moon: Rotation,
    /// The altitudes, in metres, at which its second, third and fourth sky
    /// tracks begin.
    pub altitudes: [f32; 3],
}

/// A change a script makes to the land or the estate, which the library has
/// checked the script's owner may make.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "a host imports it beside its own changes, where `Change` alone would say nothing"
)]
pub enum LandChange {
    /// `llSetParcelMusicURL`.
    MusicUrl(String),
    /// `llAddToLandPassList`: the avatar, for so many hours.
    Pass(String, f32),
    /// `llAddToLandBanList`: the avatar, for so many hours (0 for good).
    Ban(String, f32),
    /// `llRemoveFromLandPassList`.
    Unpass(String),
    /// `llRemoveFromLandBanList`.
    Unban(String),
    /// `llResetLandPassList`.
    ResetPasses,
    /// `llResetLandBanList`.
    ResetBans,
    /// `llEjectFromLand`: the avatar is sent off the parcel.
    Eject(String),
    /// `llManageEstateAccess`: the action and the avatar or group.
    Estate(i32, String),
    /// `osParcelJoin`: the parcels standing at these points merged into the
    /// first, which keeps its record and takes the others' land and dwell;
    /// the library has checked they share an owner the script's may join for.
    Join(Vec<(i32, i32)>),
    /// `osParcelSubdivide`: the whole four-metre blocks of the box from the
    /// first corner (west, south) to the second (east, north, exclusive) cut
    /// off the parcel holding them as a parcel of their own, a copy of it with
    /// a new key, no dwell, no search listing and no landing point.
    Subdivide((i32, i32), (i32, i32)),
    /// `osSetParcelDetails`: the parcel's fields a script set, each as it now
    /// is.
    Details(ParcelDetails),
    /// `llParcelMediaCommandList`: the parcel's media as it now is, and a
    /// command for the avatars on the parcel — `PARCEL_MEDIA_COMMAND_*`, to
    /// the one avatar named or every one — with the time to seek to.
    Media {
        /// The parcel's media settings.
        media: ParcelMedia,
        /// The command, if one was given.
        command: Option<i32>,
        /// The avatar it is for, or every avatar on the parcel.
        agent: Option<String>,
        /// The time `PARCEL_MEDIA_COMMAND_TIME` seeks to.
        time: f32,
    },
}

/// A parcel's media, as `llParcelMediaQuery` reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParcelMedia {
    /// The media URL.
    pub url: String,
    /// The texture the media replaces.
    pub texture: String,
    /// Its MIME type.
    pub mime: String,
    /// Its width and height.
    pub size: (i32, i32),
    /// Its description.
    pub description: String,
    /// Whether it loops.
    pub looping: bool,
}

/// The fields of a parcel `osSetParcelDetails` changed, each as it now is;
/// [`None`] for a field it left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParcelDetails {
    /// Its name.
    pub name: Option<String>,
    /// Its description.
    pub description: Option<String>,
    /// Its owner, an avatar.
    pub owner: Option<String>,
    /// Its group; `NULL_KEY` for none.
    pub group: Option<String>,
    /// When it was claimed, in seconds since the epoch.
    pub claim_date: Option<i32>,
    /// Whether avatars outside it see the avatars in it.
    pub see_avatars: Option<bool>,
    /// Whether anyone's avatar sounds are heard on it.
    pub any_sounds: Option<bool>,
    /// Whether its group's avatar sounds are.
    pub group_sounds: Option<bool>,
}

/// A change an OpenSim function makes to the region — its ground, water, sun,
/// wind, sky or the notices its avatars are sent — which the library has
/// checked the script's owner may make.
#[derive(Debug, Clone, PartialEq)]
pub enum RegionEdit {
    /// `osSetTerrainHeight`: the ground at the whole-metre point `(x, y)`,
    /// which lies within the region, to `height`. The region tells its
    /// avatars of the patches it moved, after the tick.
    TerrainHeight {
        /// Metres east.
        x: u32,
        /// Metres north.
        y: u32,
        /// The new height, in metres.
        height: f32,
    },
    /// `osTerrainFlush`: every patch of the ground sent again to every avatar.
    TerrainFlush,
    /// `osSetTerrainTexture`: the detail texture of `corner` (0–3).
    TerrainTexture {
        /// The corner.
        corner: usize,
        /// The texture's key.
        texture: String,
    },
    /// `osSetTerrainTextureHeight`: where `corner`'s (0–3) textures blend.
    TerrainTextureHeights {
        /// The corner.
        corner: usize,
        /// The height the blend starts at, in metres.
        low: f32,
        /// The range it spans, in metres.
        high: f32,
    },
    /// `osSetRegionWaterHeight`: the region's water line, in metres.
    WaterHeight(f32),
    /// `osSetRegionSunSettings`: whether the region follows the estate's sun,
    /// whether its own stands still, and where (Linden's 6–30 sun hour).
    Sun {
        /// Whether it follows the estate's sun.
        estate_sun: bool,
        /// Whether the region's sun stands still.
        fixed: bool,
        /// Its sun hour, 6 at sunrise.
        hour: f32,
    },
    /// `osSetWindParam`: a parameter of one of the region's wind models.
    WindParam {
        /// The model's name.
        plugin: String,
        /// The parameter's.
        param: String,
        /// The value.
        value: f32,
    },
    /// `osRegionNotice`: an alert to one avatar of the region (a non-modal
    /// `AgentAlertMessage`), or to every avatar in it (an `AlertMessage`).
    Notice {
        /// The avatar, or every one.
        agent: Option<String>,
        /// The text, as sent.
        text: String,
    },
    /// `osRegionRestart`: a restart scheduled, its first notice sent to every
    /// avatar in the region now; `alerts` are the seconds before the restart
    /// at which a notice is due, the first now, and `notice` the first
    /// notice's text.
    Restart {
        /// The countdown, in seconds before the restart.
        alerts: Vec<i32>,
        /// The first notice.
        notice: String,
    },
    /// `osRegionRestart` with less than 15 seconds: a scheduled restart
    /// called off, its avatars told so; nothing when none is scheduled.
    AbortRestart,
    /// `lsSetWindlightScene`: the region's environment replaced by one made
    /// of these settings, as OpenSim converts them, and its avatars told.
    LightShare(Box<LightShare>),
    /// `lsClearWindlightScene`: the region's own environment dropped, so it
    /// falls back to the grid's default.
    ClearLightShare,
}

/// Why a wind model parameter could not be read (`osGetWindParam`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindParamError {
    /// The region has no wind model of that name.
    NoPlugin,
    /// The model has no parameter of that name.
    NoParam {
        /// The model's name, as it calls itself.
        plugin: String,
    },
}

/// Whose environment an OpenSim function replaces.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvironmentScope {
    /// One avatar's own, which overrides every other it would see.
    Agent(String),
    /// The parcel's at a position.
    Parcel(Vector),
    /// The region's.
    Region,
}

/// What becomes of an environment ([`EnvironmentScope`]).
#[derive(Debug, Clone, PartialEq)]
pub enum EnvironmentChange {
    /// It is set: the one it has — the region's for a parcel or an avatar
    /// that has none — with the day cycle of the settings asset keyed
    /// `day_cycle`, and for the region the day's length and offset in
    /// seconds and its sky tracks' altitudes, each where given.
    Set {
        /// The day cycle's asset.
        day_cycle: Option<String>,
        /// The day's length.
        day_length: Option<i32>,
        /// The day's offset.
        day_offset: Option<i32>,
        /// The sky tracks' altitudes, sorted.
        altitudes: Option<[f32; 3]>,
    },
    /// It is dropped, so what it overrode shows again.
    Reset,
}

/// What a settings asset is, to a function that wants a day cycle of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayCycle {
    /// No asset of that key, or one that is no settings asset.
    Missing,
    /// A settings asset that holds a sky or a water, not a day cycle.
    NotDayCycle,
    /// A settings asset the grid cannot read.
    Unreadable,
    /// A day cycle.
    DayCycle,
}

/// The region's environment in LightShare's terms — OpenSim's
/// `RegionLightShareData`, which its `ToLightShare` reads out of the first
/// sky and water frames of the region's day cycle. The default is OpenSim's,
/// what a region with no environment of its own answers.
#[derive(Debug, Clone, PartialEq)]
pub struct LightShare {
    /// `WL_WATER_COLOR`, in 0–256 steps.
    pub water_color: Vector,
    /// `WL_WATER_FOG_DENSITY_EXPONENT`.
    pub water_fog_density_exponent: f32,
    /// `WL_UNDERWATER_FOG_MODIFIER`.
    pub underwater_fog_modifier: f32,
    /// `WL_REFLECTION_WAVELET_SCALE`.
    pub reflection_wavelet_scale: Vector,
    /// `WL_FRESNEL_SCALE`.
    pub fresnel_scale: f32,
    /// `WL_FRESNEL_OFFSET`.
    pub fresnel_offset: f32,
    /// `WL_REFRACT_SCALE_ABOVE`.
    pub refract_scale_above: f32,
    /// `WL_REFRACT_SCALE_BELOW`.
    pub refract_scale_below: f32,
    /// `WL_BLUR_MULTIPLIER`.
    pub blur_multiplier: f32,
    /// `WL_BIG_WAVE_DIRECTION`.
    pub big_wave_direction: (f32, f32),
    /// `WL_LITTLE_WAVE_DIRECTION`.
    pub little_wave_direction: (f32, f32),
    /// `WL_NORMAL_MAP_TEXTURE`.
    pub normal_map_texture: String,
    /// `WL_HORIZON`.
    pub horizon: Rotation,
    /// `WL_HAZE_HORIZON`.
    pub haze_horizon: f32,
    /// `WL_BLUE_DENSITY`.
    pub blue_density: Rotation,
    /// `WL_HAZE_DENSITY`.
    pub haze_density: f32,
    /// `WL_DENSITY_MULTIPLIER`.
    pub density_multiplier: f32,
    /// `WL_DISTANCE_MULTIPLIER`.
    pub distance_multiplier: f32,
    /// `WL_MAX_ALTITUDE`.
    pub max_altitude: u16,
    /// `WL_SUN_MOON_COLOR`.
    pub sun_moon_color: Rotation,
    /// `WL_SUN_MOON_POSITION`, the fraction of a turn.
    pub sun_moon_position: f32,
    /// `WL_AMBIENT`.
    pub ambient: Rotation,
    /// `WL_EAST_ANGLE`, the fraction of a turn.
    pub east_angle: f32,
    /// `WL_SUN_GLOW_FOCUS`.
    pub sun_glow_focus: f32,
    /// `WL_SUN_GLOW_SIZE`.
    pub sun_glow_size: f32,
    /// `WL_SCENE_GAMMA`.
    pub scene_gamma: f32,
    /// `WL_STAR_BRIGHTNESS`.
    pub star_brightness: f32,
    /// `WL_CLOUD_COLOR`.
    pub cloud_color: Rotation,
    /// `WL_CLOUD_XY_DENSITY`.
    pub cloud_xy_density: Vector,
    /// `WL_CLOUD_COVERAGE`.
    pub cloud_coverage: f32,
    /// `WL_CLOUD_SCALE`.
    pub cloud_scale: f32,
    /// `WL_CLOUD_DETAIL_XY_DENSITY`.
    pub cloud_detail_xy_density: Vector,
    /// `WL_CLOUD_SCROLL_X`.
    pub cloud_scroll_x: f32,
    /// `WL_CLOUD_SCROLL_X_LOCK`.
    pub cloud_scroll_x_lock: bool,
    /// `WL_CLOUD_SCROLL_Y`.
    pub cloud_scroll_y: f32,
    /// `WL_CLOUD_SCROLL_Y_LOCK`.
    pub cloud_scroll_y_lock: bool,
    /// `WL_DRAW_CLASSIC_CLOUDS`.
    pub draw_classic_clouds: bool,
}

/// `(x, y, z, s)` as a rotation, the four-component colour LightShare keeps.
const fn four(x: f32, y: f32, z: f32, s: f32) -> Rotation {
    Rotation { x, y, z, s }
}

/// `(x, y, z)` as a vector.
const fn three(x: f32, y: f32, z: f32) -> Vector {
    Vector { x, y, z }
}

impl Default for LightShare {
    /// OpenSim's `RegionLightShareData` defaults.
    fn default() -> Self {
        Self {
            water_color: three(4.0, 38.0, 64.0),
            water_fog_density_exponent: 4.0,
            underwater_fog_modifier: 0.25,
            reflection_wavelet_scale: three(2.0, 2.0, 2.0),
            fresnel_scale: 0.40,
            fresnel_offset: 0.50,
            refract_scale_above: 0.03,
            refract_scale_below: 0.20,
            blur_multiplier: 0.040,
            big_wave_direction: (1.05, -0.42),
            little_wave_direction: (1.11, -1.16),
            normal_map_texture: "822ded49-9a6c-f61c-cb89-6df54f42cdf4".to_owned(),
            horizon: four(0.25, 0.25, 0.32, 0.32),
            haze_horizon: 0.19,
            blue_density: four(0.12, 0.22, 0.38, 0.38),
            haze_density: 0.70,
            density_multiplier: 0.18,
            distance_multiplier: 0.8,
            max_altitude: 1605,
            sun_moon_color: four(0.24, 0.26, 0.30, 0.30),
            sun_moon_position: 0.317,
            ambient: four(0.35, 0.35, 0.35, 0.35),
            east_angle: 0.0,
            sun_glow_focus: 0.10,
            sun_glow_size: 1.75,
            scene_gamma: 1.0,
            star_brightness: 0.0,
            cloud_color: four(0.41, 0.41, 0.41, 0.41),
            cloud_xy_density: three(1.00, 0.53, 1.00),
            cloud_coverage: 0.27,
            cloud_scale: 0.42,
            cloud_detail_xy_density: three(1.00, 0.53, 0.12),
            cloud_scroll_x: 0.20,
            cloud_scroll_x_lock: false,
            cloud_scroll_y: 0.01,
            cloud_scroll_y_lock: false,
            draw_classic_clouds: true,
        }
    }
}
