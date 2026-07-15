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

    /// Anyone may fly over the parcel (the viewer's `PF_ALLOW_FLY` /
    /// `LLParcel::getAllowFly`, one input to [`Session::can_fly`](crate::Session::can_fly)).
    #[must_use]
    pub const fn allow_fly(&self) -> bool {
        self.flags().contains(sl_wire::ParcelFlags::ALLOW_FLY)
    }

    /// Whether the region-local point `(x, y)` (metres) lies on this parcel,
    /// tested against the membership [`bitmap`](Self::bitmap) — one bit per 4×4 m
    /// block, row-major, least-significant-bit first (the parcel's block at
    /// `⌊x/4⌋, ⌊y/4⌋`). The region's blocks-per-edge is derived from the bitmap
    /// length (`isqrt(bits)`) so the test works for both standard 256 m regions
    /// (a 64×64 grid, 512-byte bitmap) and variable-size regions without needing
    /// the region dimensions. Points outside the region or off the bitmap return
    /// `false`. Used by [`Session::current_parcel`](crate::Session::current_parcel)
    /// to find the parcel the agent stands on.
    #[must_use]
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        bitmap_contains_point(&self.bitmap, x, y)
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

/// Whether the region-local point `(x, y)` (metres) lies on the parcel described
/// by a membership `bitmap` — one bit per 4×4 m block, row-major, LSB-first. The
/// region's blocks-per-edge is derived from the bitmap length (`isqrt(bits)`), so
/// it handles standard 256 m regions (64×64, 512 bytes) and variable-size regions
/// alike. Points outside the region or off the bitmap yield `false`. This is the
/// core of [`ParcelInfo::contains_point`].
fn bitmap_contains_point(bitmap: &[u8], x: f32, y: f32) -> bool {
    /// Metres along each edge of one parcel bitmap block.
    const BLOCK_METRES: f32 = 4.0;
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return false;
    }
    let edge = bitmap.len().saturating_mul(8).isqrt();
    let (Some(block_x), Some(block_y)) =
        (block_index(x, BLOCK_METRES), block_index(y, BLOCK_METRES))
    else {
        return false;
    };
    if block_x >= edge || block_y >= edge {
        return false;
    }
    // Bounded above (`block_x`, `block_y` < `edge`, so `bit` < `edge²` ≤ bits), but
    // use saturating ops to satisfy the arithmetic-side-effects lint.
    let bit = block_y.saturating_mul(edge).saturating_add(block_x);
    bitmap
        .get(bit / 8)
        .is_some_and(|byte| byte & (1_u8 << (bit % 8)) != 0)
}

/// The parcel-bitmap block index `⌊coord / block⌋` for a region-local `coord`
/// (metres), or `None` when `coord` is negative, non-finite, or beyond any sane
/// region extent. Kept free of unguarded `as` casts: the quotient is floored and
/// range-checked before the (now exact) conversion.
fn block_index(coord: f32, block: f32) -> Option<usize> {
    /// The largest block index worth considering — far beyond the largest
    /// variable-size region, so anything past it is off the map, not an index.
    const MAX_BLOCK: f32 = 65_536.0;
    let quotient = (coord / block).floor();
    if !(0.0..=MAX_BLOCK).contains(&quotient) {
        return None;
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "quotient is a non-negative finite value bounded to 0..=MAX_BLOCK just above, so the floored block index converts exactly"
    )]
    let index = quotient as usize;
    Some(index)
}

/// A region parcel-ownership overlay chunk, parsed from `ParcelOverlay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelOverlayInfo {
    /// Which of the four overlay chunks this is (0–3).
    pub sequence_id: i32,
    /// The packed overlay bytes: per-square ownership colour and edge/flag bits.
    pub data: Vec<u8>,
}

/// Side length, in metres, of one parcel-overlay grid square. The overlay
/// divides the region into 4 m squares, one packed byte each
/// (`PARCEL_GRID_STEP_METERS` in the reference viewer).
pub const PARCEL_GRID_STEP_METRES: f32 = 4.0;

/// The number of overlay grid squares along each edge of a standard 256 m
/// region (`256 / 4`). Variable-sized regions scale this, but the viewer — like
/// its terrain path — assumes the classic 256 m region.
pub const DEFAULT_GRIDS_PER_EDGE: usize = 64;

