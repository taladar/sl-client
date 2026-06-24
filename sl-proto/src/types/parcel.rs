//! Parcels and land management: properties, access lists, media, overlays.

use sl_types::key::{AgentKey, GroupKey, ObjectKey, OwnerKey, ParcelKey, TextureKey};
use sl_types::map::{RegionCoordinates, RegionName};
use sl_types::money::LindenAmount;
use sl_wire::ParcelFlags;
use sl_wire::{Direction, GlobalCoordinates};
use sl_wire::{RegionLocalObjectId, RegionLocalParcelId};
use uuid::Uuid;

use crate::types::LandArea;

/// How many parcels a `ParcelProperties` reply describes, the `RequestResult`
/// field. A "not found / no access" reply arrives as [`NoData`](Self::NoData)
/// and must be distinguished from a normal parcel (mirrors the viewer's
/// `PARCEL_RESULT_*` constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ParcelRequestResult {
    /// No parcel data (the query found nothing, or access was denied)
    /// (`PARCEL_RESULT_NO_DATA`, `-1`).
    NoData,
    /// Exactly one parcel was selected (`PARCEL_RESULT_SUCCESS`, `0`).
    #[default]
    Single,
    /// Multiple parcels were selected (`PARCEL_RESULT_MULTIPLE`, `1`).
    Multiple,
    /// An unrecognised result code, preserved verbatim.
    Unknown(i32),
}

impl ParcelRequestResult {
    /// Classifies a `RequestResult` wire value.
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            -1 => Self::NoData,
            0 => Self::Single,
            1 => Self::Multiple,
            other => Self::Unknown(other),
        }
    }

    /// The `RequestResult` wire value for this classification (inverse of
    /// [`from_i32`](Self::from_i32)).
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::NoData => -1,
            Self::Single => 0,
            Self::Multiple => 1,
            Self::Unknown(other) => other,
        }
    }

    /// Whether the reply carries real parcel data (anything but
    /// [`NoData`](Self::NoData)).
    #[must_use]
    pub const fn has_data(self) -> bool {
        !matches!(self, Self::NoData)
    }
}

/// A parcel's ownership status, the `Status` field of `ParcelProperties` (the
/// viewer's `LLParcel::EOwnershipStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ParcelStatus {
    /// The parcel is leased (owned) (`OS_LEASED`, `0`).
    #[default]
    Leased,
    /// A lease is pending (`OS_LEASE_PENDING`, `1`).
    LeasePending,
    /// The parcel has been abandoned (`OS_ABANDONED`, `2`).
    Abandoned,
    /// No ownership status (`OS_NONE`, `-1`).
    None,
    /// An unrecognised status value, preserved verbatim.
    Unknown(i32),
}

impl ParcelStatus {
    /// Classifies a `Status` wire value (the UDP `U8` widened to `i32`, or the
    /// CAPS integer which may be the negative `OS_NONE`).
    #[must_use]
    pub const fn from_i32(value: i32) -> Self {
        match value {
            0 => Self::Leased,
            1 => Self::LeasePending,
            2 => Self::Abandoned,
            -1 => Self::None,
            other => Self::Unknown(other),
        }
    }

    /// The `Status` wire value for this classification (inverse of
    /// [`from_i32`](Self::from_i32)).
    #[must_use]
    pub const fn to_i32(self) -> i32 {
        match self {
            Self::Leased => 0,
            Self::LeasePending => 1,
            Self::Abandoned => 2,
            Self::None => -1,
            Self::Unknown(other) => other,
        }
    }
}

/// How an avatar arrives on a parcel, the `LandingType` field of
/// `ParcelProperties` (the viewer's `LLParcel::ELandingType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LandingType {
    /// Teleport routing is blocked (`L_NONE`, `0`).
    #[default]
    Blocked,
    /// Arrivals are routed to the parcel's landing point (`L_LANDING_POINT`, `1`).
    LandingPoint,
    /// Arrivals land directly at the requested spot (`L_DIRECT`, `2`).
    Anywhere,
    /// An unrecognised landing type, preserved verbatim.
    Unknown(u8),
}

