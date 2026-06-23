//! The search / directory system: the `Dir*Query` requests and their per-category
//! replies, plus avatar-picker autocomplete and the `PlacesQuery` land-holdings
//! lookup.
//!
//! A viewer's *Search* floater drives these. `DirFindQuery` is the unified
//! people / groups / events query whose [`DirFindFlags`] select what is being
//! searched (and how the reply is sorted); `DirPlacesQuery`, `DirLandQuery` and
//! `DirClassifiedQuery` are the dedicated places / land-for-sale / classifieds
//! queries. Each query carries a client-chosen `query_id` echoed back in its
//! reply so the caller can correlate them. `AvatarPickerRequest` is the name
//! autocomplete behind the avatar picker, and `PlacesQuery` (distinct from the
//! directory) lists an agent's or group's land holdings.

use crate::types::{LandArea, ParcelCategory};
use sl_types::key::{AgentKey, ClassifiedKey, GroupKey, ParcelKey, TextureKey};
use sl_types::money::LindenAmount;
use uuid::Uuid;

/// The directory-query flags (`DFQ_*`), shared by every `Dir*Query` and
/// `PlacesQuery`: a bitfield selecting what a `DirFindQuery` searches
/// (people / events / groups), which results to include, and how they are
/// sorted. Combine the constants with [`union`](Self::union).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirFindFlags(pub u32);

impl DirFindFlags {
    /// No flags set.
    pub const NONE: Self = Self(0);
    /// Search people (`DFQ_PEOPLE`, `1 << 0`); the reply is a
    /// [`DirPeopleReply`](crate::Event::DirPeopleReply).
    pub const PEOPLE: Self = Self(1 << 0);
    /// Restrict a people search to online avatars (`DFQ_ONLINE`, `1 << 1`).
    pub const ONLINE: Self = Self(1 << 1);
    /// Search events (`DFQ_EVENTS`, `1 << 3`); the reply is a
    /// [`DirEventsReply`](crate::Event::DirEventsReply).
    pub const EVENTS: Self = Self(1 << 3);
    /// Search groups (`DFQ_GROUPS`, `1 << 4`); the reply is a
    /// [`DirGroupsReply`](crate::Event::DirGroupsReply).
    pub const GROUPS: Self = Self(1 << 4);
    /// The event query's `QueryText` is a date offset (`DFQ_DATE_EVENTS`,
    /// `1 << 5`).
    pub const DATE_EVENTS: Self = Self(1 << 5);
    /// Limit a land query to agent-owned parcels (`DFQ_AGENT_OWNED`, `1 << 6`).
    pub const AGENT_OWNED: Self = Self(1 << 6);
    /// Limit a land query to parcels for sale (`DFQ_FOR_SALE`, `1 << 7`).
    pub const FOR_SALE: Self = Self(1 << 7);
    /// Limit a land query to group-owned parcels (`DFQ_GROUP_OWNED`, `1 << 8`).
    pub const GROUP_OWNED: Self = Self(1 << 8);
    /// Sort places/land results by dwell (`DFQ_DWELL_SORT`, `1 << 10`).
    pub const DWELL_SORT: Self = Self(1 << 10);
    /// Include PG sims only (`DFQ_PG_SIMS_ONLY`, `1 << 11`).
    pub const PG_SIMS_ONLY: Self = Self(1 << 11);
    /// Results with a picture only (`DFQ_PICTURES_ONLY`, `1 << 12`).
    pub const PICTURES_ONLY: Self = Self(1 << 12);
    /// Include PG events only (`DFQ_PG_EVENTS_ONLY`, `1 << 13`).
    pub const PG_EVENTS_ONLY: Self = Self(1 << 13);
    /// Include mature sims only (`DFQ_MATURE_SIMS_ONLY`, `1 << 14`).
    pub const MATURE_SIMS_ONLY: Self = Self(1 << 14);
    /// Sort ascending rather than descending (`DFQ_SORT_ASC`, `1 << 15`).
    pub const SORT_ASC: Self = Self(1 << 15);
    /// Sort land results by price (`DFQ_PRICE_SORT`, `1 << 16`).
    pub const PRICE_SORT: Self = Self(1 << 16);
    /// Sort land results by price per metre (`DFQ_PER_METER_SORT`, `1 << 17`).
    pub const PER_METER_SORT: Self = Self(1 << 17);
    /// Sort land results by area (`DFQ_AREA_SORT`, `1 << 18`).
    pub const AREA_SORT: Self = Self(1 << 18);
    /// Sort results by name (`DFQ_NAME_SORT`, `1 << 19`).
    pub const NAME_SORT: Self = Self(1 << 19);
    /// Apply the land query's price limit (`DFQ_LIMIT_BY_PRICE`, `1 << 20`).
    pub const LIMIT_BY_PRICE: Self = Self(1 << 20);
    /// Apply the land query's area limit (`DFQ_LIMIT_BY_AREA`, `1 << 21`).
    pub const LIMIT_BY_AREA: Self = Self(1 << 21);
    /// Filter out mature content (`DFQ_FILTER_MATURE`, `1 << 22`).
    pub const FILTER_MATURE: Self = Self(1 << 22);
    /// Include PG parcels only (`DFQ_PG_PARCELS_ONLY`, `1 << 23`).
    pub const PG_PARCELS_ONLY: Self = Self(1 << 23);
    /// Include PG-rated results (`DFQ_INC_PG`, `1 << 24`).
    pub const INC_PG: Self = Self(1 << 24);
    /// Include mature-rated results (`DFQ_INC_MATURE`, `1 << 25`).
    pub const INC_MATURE: Self = Self(1 << 25);
    /// Include adult-rated results (`DFQ_INC_ADULT`, `1 << 26`).
    pub const INC_ADULT: Self = Self(1 << 26);
    /// Include adult sims only (`DFQ_ADULT_SIMS_ONLY`, `1 << 27`).
    pub const ADULT_SIMS_ONLY: Self = Self(1 << 27);