/// Bit layout of a packed overlay byte (mirrors the reference viewer's
/// `llparcel.h` `PARCEL_*` constants).
///
/// The low three bits carry the ownership colour class; the high bits are
/// independent flags.
mod overlay_bits {
    /// Mask selecting the ownership colour class (low three bits).
    pub(super) const COLOUR_MASK: u8 = 0x07;
    /// Avatars are hidden to onlookers outside this parcel (`PARCEL_HIDDENAVS`).
    pub(super) const HIDDEN_AVATARS: u8 = 0x10;
    /// Sounds made here are audible only within this parcel
    /// (`PARCEL_SOUND_LOCAL`).
    pub(super) const SOUND_LOCAL: u8 = 0x20;
    /// A parcel boundary runs along this square's western edge
    /// (`PARCEL_WEST_LINE`).
    pub(super) const WEST_LINE: u8 = 0x40;
    /// A parcel boundary runs along this square's southern edge
    /// (`PARCEL_SOUTH_LINE`).
    pub(super) const SOUTH_LINE: u8 = 0x80;
}

/// The ownership colour class of a parcel-overlay square — the low three bits of
/// a packed overlay byte, which decide the colour the map and property overlay
/// paint the square (mirrors the reference viewer's `PARCEL_PUBLIC` … constants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParcelOwnership {
    /// Unowned / public land (`PARCEL_PUBLIC`, `0`).
    Public,
    /// Owned by someone other than you or your active group
    /// (`PARCEL_OWNED`, `1`).
    Owned,
    /// Owned by a group you are a member of (`PARCEL_GROUP`, `2`).
    Group,
    /// Owned by you (`PARCEL_SELF`, `3`).
    SelfOwned,
    /// Advertised for sale (`PARCEL_FOR_SALE`, `4`).
    ForSale,
    /// Up for auction (`PARCEL_AUCTION`, `5`).
    Auction,
    /// An unassigned colour index (`6` or `7`), preserved verbatim.
    Reserved(u8),
}

impl ParcelOwnership {
    /// Classifies the ownership colour index (already masked to the low three
    /// bits).
    #[must_use]
    const fn from_index(index: u8) -> Self {
        match index {
            0 => Self::Public,
            1 => Self::Owned,
            2 => Self::Group,
            3 => Self::SelfOwned,
            4 => Self::ForSale,
            5 => Self::Auction,
            other => Self::Reserved(other),
        }
    }
}

/// One decoded parcel-overlay square: its ownership colour class plus the
/// boundary and sound flags packed into a single overlay byte.
///
/// [`west_line`](Self::west_line) / [`south_line`](Self::south_line) mark the two
/// edges the reference viewer draws property lines along (the other two edges of
/// a parcel are the west/south lines of the neighbouring squares).
/// [`sound_local`](Self::sound_local) is the bit that clamps in-world sound to
/// the parcel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the packed overlay byte carries four independent boolean flags; modelling each as its own field mirrors the wire layout"
)]
pub struct ParcelOverlayCell {
    /// The ownership colour class (low three bits).
    pub ownership: ParcelOwnership,
    /// Avatars here are hidden to onlookers outside the parcel.
    pub hidden_avatars: bool,
    /// Sound made here is audible only within the parcel.
    pub sound_local: bool,
    /// A parcel boundary runs along the square's western edge.
    pub west_line: bool,
    /// A parcel boundary runs along the square's southern edge.
    pub south_line: bool,
}

impl ParcelOverlayCell {
    /// Decodes a single packed overlay byte into its typed fields.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self {
            ownership: ParcelOwnership::from_index(byte & overlay_bits::COLOUR_MASK),
            hidden_avatars: byte & overlay_bits::HIDDEN_AVATARS != 0,
            sound_local: byte & overlay_bits::SOUND_LOCAL != 0,
            west_line: byte & overlay_bits::WEST_LINE != 0,
            south_line: byte & overlay_bits::SOUTH_LINE != 0,
        }
    }
}