impl LandingType {
    /// Classifies a `LandingType` wire value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Blocked,
            1 => Self::LandingPoint,
            2 => Self::Anywhere,
            other => Self::Unknown(other),
        }
    }

    /// The `LandingType` wire value for this classification (inverse of
    /// [`from_u8`](Self::from_u8)).
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Blocked => 0,
            Self::LandingPoint => 1,
            Self::Anywhere => 2,
            Self::Unknown(other) => other,
        }
    }
}

/// A parcel's geometry, flags, and limits, parsed from `ParcelProperties`.
///
/// The parcel flag bits are exposed through the boolean accessor methods
/// ([`ParcelInfo::create_objects`], [`ParcelInfo::use_ban_list`], …); the raw
/// bitfield is available via [`ParcelInfo::flags`] / [`ParcelInfo::raw_parcel_flags`].
/// The `region_deny_*` / `region_*_override` booleans are *region*-level
/// settings echoed into the parcel reply, distinct from the parcel's own
/// [`ParcelFlags`](sl_wire::ParcelFlags) bits.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "faithfully mirrors the distinct ParcelProperties wire booleans"
)]
pub struct ParcelInfo {
    /// The request sequence id echoed back (used to match an outstanding query).
    pub sequence_id: i32,
    /// How many parcels this reply describes; [`NoData`](ParcelRequestResult::NoData)
    /// means "not found / no access" rather than a real parcel.
    pub request_result: ParcelRequestResult,
    /// Whether the viewer should snap its selection to the returned parcel.
    pub snap_selection: bool,
    /// The number of the requesting agent's own avatars on the parcel.
    pub self_count: i32,
    /// The number of other agents on the parcel.
    pub other_count: i32,
    /// The number of public/anonymous agents on the parcel.
    pub public_count: i32,
    /// The parcel's region-local id.
    pub local_id: RegionLocalParcelId,
    /// The parcel's owner — an agent or a group.
    pub owner: OwnerKey,
    /// The group the parcel is set to, or `None` when no group is set (distinct
    /// from a group-*owned* parcel, which is signalled by [`owner`](Self::owner)).
    pub group: Option<GroupKey>,
    /// The auction id, if the parcel is being auctioned (`0` if not).
    pub auction_id: u32,
    /// When the parcel was claimed, as a Unix timestamp (`time_t`).
    pub claim_date: i32,
    /// The price paid to claim the parcel, in L$.
    pub claim_price: LindenAmount,
    /// The parcel's rent price, in L$.
    pub rent_price: LindenAmount,
    /// The minimum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_min: RegionCoordinates,
    /// The maximum corner of the parcel's axis-aligned bounding box, in metres.
    pub aabb_max: RegionCoordinates,
    /// The parcel area in square metres.
    pub area: LandArea,
    /// One bit per 4×4 m region square, marking which squares belong to this
    /// parcel (row-major, least-significant-bit first).
    pub bitmap: Vec<u8>,
    /// The parcel's ownership status (leased / abandoned / …).
    pub status: ParcelStatus,
    /// The parcel's search category.
    pub category: ParcelCategory,
    /// The parcel's maximum object/prim capacity (without bonus).
    pub max_prims: i32,
    /// The region-wide maximum object/prim capacity.
    pub sim_wide_max_prims: i32,
    /// The region-wide current object/prim count.
    pub sim_wide_total_prims: i32,
    /// The total objects/prims currently on the parcel.
    pub total_prims: i32,
    /// The objects/prims on the parcel owned by the parcel owner.
    pub owner_prims: i32,
    /// The objects/prims on the parcel set to the parcel's group.
    pub group_prims: i32,
    /// The objects/prims on the parcel owned by anyone else.
    pub other_prims: i32,
    /// The objects/prims on the parcel that are currently selected.
    pub selected_prims: i32,
    /// The parcel's object-bonus multiplier applied to [`max_prims`](Self::max_prims).
    pub parcel_prim_bonus: f32,
    /// The auto-return time for other people's objects, in minutes (`0` = never).
    pub other_clean_time: i32,
    /// The raw `ParcelFlags` bitfield (decode with [`sl_wire::ParcelFlags`]).
    pub raw_parcel_flags: u32,
    /// The parcel's sale price, in L$.
    pub sale_price: Option<LindenAmount>,
    /// The parcel's name.
    pub name: String,
    /// The parcel's description.
    pub description: String,
    /// The parcel's streaming-audio URL (the "music" stream), [`None`] if none.
    /// Set it with [`ParcelUpdate::music_url`].
    pub music_url: Option<url::Url>,
    /// The parcel's media URL (movie / web page), [`None`] if none. Set it with
    /// [`ParcelUpdate::media_url`]. This is the legacy single-media-URL field;
    /// the per-face media-on-a-prim system is a separate (CAPS) surface.
    pub media_url: Option<url::Url>,
    /// The texture id the parcel media replaces while playing (`None` if none).
    pub media_id: Option<TextureKey>,
    /// Whether the media is auto-scaled to fit the surface it replaces.
    pub media_auto_scale: bool,
    /// The only agent allowed to buy the parcel (`None` for anyone).
    pub auth_buyer_id: Option<AgentKey>,
    /// The parcel's snapshot texture id (`None` if none).
    pub snapshot_id: Option<TextureKey>,
    /// The price of a parcel pass, in L$.
    pub pass_price: LindenAmount,
    /// How many hours a parcel pass lasts.
    pub pass_hours: f32,
    /// The teleport-landing location within the parcel, in region metres.
    pub user_location: RegionCoordinates,
    /// The direction an arriving agent faces at the landing point.
    pub user_look_at: Direction,
    /// How an avatar arrives on the parcel (blocked / landing point / anywhere).
    pub landing_type: LandingType,
    /// Region setting: pushing (`llPushObject`) is overridden/blocked region-wide.
    pub region_push_override: bool,
    /// Region setting: anonymous (non-account) avatars are denied region-wide.
    pub region_deny_anonymous: bool,
    /// Region setting: identified-but-not-payment avatars are denied region-wide.
    pub region_deny_identified: bool,
    /// Region setting: avatars without a payment-info-on-file are denied region-wide.
    pub region_deny_transacted: bool,
    /// Region setting: age-unverified avatars are denied region-wide.
    pub region_deny_age_unverified: bool,
    /// Region setting: per-parcel access restrictions are allowed (estate not tax-free).
    pub region_allow_access_override: bool,
    /// The parcel's environment (EEP) version, or `-1` when overrides are off.
    pub parcel_environment_version: i32,
    /// Region setting: per-parcel environment (EEP) overrides are allowed.
    pub region_allow_environment_override: bool,
    /// Whether avatars on the parcel are visible from outside it
    /// (`SeeAVs`); `None` when not provided (the UDP path omits it).
    pub see_avs: Option<bool>,
    /// Whether anyone's avatar sounds (gestures, footsteps) play on the parcel
    /// (`AnyAVSounds`); `None` when not provided (the UDP path omits it).
    pub any_av_sounds: Option<bool>,
    /// Whether group members' avatar sounds play on the parcel
    /// (`GroupAVSounds`); `None` when not provided (the UDP path omits it).
    pub group_av_sounds: Option<bool>,
}