    /// Wraps a raw flags value.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw flags value.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Combines two sets of flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The land-for-sale sale-type filter (`ST_*`) of a `DirLandQuery`: a bitfield
/// selecting which sale categories to include. Combine the constants with
/// [`union`](Self::union).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LandSearchType(pub u32);

impl LandSearchType {
    /// Include auctioned land (`ST_AUCTION`, `1 << 1`).
    pub const AUCTION: Self = Self(1 << 1);
    /// Include newbie/first-land parcels (`ST_NEWBIE`, `1 << 2`).
    pub const NEWBIE: Self = Self(1 << 2);
    /// Include mainland parcels (`ST_MAINLAND`, `1 << 3`).
    pub const MAINLAND: Self = Self(1 << 3);
    /// Include estate parcels (`ST_ESTATE`, `1 << 4`).
    pub const ESTATE: Self = Self(1 << 4);
    /// Include every sale type (`ST_ALL`, all bits set).
    pub const ALL: Self = Self(0xFFFF_FFFF);

    /// Wraps a raw sale-type value.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw sale-type value.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Combines two sets of sale-type flags.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl Default for LandSearchType {
    /// The default land filter is [`ALL`](Self::ALL), matching the viewer's
    /// initial state.
    fn default() -> Self {
        Self::ALL
    }
}

/// One person matched by a `DirFindQuery` with [`DirFindFlags::PEOPLE`], carried
/// in a [`DirPeopleReply`](crate::Event::DirPeopleReply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPeopleResult {
    /// The matched avatar.
    pub agent_id: AgentKey,
    /// The avatar's first (legacy) name.
    pub first_name: String,
    /// The avatar's last (legacy) name.
    pub last_name: String,
    /// A legacy "group" string (unused by modern grids; usually empty).
    pub group: String,
    /// Whether the avatar is currently online.
    pub online: bool,
    /// A legacy reputation score (unused by modern grids; usually `0`).
    pub reputation: i32,
}

/// One group matched by a `DirFindQuery` with [`DirFindFlags::GROUPS`], carried
/// in a [`DirGroupsReply`](crate::Event::DirGroupsReply).
#[derive(Debug, Clone, PartialEq)]
pub struct DirGroupResult {
    /// The matched group.
    pub group_id: GroupKey,
    /// The group's name.
    pub group_name: String,
    /// The group's member count.
    pub members: i32,
    /// The search-ranking score the dataserver assigned the match.
    pub search_order: f32,
}

/// The id of an in-world scheduled **event** in the Second Life *events
/// directory* (Search → Events) — the numeric handle a `DirEventResult` carries
/// and `EventInfoRequest`/`EventNotificationAddRequest` reference.
///
/// Unlike the UUID-based ids elsewhere this is a 32-bit integer on the wire (the
/// reference viewer's event `U32`, *not* an `LLUUID`), so the shared
/// `sl_types::key::EventKey` (a UUID wrapper) does not fit. It lives here as a
/// repo-local newtype rather than a bare `u32` so an events-directory id can't be
/// transposed with any other 32-bit field. Not to be confused with the
/// [`Event`](crate::Event) dispatch enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct EventId(pub u32);