/// Why a [`ParcelOverlay`](crate::Event::ParcelOverlay) chunk could not be
/// folded into a [`ParcelOverlayGrid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[expect(
    variant_size_differences,
    reason = "the diagnostic fields on ChunkOutOfRange are worth more than shrinking a rarely-constructed error to match the tiny NegativeSequenceId variant"
)]
pub enum ParcelOverlayError {
    /// The chunk's `sequence_id` was negative; overlay chunks are numbered from
    /// zero.
    #[error("parcel overlay chunk has a negative sequence id ({0})")]
    NegativeSequenceId(i32),
    /// The chunk, placed at `sequence_id × chunk_length`, would run past the end
    /// of the grid — a mismatched region size or a corrupt chunk.
    #[error(
        "parcel overlay chunk {sequence_id} of {length} bytes does not fit a {edge}×{edge} grid"
    )]
    ChunkOutOfRange {
        /// The offending chunk's sequence id.
        sequence_id: i32,
        /// The chunk's byte length.
        length: usize,
        /// The grid's edge length in squares.
        edge: usize,
    },
}

/// Maps a region-local metre coordinate to its 4 m overlay-square index, or
/// `None` if the coordinate is negative or not finite (bounds against the grid
/// edge are the caller's job).
fn grid_square_index(coord: f32) -> Option<usize> {
    if !coord.is_finite() || coord < 0.0 {
        return None;
    }
    let square = (coord / PARCEL_GRID_STEP_METRES).floor();
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "square is a non-negative finite floored value, so the truncating conversion to usize is exact for any in-region coordinate"
    )]
    let square = square as usize;
    Some(square)
}

/// A region's reassembled parcel-ownership overlay: a square grid of
/// [`ParcelOverlayCell`]s built up from the four
/// [`ParcelOverlay`](crate::Event::ParcelOverlay) chunks the simulator pushes on
/// region entry (and re-pushes when parcels are split, joined, or sold).
///
/// Each chunk is a run of packed bytes covering a contiguous band of southern
/// rows (chunk 0 is the southernmost); [`ingest_chunk`](Self::ingest_chunk)
/// copies each into place, so a grid becomes [`complete`](Self::is_complete)
/// once every chunk has arrived. Squares are addressed by `row` (south→north,
/// zero-based) and `col` (west→east), matching the reference viewer's
/// `mOwnership[row * grids_per_edge + col]` layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParcelOverlayGrid {
    /// Number of squares along each edge (64 for a standard 256 m region).
    grids_per_edge: usize,
    /// The packed overlay bytes, row-major (`row * grids_per_edge + col`).
    packed: Vec<u8>,
    /// Which byte positions have been filled by a chunk, for completeness
    /// tracking and idempotent re-ingestion.
    filled: Vec<bool>,
    /// How many byte positions in [`filled`](Self::filled) are set.
    filled_count: usize,
}

impl ParcelOverlayGrid {
    /// Creates an empty grid `grids_per_edge` squares on a side (every square
    /// reads as [`ParcelOwnership::Public`] with no flags until a chunk fills
    /// it).
    #[must_use]
    pub fn new(grids_per_edge: usize) -> Self {
        let count = grids_per_edge.saturating_mul(grids_per_edge);
        Self {
            grids_per_edge,
            packed: vec![0; count],
            filled: vec![false; count],
            filled_count: 0,
        }
    }