impl ParcelInfo {
    /// The decoded parcel flag bits.
    #[must_use]
    pub const fn flags(&self) -> sl_wire::ParcelFlags {
        sl_wire::ParcelFlags::from_bits(self.raw_parcel_flags)
    }

    /// Anyone may create (rez) objects here — a public rez zone.
    #[must_use]
    pub const fn create_objects(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::CREATE_OBJECTS)
    }

    /// Group members may create (rez) objects here — a group rez zone.
    #[must_use]
    pub const fn create_group_objects(&self) -> bool {
        self.flags()
            .contains(sl_wire::ParcelFlags::CREATE_GROUP_OBJECTS)
    }

    /// A ban list is in effect (banlines).
    #[must_use]
    pub const fn use_ban_list(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::USE_BAN_LIST)
    }

    /// Access is restricted to an allow list.
    #[must_use]
    pub const fn use_access_list(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::USE_ACCESS_LIST)
    }

    /// Anonymous (non-account) avatars are denied access.
    #[must_use]
    pub const fn deny_anonymous(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::DENY_ANONYMOUS)
    }
}

/// A region parcel-ownership overlay chunk, parsed from `ParcelOverlay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelOverlayInfo {
    /// Which of the four overlay chunks this is (0–3).
    pub sequence_id: i32,
    /// The packed overlay bytes: per-square ownership colour and edge/flag bits.
    pub data: Vec<u8>,
}