impl EventId {
    /// Builds an event-directory id from its raw `u32` wire value.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw `u32` wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for EventId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One event matched by a `DirFindQuery` with [`DirFindFlags::EVENTS`], carried
/// in a [`DirEventsReply`](crate::Event::DirEventsReply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEventResult {
    /// The event owner.
    pub owner_id: Uuid,
    /// The event's name.
    pub name: String,
    /// The event id (used with the events directory, e.g. `EventInfoRequest`).
    pub event_id: EventId,
    /// The event's date, as the human-readable string the dataserver formats.
    pub date: String,
    /// The event's start time, as a Unix timestamp (seconds).
    pub unix_time: u32,
    /// The event flags (e.g. mature/adult; `EVENT_FLAG_*`).
    pub event_flags: u32,
}

/// One classified matched by a [`DirClassifiedQuery`](crate::Command::DirClassifiedQuery),
/// carried in a [`DirClassifiedReply`](crate::Event::DirClassifiedReply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirClassifiedResult {
    /// The classified ad's id (use with `ClassifiedInfoRequest` for full detail).
    pub classified_id: ClassifiedKey,
    /// The classified's name.
    pub name: String,
    /// The classified flags (e.g. mature; `CLASSIFIED_FLAG_*`).
    pub classified_flags: u8,
    /// When the classified was created, as a Unix timestamp (seconds).
    pub creation_date: u32,
    /// When the classified expires, as a Unix timestamp (seconds).
    pub expiration_date: u32,
    /// The weekly L$ the owner pays to list the classified.
    pub price_for_listing: LindenAmount,
}

/// One place matched by a [`DirPlacesQuery`](crate::Command::DirPlacesQuery),
/// carried in a [`DirPlacesReply`](crate::Event::DirPlacesReply).
#[derive(Debug, Clone, PartialEq)]
pub struct DirPlaceResult {
    /// The matched parcel.
    pub parcel_id: ParcelKey,
    /// The parcel's name.
    pub name: String,
    /// Whether the parcel is for sale.
    pub for_sale: bool,
    /// Whether the parcel is being auctioned.
    pub auction: bool,
    /// The parcel's dwell (traffic) score.
    pub dwell: f32,
}

/// One land parcel matched by a [`DirLandQuery`](crate::Command::DirLandQuery),
/// carried in a [`DirLandReply`](crate::Event::DirLandReply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirLandResult {
    /// The matched parcel.
    pub parcel_id: ParcelKey,
    /// The parcel's name.
    pub name: String,
    /// Whether the parcel is being auctioned.
    pub auction: bool,
    /// Whether the parcel is for sale.
    pub for_sale: bool,
    /// The parcel's asking price in L$ when [`for_sale`](Self::for_sale), or
    /// `None` when it is not for sale. A for-sale parcel may still be free
    /// (`Some(LindenAmount(0))`).
    pub sale_price: Option<LindenAmount>,
    /// The parcel's area, in square metres.
    pub actual_area: LandArea,
}

/// One name matched by an [`AvatarPickerRequest`](crate::Command::AvatarPickerRequest),
/// carried in an [`AvatarPickerReply`](crate::Event::AvatarPickerReply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarPickerResult {
    /// The matched avatar.
    pub avatar_id: AgentKey,
    /// The avatar's first (legacy) name.
    pub first_name: String,
    /// The avatar's last (legacy) name.
    pub last_name: String,
}

/// One land holding returned by a [`PlacesQuery`](crate::Command::PlacesQuery),
/// carried in a [`PlacesReply`](crate::Event::PlacesReply). The `PlacesQuery`
/// drives the land-holdings panels (an agent's or a group's parcels), distinct
/// from the directory search above.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacesResult {
    /// The parcel's owner.
    pub owner_id: Uuid,
    /// The parcel's name.
    pub name: String,
    /// The parcel's description.
    pub description: String,
    /// The parcel's actual area, in square metres.
    pub actual_area: LandArea,
    /// The parcel's billable area, in square metres.
    pub billable_area: LandArea,
    /// The parcel flags byte.
    pub flags: u8,
    /// The parcel's global position, in metres (`(x, y, z)`).
    pub global_position: (f32, f32, f32),
    /// The name of the region the parcel is in.
    pub sim_name: String,
    /// The parcel's snapshot texture.
    pub snapshot_id: TextureKey,
    /// The parcel's dwell (traffic) score.
    pub dwell: f32,
    /// The parcel's price, in L$.
    pub price: LindenAmount,
}