    /// Creates an empty grid sized for a region `width_metres` metres on a side
    /// (`width / 4`), or `None` if the width is not a positive, finite multiple
    /// of the 4 m grid step.
    #[must_use]
    pub fn for_region_width_metres(width_metres: f32) -> Option<Self> {
        if !width_metres.is_finite() || width_metres <= 0.0 {
            return None;
        }
        let edge = width_metres / PARCEL_GRID_STEP_METRES;
        if edge.fract() != 0.0 {
            return None;
        }
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "edge is a positive, finite, integral value (its fractional part is zero and it is > 0), so the truncating conversion to usize is exact"
        )]
        let edge = edge as usize;
        Some(Self::new(edge))
    }

    /// The number of squares along each edge of the grid.
    #[must_use]
    pub const fn grids_per_edge(&self) -> usize {
        self.grids_per_edge
    }

    /// Whether every square has been filled by a chunk — i.e. the full set of
    /// overlay chunks has arrived.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.filled_count == self.packed.len()
    }

    /// Folds one overlay chunk into the grid, copying its bytes to the band of
    /// squares starting at `sequence_id × chunk_length`.
    ///
    /// Re-ingesting a chunk simply overwrites it (the simulator re-pushes the
    /// whole overlay after an edit), leaving [`is_complete`](Self::is_complete)
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ParcelOverlayError`] if the sequence id is negative or the
    /// chunk would not fit the grid.
    pub fn ingest_chunk(
        &mut self,
        sequence_id: i32,
        data: &[u8],
    ) -> Result<(), ParcelOverlayError> {
        let chunk = usize::try_from(sequence_id)
            .map_err(|_ignored| ParcelOverlayError::NegativeSequenceId(sequence_id))?;
        let out_of_range = || ParcelOverlayError::ChunkOutOfRange {
            sequence_id,
            length: data.len(),
            edge: self.grids_per_edge,
        };
        let offset = chunk.checked_mul(data.len()).ok_or_else(out_of_range)?;
        let end = offset.checked_add(data.len()).ok_or_else(out_of_range)?;
        let destination = self.packed.get_mut(offset..end).ok_or_else(out_of_range)?;
        destination.copy_from_slice(data);
        if let Some(flags) = self.filled.get_mut(offset..end) {
            for flag in flags {
                if !*flag {
                    *flag = true;
                    self.filled_count = self.filled_count.saturating_add(1);
                }
            }
        }
        Ok(())
    }

    /// The decoded square at `row` (south→north) and `col` (west→east), or
    /// `None` if either coordinate is off the grid.
    ///
    /// A square that no chunk has filled yet decodes as the zero byte
    /// ([`ParcelOwnership::Public`], no flags).
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Option<ParcelOverlayCell> {
        if col >= self.grids_per_edge || row >= self.grids_per_edge {
            return None;
        }
        let index = row.checked_mul(self.grids_per_edge)?.checked_add(col)?;
        self.packed
            .get(index)
            .copied()
            .map(ParcelOverlayCell::from_byte)
    }

    /// The decoded square covering the region-local point `(x, y)` in metres
    /// (`x` east, `y` north), or `None` if the point lies outside the region.
    #[must_use]
    pub fn cell_at_region_local(&self, x: f32, y: f32) -> Option<ParcelOverlayCell> {
        let col = grid_square_index(x)?;
        let row = grid_square_index(y)?;
        self.cell(row, col)
    }

    /// Iterates every square in row-major order (south→north, then west→east),
    /// yielding `(row, col, cell)` — the map/minimap consumer's entry point.
    pub fn cells(&self) -> impl Iterator<Item = (usize, usize, ParcelOverlayCell)> + '_ {
        let edge = self.grids_per_edge;
        (0..edge).flat_map(move |row| {
            (0..edge).filter_map(move |col| self.cell(row, col).map(|cell| (row, col, cell)))
        })
    }
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::expect_used,
        reason = "a failed expectation is the intended failure signal in a unit test"
    )]

    use pretty_assertions::assert_eq;

    use super::bitmap_contains_point;

    /// A 64×64-block (standard 256 m region) membership bitmap with a single block
    /// set at `(block_x, block_y)`; the 4096-bit map is 512 bytes.
    fn one_block_bitmap(block_x: usize, block_y: usize) -> Vec<u8> {
        let mut bitmap = vec![0_u8; 512];
        let bit = block_y.saturating_mul(64).saturating_add(block_x);
        // The one block owned by this parcel.
        if let Some(byte) = bitmap.get_mut(bit / 8) {
            *byte |= 1_u8 << (bit % 8);
        }
        bitmap
    }

    /// A point inside the owned 4×4 m block is on the parcel; the block's whole
    /// 4 m span (and only it) counts.
    #[test]
    fn a_point_in_the_owned_block_is_on_the_parcel() {
        // Block (2, 3) spans x ∈ [8, 12), y ∈ [12, 16).
        let bitmap = one_block_bitmap(2, 3);
        assert!(bitmap_contains_point(&bitmap, 8.0, 12.0));
        assert!(bitmap_contains_point(&bitmap, 11.9, 15.9));
        // Just outside the block on every side.
        assert!(!bitmap_contains_point(&bitmap, 7.9, 13.0));
        assert!(!bitmap_contains_point(&bitmap, 12.0, 13.0));
        assert!(!bitmap_contains_point(&bitmap, 10.0, 11.9));
        assert!(!bitmap_contains_point(&bitmap, 10.0, 16.0));
    }

    /// The block index is row-major (`x + y*edge`), so swapping x and y picks a
    /// different block — a regression guard against transposed indexing.
    #[test]
    fn indexing_is_row_major_not_transposed() {
        let bitmap = one_block_bitmap(1, 5);
        // The owned block (1, 5): x ∈ [4, 8), y ∈ [20, 24).
        assert!(bitmap_contains_point(&bitmap, 5.0, 21.0));
        // The transposed point (block (5, 1)) is a different, unowned block.
        assert!(!bitmap_contains_point(&bitmap, 21.0, 5.0));
    }

    /// Points off the region, negative, or non-finite are never on a parcel, and
    /// an empty bitmap owns nothing.
    #[test]
    fn out_of_range_and_degenerate_inputs_are_rejected() {
        let bitmap = one_block_bitmap(0, 0);
        assert!(bitmap_contains_point(&bitmap, 0.0, 0.0));
        // Negative / non-finite coordinates.
        assert!(!bitmap_contains_point(&bitmap, -1.0, 0.0));
        assert!(!bitmap_contains_point(&bitmap, 0.0, f32::NAN));
        assert!(!bitmap_contains_point(&bitmap, f32::INFINITY, 0.0));
        // Past the 256 m region edge (block ≥ 64).
        assert!(!bitmap_contains_point(&bitmap, 256.0, 0.0));
        // An empty bitmap has a zero edge and owns nothing.
        assert!(!bitmap_contains_point(&[], 0.0, 0.0));
    }

    /// The blocks-per-edge is derived from the bitmap length, so a smaller region
    /// (here 32×32 blocks = 128 bytes, a 128 m region) indexes correctly.
    #[test]
    fn edge_is_derived_from_bitmap_length() {
        // 32×32 blocks → 1024 bits → 128 bytes. Own block (10, 20).
        let edge = 32_usize;
        let mut bitmap = vec![0_u8; edge.saturating_mul(edge) / 8];
        let bit = 20_usize.saturating_mul(edge).saturating_add(10);
        if let Some(byte) = bitmap.get_mut(bit / 8) {
            *byte |= 1_u8 << (bit % 8);
        }
        // Block (10, 20): x ∈ [40, 44), y ∈ [80, 84).
        assert!(bitmap_contains_point(&bitmap, 41.0, 81.0));
        // The same block coordinates read as a 64-edge region would land on a
        // different byte/bit, so this confirms the edge came from the length.
        assert!(!bitmap_contains_point(&bitmap, 41.0, 41.0));
    }

    use super::{ParcelOverlayError, ParcelOverlayGrid, ParcelOwnership};

    /// Every overlay byte decodes into its colour class and the four independent
    /// high-bit flags.
    #[test]
    fn a_packed_byte_decodes_into_class_and_flags() {
        let mut grid = ParcelOverlayGrid::new(2);
        // 0x03 = PARCEL_SELF, 0x20 = SOUND_LOCAL, 0x40 = WEST_LINE.
        grid.ingest_chunk(0, &[0x03 | 0x20 | 0x40, 0x00, 0x00, 0x00])
            .expect("a full-grid chunk fits");
        let cell = grid.cell(0, 0).expect("(0, 0) is on the grid");
        assert_eq!(cell.ownership, ParcelOwnership::SelfOwned);
        assert!(cell.sound_local);
        assert!(cell.west_line);
        assert!(!cell.south_line);
        assert!(!cell.hidden_avatars);
        // The remaining squares are the zero byte: public, no flags.
        let empty = grid.cell(1, 1).expect("(1, 1) is on the grid");
        assert_eq!(empty.ownership, ParcelOwnership::Public);
        assert!(!empty.sound_local);
    }

    /// The four southern-band chunks reassemble into a complete 64×64 grid, and
    /// squares are addressed row-major with row 0 the southern edge.
    #[test]
    fn four_chunks_reassemble_a_complete_grid() {
        let mut grid = ParcelOverlayGrid::new(64);
        assert!(!grid.is_complete());
        // Chunk c owns rows [16c, 16c+16); tag each chunk's squares with a
        // distinct ownership class so the row→chunk mapping is observable.
        let classes = [0x01_u8, 0x02, 0x03, 0x04];
        for (sequence, &class) in classes.iter().enumerate() {
            let seq = i32::try_from(sequence).expect("0..4 fits i32");
            grid.ingest_chunk(seq, &vec![class; 1024])
                .expect("each 1024-byte chunk is one southern band");
            assert_eq!(grid.is_complete(), sequence == 3);
        }
        // Row 0 (south) came from chunk 0, row 63 (north) from chunk 3.
        assert_eq!(
            grid.cell(0, 0).expect("on grid").ownership,
            ParcelOwnership::Owned
        );
        assert_eq!(
            grid.cell(63, 63).expect("on grid").ownership,
            ParcelOwnership::ForSale
        );
        // The boundary between chunk 1 (rows 16..32) and chunk 2 (rows 32..48).
        assert_eq!(
            grid.cell(31, 0).expect("on grid").ownership,
            ParcelOwnership::Group
        );
        assert_eq!(
            grid.cell(32, 0).expect("on grid").ownership,
            ParcelOwnership::SelfOwned
        );
    }

    /// `cell_at_region_local` floors metre coordinates onto the 4 m grid; off the
    /// region it returns `None`.
    #[test]
    fn a_region_local_point_maps_to_its_4m_square() {
        let mut grid = ParcelOverlayGrid::new(64);
        // (x, y) = (10, 5) → col floor(10/4)=2, row floor(5/4)=1.
        // Fill the whole southern band so (row 1, col 2) is observable.
        let mut band = vec![0x00_u8; 1024];
        if let Some(byte) = band.get_mut(64 + 2) {
            *byte = 0x01; // PARCEL_OWNED at row 1, col 2.
        }
        grid.ingest_chunk(0, &band).expect("fits");
        assert_eq!(
            grid.cell_at_region_local(10.0, 5.0)
                .expect("inside the region")
                .ownership,
            ParcelOwnership::Owned
        );
        // Off the region edge / negative / non-finite.
        assert!(grid.cell_at_region_local(256.0, 0.0).is_none());
        assert!(grid.cell_at_region_local(-1.0, 0.0).is_none());
        assert!(grid.cell_at_region_local(0.0, f32::NAN).is_none());
    }

    /// A negative sequence id and a chunk that runs past the grid are both
    /// rejected, leaving the grid untouched.
    #[test]
    fn malformed_chunks_are_rejected() {
        let mut grid = ParcelOverlayGrid::new(2); // 4 squares total.
        assert_eq!(
            grid.ingest_chunk(-1, &[0]),
            Err(ParcelOverlayError::NegativeSequenceId(-1))
        );
        // Chunk 2 of a 3-byte chunk starts at offset 6, past the 4-square grid.
        assert!(matches!(
            grid.ingest_chunk(2, &[0, 0, 0]),
            Err(ParcelOverlayError::ChunkOutOfRange { .. })
        ));
        assert!(!grid.is_complete());
    }

    /// Re-ingesting the whole overlay (as the simulator does after a parcel
    /// edit) overwrites in place without disturbing completeness.
    #[test]
    fn re_ingesting_overwrites_without_double_counting() {
        let mut grid = ParcelOverlayGrid::new(2);
        grid.ingest_chunk(0, &[0x01, 0x01, 0x01, 0x01])
            .expect("fits");
        assert!(grid.is_complete());
        grid.ingest_chunk(0, &[0x03, 0x03, 0x03, 0x03])
            .expect("fits");
        assert!(grid.is_complete());
        assert_eq!(
            grid.cell(0, 0).expect("on grid").ownership,
            ParcelOwnership::SelfOwned
        );
    }

    /// A grid sized from a region width divides by the 4 m step; non-multiples
    /// and degenerate widths are rejected.
    #[test]
    fn a_grid_sizes_from_region_width() {
        assert_eq!(
            ParcelOverlayGrid::for_region_width_metres(256.0)
                .expect("256 is a multiple of 4")
                .grids_per_edge(),
            64
        );
        assert!(ParcelOverlayGrid::for_region_width_metres(250.0).is_none());
        assert!(ParcelOverlayGrid::for_region_width_metres(0.0).is_none());
        assert!(ParcelOverlayGrid::for_region_width_metres(f32::NAN).is_none());
    }
}