/// A scripted parcel-media control command, the `Command` of a
/// [`Event::ParcelMediaCommand`](crate::Event::ParcelMediaCommand) (`ParcelMediaCommandMessage`). The values match
/// the viewer's `PARCEL_MEDIA_COMMAND_*` constants and the LSL
/// `PARCEL_MEDIA_COMMAND_*` flags fed to `llParcelMediaCommandList`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParcelMediaCommand {
    /// Stop the media and unload it (`PARCEL_MEDIA_COMMAND_STOP`).
    Stop,
    /// Pause the media, keeping it loaded (`PARCEL_MEDIA_COMMAND_PAUSE`).
    Pause,
    /// Start (or resume) playback once (`PARCEL_MEDIA_COMMAND_PLAY`).
    Play,
    /// Start playback looping (`PARCEL_MEDIA_COMMAND_LOOP`).
    Loop,
    /// Set the media's replacement texture (`PARCEL_MEDIA_COMMAND_TEXTURE`).
    Texture,
    /// Set the media URL (`PARCEL_MEDIA_COMMAND_URL`).
    Url,
    /// Seek to a time offset, in seconds (`PARCEL_MEDIA_COMMAND_TIME`; the value
    /// is the [`time`](crate::Event::ParcelMediaCommand::time) field).
    Time,
    /// Target a single agent rather than the whole parcel
    /// (`PARCEL_MEDIA_COMMAND_AGENT`).
    Agent,
    /// Unload the media from memory (`PARCEL_MEDIA_COMMAND_UNLOAD`).
    Unload,
    /// Auto-align the media to the texture (`PARCEL_MEDIA_COMMAND_AUTO_ALIGN`).
    AutoAlign,
    /// Set the media MIME type (`PARCEL_MEDIA_COMMAND_TYPE`).
    Type,
    /// Set the media surface size in pixels (`PARCEL_MEDIA_COMMAND_SIZE`).
    Size,
    /// Set the media description (`PARCEL_MEDIA_COMMAND_DESC`).
    Desc,
    /// Set whether the media loops (`PARCEL_MEDIA_COMMAND_LOOP_SET`).
    LoopSet,
    /// An unrecognised command code (forward-compatible).
    Other(u32),
}

impl ParcelMediaCommand {
    /// Maps a wire `Command` code to a [`ParcelMediaCommand`], preserving an
    /// unknown code as [`Other`](Self::Other).
    #[must_use]
    pub const fn from_u32(code: u32) -> Self {
        match code {
            0 => Self::Stop,
            1 => Self::Pause,
            2 => Self::Play,
            3 => Self::Loop,
            4 => Self::Texture,
            5 => Self::Url,
            6 => Self::Time,
            7 => Self::Agent,
            8 => Self::Unload,
            9 => Self::AutoAlign,
            10 => Self::Type,
            11 => Self::Size,
            12 => Self::Desc,
            13 => Self::LoopSet,
            other => Self::Other(other),
        }
    }
}