/// The full detail of a single in-world event, carried in an
/// [`EventInfoReply`](crate::Event::EventInfoReply) in response to an
/// [`EventInfoRequest`](crate::Command::EventInfoRequest). The event id comes
/// from a [`DirEventResult`] of an events `DirFindQuery`, or from the events
/// directory; this fills in the rest of the listing.
#[derive(Debug, Clone, PartialEq)]
pub struct EventInfo {
    /// The event id (the same id passed to `EventInfoRequest`).
    pub event_id: EventId,
    /// The avatar running the event (the viewer parses the `Creator` string as a
    /// UUID; a non-UUID value reads as [`Uuid::nil`](uuid::Uuid::nil)).
    pub creator: AgentKey,
    /// The event's name.
    pub name: String,
    /// The event's category (a human-readable label, e.g. `"Discussion"`).
    pub category: String,
    /// The event's description.
    pub description: String,
    /// The event's start time, as the human-readable string the dataserver
    /// formats.
    pub date: String,
    /// The event's start time, as a Unix timestamp (seconds).
    pub date_utc: u32,
    /// The event's duration, in minutes.
    pub duration: u32,
    /// Whether a cover charge applies (non-zero) and, with it, the legacy cover
    /// flag the dataserver sets.
    pub cover: u32,
    /// The cover charge, in L$: `Some` when a cover charge applies (i.e.
    /// [`cover`](Self::cover) is non-zero), `None` otherwise. On the wire `None`
    /// is the `0` no-cover sentinel.
    pub amount: Option<LindenAmount>,
    /// The name of the region the event is in.
    pub sim_name: String,
    /// The event's global position, in metres (`(x, y, z)`).
    pub global_position: (f64, f64, f64),
    /// The event flags (e.g. mature/adult; `EVENT_FLAG_*`).
    pub flags: u32,
}

/// Converts a [`ParcelCategory`] to the signed wire byte the `*PlacesQuery`
/// `Category` field uses.
#[must_use]
pub(crate) const fn category_to_wire(category: ParcelCategory) -> i8 {
    category.to_u8().cast_signed()
}

/// Converts the signed wire byte of a `*PlacesQuery` `Category` field to a
/// [`ParcelCategory`].
#[must_use]
pub(crate) const fn category_from_wire(value: i8) -> ParcelCategory {
    ParcelCategory::from_u8(value.cast_unsigned())
}

#[cfg(test)]
mod tests {
    use super::{DirFindFlags, EventId, LandSearchType, category_from_wire, category_to_wire};
    use crate::types::ParcelCategory;
    use pretty_assertions::assert_eq;

    /// The events-directory [`EventId`] is a transparent `u32` wrapper:
    /// `new`/`get` round-trip the raw wire value and `Display` matches the bare
    /// integer, so the typed id puts the exact same bytes on the wire.
    #[test]
    fn event_id_round_trips_raw_u32() {
        assert_eq!(EventId::new(424_242).get(), 424_242);
        assert_eq!(EventId::default(), EventId::new(0));
        assert_eq!(EventId::new(7).to_string(), "7");
    }

    /// The find-flag bitfield combines and tests bits as expected.
    #[test]
    fn find_flags_union_and_contains() {
        let flags = DirFindFlags::PEOPLE.union(DirFindFlags::ONLINE);
        assert!(flags.contains(DirFindFlags::PEOPLE));
        assert!(flags.contains(DirFindFlags::ONLINE));
        assert!(!flags.contains(DirFindFlags::GROUPS));
        assert_eq!(flags.bits(), 0b11);
        assert!(DirFindFlags::NONE.is_empty());
    }

    /// The default land filter includes everything.
    #[test]
    fn land_search_type_default_is_all() {
        assert_eq!(LandSearchType::default(), LandSearchType::ALL);
        assert!(LandSearchType::ALL.contains(LandSearchType::AUCTION));
    }

    /// A parcel category round-trips through the signed wire byte (including a
    /// value with the high bit set).
    #[test]
    fn category_round_trips() {
        for category in [
            ParcelCategory::None,
            ParcelCategory::Linden,
            ParcelCategory::Adult,
            ParcelCategory::Unknown(200),
        ] {
            assert_eq!(category_from_wire(category_to_wire(category)), category);
        }
    }
}
