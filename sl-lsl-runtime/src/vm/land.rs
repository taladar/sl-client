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