/// The parcel's media settings, parsed from a `ParcelMediaUpdate` and surfaced
/// as [`Event::ParcelMediaUpdate`](crate::Event::ParcelMediaUpdate). This is the streaming media *surface* (the
/// "media" half of a parcel's media/music split); the streaming-audio URL is the
/// separate [`ParcelInfo::music_url`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelMediaUpdateInfo {
    /// The media URL the parcel streams (e.g. an HLS/MP4/web page), [`None`] if
    /// the update cleared it.
    pub media_url: Option<url::Url>,
    /// The texture the media replaces on the parcel surface (`None` if none).
    pub media_id: Option<TextureKey>,
    /// Whether the media is auto-scaled to the surface.
    pub media_auto_scale: bool,
    /// The media MIME type (e.g. `"video/vnd.secondlife.qt.legacy"`,
    /// `"text/html"`); empty if unset.
    pub media_type: String,
    /// The media description; empty if unset.
    pub media_desc: String,
    /// The media surface width in pixels (`None` if unset / native — the `0`
    /// wire sentinel).
    pub media_width: Option<i32>,
    /// The media surface height in pixels (`None` if unset / native — the `0`
    /// wire sentinel).
    pub media_height: Option<i32>,
    /// Whether the media loops.
    pub media_loop: bool,
}

/// A parcel category, the `Category` of a [`ParcelUpdate`] (the parcel's search
/// classification).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ParcelCategory {
    /// No category set.
    #[default]
    None,
    /// A Linden-owned location.
    Linden,
    /// Residential land.
    Residential,
    /// Commercial land.
    Commercial,
    /// Industrial land.
    Industrial,
    /// A park or recreation area.
    ParkAndRecreation,
    /// Anything else.
    Other,
    /// Adult-oriented land.
    Adult,
    /// An unrecognised category value, preserved verbatim.
    Unknown(u8),
}

impl ParcelCategory {
    /// Classifies a parcel-category wire value.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Linden,
            2 => Self::Residential,
            3 => Self::Commercial,
            4 => Self::Industrial,
            5 => Self::ParkAndRecreation,
            6 => Self::Other,
            7 => Self::Adult,
            other => Self::Unknown(other),
        }
    }

    /// The wire value for this category.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Linden => 1,
            Self::Residential => 2,
            Self::Commercial => 3,
            Self::Industrial => 4,
            Self::ParkAndRecreation => 5,
            Self::Other => 6,
            Self::Adult => 7,
            Self::Unknown(value) => value,
        }
    }
}

/// Which parcel access list to query or modify: the allow list or the ban list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParcelAccessScope {
    /// The allow list (`AL_ACCESS`, `0x1`).
    Access,
    /// The ban list (`AL_BAN`, `0x2`).
    Ban,
}

impl ParcelAccessScope {
    /// The access-list flag wire value.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Access => 0x1,
            Self::Ban => 0x2,
        }
    }

    /// Classifies an access-list flag value (preferring `Access` if both bits
    /// are set).
    #[must_use]
    pub const fn from_u32(flags: u32) -> Self {
        if flags & 0x1 != 0 {
            Self::Access
        } else {
            Self::Ban
        }
    }
}

/// The per-entry classification flags (`AL_*`) on one parcel access-list entry.
///
/// A bitfield carried by every `List` entry of a `ParcelAccessListReply`
/// (alongside the whole-list [`ParcelAccessScope`]). On Second Life an entry can
/// be flagged as an experience allow/block in addition to the plain
/// access/ban list it belongs to; OpenSim sets the per-entry flags equal to the
/// list's [`ParcelAccessScope`]. Combine the constants with
/// [`ParcelAccessFlags::union`].
// `ParcelAccessFlags` now lives in `sl_types::parcel`; re-exported here so the
// existing `sl_proto::…` path is unchanged.
pub use sl_types::parcel::ParcelAccessFlags;

/// One entry of a parcel access (allow) or ban list, from an
/// [`Event::ParcelAccessList`](crate::Event::ParcelAccessList) or supplied to
/// [`Session::update_parcel_access_list`](crate::Session::update_parcel_access_list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelAccessEntry {
    /// The agent the entry applies to.
    pub id: Uuid,
    /// The Unix expiry time (`time_t`); `0` means the entry never expires.
    pub time: i32,
    /// The per-entry classification flags (`AL_*`). On a received reply these
    /// carry the entry's access/ban/experience sub-type; when supplied to
    /// [`Session::update_parcel_access_list`](crate::Session::update_parcel_access_list)
    /// they are OR'd onto the list's [`ParcelAccessScope`] (leave
    /// [`ParcelAccessFlags::NONE`] to send just the scope).
    pub flags: ParcelAccessFlags,
}

/// The kinds of objects to return or select on a parcel, as the `ReturnType` of
/// [`Session::return_parcel_objects`](crate::Session::return_parcel_objects) and
/// [`Session::select_parcel_objects`](crate::Session::select_parcel_objects). A
/// bitfield: combine the constants with [`ParcelReturnType::union`].
// `ParcelReturnType` now lives in `sl_types::parcel`; re-exported here so the
// existing `sl_proto::…` path is unchanged.
pub use sl_types::parcel::ParcelReturnType;

/// The settings to apply to a parcel via
/// [`Session::update_parcel`](crate::Session::update_parcel)
/// (`ParcelPropertiesUpdate`). Start from [`ParcelUpdate::default`] and set the
/// fields to change; `local_id` is required (from [`ParcelInfo::local_id`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ParcelUpdate {
    /// The parcel's region-local id (from [`ParcelInfo::local_id`]).
    pub local_id: RegionLocalParcelId,
    /// The parcel flags bitfield to set.
    pub parcel_flags: ParcelFlags,
    /// The sale price in L$ (when [`ParcelFlags::FOR_SALE`] is set).
    pub sale_price: Option<LindenAmount>,
    /// The parcel name.
    pub name: String,
    /// The parcel description.
    pub description: String,
    /// The streaming music URL ([`None`] clears it).
    pub music_url: Option<url::Url>,
    /// The streaming media URL ([`None`] clears it).
    pub media_url: Option<url::Url>,
    /// The media texture id (`None` if none).
    pub media_id: Option<TextureKey>,
    /// Whether to auto-scale the media to the prim face.
    pub media_auto_scale: bool,
    /// The group the parcel is set to (`None` for none).
    pub group_id: Option<GroupKey>,
    /// The price of a parcel pass in L$.
    pub pass_price: LindenAmount,
    /// How many hours a parcel pass lasts.
    pub pass_hours: f32,
    /// The parcel's search category.
    pub category: ParcelCategory,
    /// The only agent allowed to buy the parcel (`None` for anyone).
    pub auth_buyer_id: Option<AgentKey>,
    /// The parcel snapshot texture id (`None` if none).
    pub snapshot_id: Option<TextureKey>,
    /// The teleport-landing location within the parcel.
    pub user_location: RegionCoordinates,
    /// The direction an arriving agent faces at the landing point.
    pub user_look_at: Direction,
    /// The landing type (`0` = blocked, `1` = landing point, `2` = anywhere).
    pub landing_type: u8,
}

impl Default for ParcelUpdate {
    fn default() -> Self {
        Self {
            local_id: RegionLocalParcelId(0),
            parcel_flags: ParcelFlags::from_bits(0),
            sale_price: None,
            name: String::new(),
            description: String::new(),
            music_url: None,
            media_url: None,
            media_id: None,
            media_auto_scale: false,
            group_id: None,
            pass_price: LindenAmount(0),
            pass_hours: 0.0,
            category: ParcelCategory::None,
            auth_buyer_id: None,
            snapshot_id: None,
            user_location: RegionCoordinates::new(0.0, 0.0, 0.0),
            user_look_at: Direction::ZERO,
            landing_type: 0,
        }
    }
}

/// One owner's object tally on a parcel, from a `ParcelObjectOwnersReply` block
/// (the per-owner rows the "Returnable objects" land panel shows). Requested via
/// [`Command::RequestParcelObjectOwners`](crate::Command::RequestParcelObjectOwners)
/// and surfaced as [`Event::ParcelObjectOwners`](crate::Event::ParcelObjectOwners).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelObjectOwner {
    /// The owner of the objects — an agent or a group.
    pub owner: OwnerKey,
    /// How many of this owner's objects sit on the parcel.
    pub count: i32,
    /// Whether the owner is currently online (the grid only fills this for the
    /// estate owner / managers, otherwise `false`).
    pub online_status: bool,
}

/// Which top-objects report a `LandStatReply` carries (`ReportType`): a parcel's
/// or region's top script-using objects, or its top colliding objects. Selected
/// in [`Command::RequestLandStat`](crate::Command::RequestLandStat) and echoed in
/// [`Event::LandStatReply`](crate::Event::LandStatReply). This is the data behind
/// the estate-tools "Top Scripts" / "Top Colliders" panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum LandStatReportType {
    /// The top objects by script resource use (`0`).
    #[default]
    TopScripts,
    /// The top objects by collisions (`1`).
    TopColliders,
    /// An unrecognised report-type value, preserved verbatim.
    Other(u32),
}

impl LandStatReportType {
    /// The raw `ReportType` value for this report.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::TopScripts => 0,
            Self::TopColliders => 1,
            Self::Other(value) => value,
        }
    }

    /// Decodes a raw `ReportType` value.
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        match value {
            0 => Self::TopScripts,
            1 => Self::TopColliders,
            other => Self::Other(other),
        }
    }
}

/// One row of a `LandStatReply` — a single top-scripts / top-colliders object,
/// from a `LandStatReply` `ReportData` block. Surfaced (with the others) as
/// [`Event::LandStatReply`](crate::Event::LandStatReply).
#[derive(Debug, Clone, PartialEq)]
pub struct LandStatItem {
    /// The object's region-local id (`TaskLocalID`).
    pub task_local_id: RegionLocalObjectId,
    /// The object's id (`TaskID`).
    pub task_id: ObjectKey,
    /// The object's region position (`LocationX`/`Y`/`Z`), in metres.
    pub location: RegionCoordinates,
    /// The object's score for this report (`Score`): script time for top-scripts,
    /// collision count for top-colliders.
    pub score: f32,
    /// The object's name (`TaskName`).
    pub task_name: String,
    /// The object owner's name (`OwnerName`).
    pub owner_name: String,
}

/// Basic parcel information from a `ParcelInfoReply` — the condensed listing the
/// places/search panels show for a parcel id (distinct from the full geometry
/// and flags of [`ParcelInfo`], which a `ParcelProperties` carries). Requested by
/// parcel id via [`Command::RequestParcelInfo`](crate::Command::RequestParcelInfo)
/// (the id comes from a `RemoteParcelRequest` capability lookup,
/// [`Command::RequestRemoteParcelId`](crate::Command::RequestRemoteParcelId)) and
/// surfaced as [`Event::ParcelDetails`](crate::Event::ParcelDetails).
#[derive(Debug, Clone, PartialEq)]
pub struct ParcelDetails {
    /// The parcel's grid-wide id (the `parcel_id` the lookup resolves).
    pub parcel_id: ParcelKey,
    /// The parcel owner's agent (or group) id.
    pub owner_id: Uuid,
    /// The parcel name.
    pub name: String,
    /// The parcel description.
    pub description: String,
    /// The actual area in m².
    pub actual_area: LandArea,
    /// The billable area in m².
    pub billable_area: LandArea,
    /// The packed parcel flags byte (a condensed subset of the full
    /// [`ParcelFlags`](sl_wire::ParcelFlags)).
    pub flags: u8,
    /// The parcel anchor's global position, in metres.
    pub global_position: GlobalCoordinates,
    /// The containing region's name, or `None` when the grid sent an empty
    /// (unknown) name.
    pub sim_name: Option<RegionName>,
    /// The parcel snapshot texture id (`None` if none).
    pub snapshot_id: Option<TextureKey>,
    /// The parcel's dwell (traffic) value.
    pub dwell: f32,
    /// The sale price in L$ (when for sale).
    pub sale_price: Option<LindenAmount>,
    /// The auction id (non-zero when the parcel is up for auction).
    pub auction_id: i32,
}

impl Default for ParcelDetails {
    fn default() -> Self {
        Self {
            parcel_id: ParcelKey::from(Uuid::nil()),
            owner_id: Uuid::nil(),
            name: String::new(),
            description: String::new(),
            actual_area: LandArea(0),
            billable_area: LandArea(0),
            flags: 0,
            global_position: GlobalCoordinates::new(0.0, 0.0, 0.0),
            sim_name: None,
            snapshot_id: None,
            dwell: 0.0,
            sale_price: None,
            auction_id: 0,
        }
    }
}
