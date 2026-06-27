//! Avatar profile and relationships: properties, picks, classifieds, friends.

use super::Maturity;
use sl_types::key::{
    AgentKey, ClassifiedKey, FriendKey, GroupKey, InventoryFolderKey, ParcelKey, TextureKey,
};
use sl_types::map::RegionName;
use sl_types::money::LindenAmount;
use sl_types::search::ClassifiedCategory;
use sl_wire::{Direction, GlobalCoordinates};
use uuid::Uuid;

// `PickKey` (a profile pick id) now lives in `sl_types::key`; re-exported here
// so the existing `sl_proto::…` path is unchanged.
pub use sl_types::key::PickKey;

/// An avatar's profile properties, parsed from `AvatarPropertiesReply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarProperties {
    /// The avatar the profile is about.
    pub avatar_id: AgentKey,
    /// The "second life" profile image (texture id).
    pub image_id: TextureKey,
    /// The "first life" profile image (texture id).
    pub fl_image_id: TextureKey,
    /// The avatar's partner, or `None` if they have none.
    pub partner_id: Option<AgentKey>,
    /// The "second life" about text.
    pub about_text: String,
    /// The "first life" about text.
    pub fl_about_text: String,
    /// The account creation date, as the grid's display string (e.g. `2008-01-15`).
    pub born_on: String,
    /// The web profile URL, if any.
    pub profile_url: String,
    /// The charter-member / account-title field (grid-specific; often empty).
    pub charter_member: String,
    /// The raw account/profile flags bitfield.
    pub flags: u32,
}

/// An avatar's interests, parsed from `AvatarInterestsReply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarInterests {
    /// The avatar the interests are about.
    pub avatar_id: AgentKey,
    /// The "want to" category bitmask.
    pub want_to_mask: u32,
    /// The "want to" free text.
    pub want_to_text: String,
    /// The "skills" category bitmask.
    pub skills_mask: u32,
    /// The "skills" free text.
    pub skills_text: String,
    /// The languages free text.
    pub languages_text: String,
}

/// One group listed in an avatar's profile, from an `AvatarGroupsReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarGroupMembership {
    /// The group id.
    pub group_id: GroupKey,
    /// The group name.
    pub group_name: String,
    /// The avatar's title in the group.
    pub group_title: String,
    /// The avatar's group powers bitfield.
    pub group_powers: u64,
    /// Whether the avatar accepts notices from the group.
    pub accept_notices: bool,
    /// The group's insignia (texture id).
    pub group_insignia_id: TextureKey,
}

/// One pick from an `AvatarPicksReply` (header data only: id and name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarPick {
    /// The pick id (use to fetch full details).
    pub pick_id: PickKey,
    /// The pick name.
    pub name: String,
}

/// One classified ad from an `AvatarClassifiedReply` (header data only: id and
/// name). Fetch the full details with
/// [`Session::request_classified_info`](crate::Session::request_classified_info).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarClassified {
    /// The classified id (use to fetch full details).
    pub classified_id: ClassifiedKey,
    /// The classified name.
    pub name: String,
}

/// The full details of one pick, parsed from `PickInfoReply` in response to
/// [`Session::request_pick_info`](crate::Session::request_pick_info).
#[derive(Debug, Clone, PartialEq)]
pub struct PickInfo {
    /// The pick id.
    pub pick_id: PickKey,
    /// The avatar that created the pick.
    pub creator_id: AgentKey,
    /// Whether this is a "top pick" (a god-only legacy flag, normally `false`).
    pub top_pick: bool,
    /// The parcel the pick points at.
    pub parcel_id: ParcelKey,
    /// The pick name.
    pub name: String,
    /// The pick description.
    pub description: String,
    /// The pick snapshot texture id.
    pub snapshot_id: Option<TextureKey>,
    /// The owner's account name, as the grid resolves it.
    pub user: String,
    /// The parcel's original name.
    pub original_name: String,
    /// The region name the pick is in, or `None` when the grid sent an empty
    /// (unknown) name.
    pub sim_name: Option<RegionName>,
    /// The pick's global position (metres, grid-wide coordinates).
    pub pos_global: GlobalCoordinates,
    /// The sort order (only meaningful for top picks).
    pub sort_order: i32,
    /// Whether the pick is enabled (shown in the profile).
    pub enabled: bool,
}

/// The full details of one classified ad, parsed from `ClassifiedInfoReply` in
/// response to
/// [`Session::request_classified_info`](crate::Session::request_classified_info).
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedInfo {
    /// The classified id.
    pub classified_id: ClassifiedKey,
    /// The avatar that created the classified.
    pub creator_id: AgentKey,
    /// The creation date (Unix timestamp, seconds).
    pub creation_date: u32,
    /// The expiration date (Unix timestamp, seconds).
    pub expiration_date: u32,
    /// The classified's search category.
    pub category: ClassifiedCategory,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at.
    pub parcel_id: ParcelKey,
    /// The parent estate id.
    pub parent_estate: u32,
    /// The classified snapshot texture id.
    pub snapshot_id: Option<TextureKey>,
    /// The region name the classified is in, or `None` when the grid sent an
    /// empty (unknown) name.
    pub sim_name: Option<RegionName>,
    /// The classified's global position (metres, grid-wide coordinates).
    pub pos_global: GlobalCoordinates,
    /// The parcel name.
    pub parcel_name: String,
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount paid to list this classified (L$).
    pub price_for_listing: LindenAmount,
}

/// An update to the agent's own profile, sent via
/// [`Session::update_profile`](crate::Session::update_profile)
/// (`AvatarPropertiesUpdate`). This replaces the whole profile, so read the
/// current values with
/// [`Session::request_avatar_properties`](crate::Session::request_avatar_properties)
/// first and edit from there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUpdate {
    /// The "second life" profile image (texture id).
    pub image_id: TextureKey,
    /// The "first life" profile image (texture id).
    pub fl_image_id: TextureKey,
    /// The "second life" about text.
    pub about_text: String,
    /// The "first life" about text.
    pub fl_about_text: String,
    /// Whether the profile may be published in search.
    pub allow_publish: bool,
    /// Whether the profile is flagged as "mature".
    pub mature_publish: bool,
    /// The web profile URL.
    pub profile_url: String,
}

impl Default for ProfileUpdate {
    fn default() -> Self {
        Self {
            image_id: TextureKey::from(Uuid::nil()),
            fl_image_id: TextureKey::from(Uuid::nil()),
            about_text: String::new(),
            fl_about_text: String::new(),
            allow_publish: false,
            mature_publish: false,
            profile_url: String::new(),
        }
    }
}

/// An update to the agent's own interests, sent via
/// [`Session::update_interests`](crate::Session::update_interests)
/// (`AvatarInterestsUpdate`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterestsUpdate {
    /// The "want to" category bitmask.
    pub want_to_mask: u32,
    /// The "want to" free text.
    pub want_to_text: String,
    /// The "skills" category bitmask.
    pub skills_mask: u32,
    /// The "skills" free text.
    pub skills_text: String,
    /// The languages free text.
    pub languages_text: String,
}

/// A create-or-update of one of the agent's picks, sent via
/// [`Session::update_pick`](crate::Session::update_pick) (`PickInfoUpdate`).
/// Supply a fresh [`pick_id`](Self::pick_id) to create a pick, or an existing
/// one to edit it; the simulator fills in [`parcel_id`](Self::parcel_id) from
/// the agent's current parcel when it is nil.
#[derive(Debug, Clone, PartialEq)]
pub struct PickUpdate {
    /// The pick id (a fresh id to create; an existing id to edit).
    pub pick_id: PickKey,
    /// The parcel the pick points at, or `None` to let the simulator fill in the
    /// agent's current parcel.
    pub parcel_id: Option<ParcelKey>,
    /// The pick name.
    pub name: String,
    /// The pick description.
    pub description: String,
    /// The pick snapshot texture id.
    pub snapshot_id: Option<TextureKey>,
    /// The pick's global position (metres; nil/zero to use the agent's).
    pub pos_global: GlobalCoordinates,
    /// The sort order (only meaningful for top picks; normally `0`).
    pub sort_order: i32,
    /// Whether the pick is enabled (shown in the profile).
    pub enabled: bool,
}

impl Default for PickUpdate {
    fn default() -> Self {
        Self {
            pick_id: PickKey::from(Uuid::nil()),
            parcel_id: None,
            name: String::new(),
            description: String::new(),
            snapshot_id: None,
            pos_global: GlobalCoordinates::new(0.0, 0.0, 0.0),
            sort_order: 0,
            enabled: true,
        }
    }
}

/// A create-or-update of one of the agent's classifieds, sent via
/// [`Session::update_classified`](crate::Session::update_classified)
/// (`ClassifiedInfoUpdate`). Supply a fresh
/// [`classified_id`](Self::classified_id) to create a classified, or an
/// existing one to edit it; the simulator fills in
/// [`parcel_id`](Self::parcel_id) and the parent estate from the agent's
/// current parcel when the parcel is nil.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifiedUpdate {
    /// The classified id (a fresh id to create; an existing id to edit).
    pub classified_id: ClassifiedKey,
    /// The classified's search category.
    pub category: ClassifiedCategory,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at, or `None` to let the simulator fill
    /// in the agent's current parcel.
    pub parcel_id: Option<ParcelKey>,
    /// The classified snapshot texture id.
    pub snapshot_id: Option<TextureKey>,
    /// The classified's global position (metres; nil/zero to use the agent's).
    pub pos_global: GlobalCoordinates,
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount to pay to list this classified (L$).
    pub price_for_listing: LindenAmount,
}

impl Default for ClassifiedUpdate {
    fn default() -> Self {
        Self {
            classified_id: ClassifiedKey::from(Uuid::nil()),
            category: ClassifiedCategory::AnyCategory,
            name: String::new(),
            description: String::new(),
            parcel_id: None,
            snapshot_id: None,
            pos_global: GlobalCoordinates::new(0.0, 0.0, 0.0),
            classified_flags: 0,
            price_for_listing: LindenAmount(0),
        }
    }
}

// `FriendRights` (the friendship grant bitfield) now lives in
// `sl_types::friend`; re-exported here so the existing `sl_proto::…` path is
// unchanged.
pub use sl_types::friend::FriendRights;

/// One friend from the login buddy list, with the friendship rights in both
/// directions (parsed from the login `buddy-list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Friend {
    /// The friend's agent id.
    pub id: FriendKey,
    /// The rights this agent grants the friend.
    pub rights_granted: FriendRights,
    /// The rights the friend grants this agent.
    pub rights_received: FriendRights,
}

/// Account-level facts carried by the login response beyond what is needed to
/// bring up the circuit (parsed from the XML-RPC `login_to_simulator` reply).
/// Emitted once as [`Event::Account`](crate::Event::Account) right after [`Event::CircuitEstablished`](crate::Event::CircuitEstablished),
/// and also available from
/// [`Session::login_account`](crate::Session::login_account).
#[derive(Debug, Clone, PartialEq)]
pub struct LoginAccount {
    /// The agent's home location (region handle, position, look-at), if the grid
    /// provided a well-formed `home` field.
    pub home: Option<sl_wire::HomeLocation>,
    /// The camera look-at direction at the start location (`look_at`), if the
    /// grid provided it.
    pub look_at: Option<Direction>,
    /// The account's current maturity / content rating (`agent_access`).
    pub agent_access: Maturity,
    /// The maximum maturity rating the account is entitled to
    /// (`agent_access_max`); a client may not raise its preference above this.
    pub agent_access_max: Maturity,
    /// The maximum number of groups this account may join (`max-agent-groups`),
    /// or `None` if the grid did not report a limit. Check before joining a
    /// group.
    pub max_agent_groups: Option<u32>,
    /// The shared Library inventory's root folder id (`inventory-lib-root`), if
    /// provided. The folder tree is delivered as [`Event::LibraryInventory`](crate::Event::LibraryInventory).
    pub library_root: Option<InventoryFolderKey>,
    /// The agent id owning the shared Library (`inventory-lib-owner`), if
    /// provided. Library folder contents are fetched as this owner's inventory.
    pub library_owner: Option<AgentKey>,
}

/// The agent's directory (search) visibility — whether the account's online
/// status is shown in the people-search directory. The wire field is a free
/// string, but the only two values the reference viewer ever uses are
/// `"default"` (shown) and `"hidden"`; it is driven by a single "hide my online
/// status" toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryVisibility {
    /// The default visibility — the account's online status is shown in search.
    Default,
    /// The account's online status is hidden from search.
    Hidden,
}

impl DirectoryVisibility {
    /// The wire token for this visibility (`"default"` or `"hidden"`).
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Hidden => "hidden",
        }
    }

    /// Parses the wire visibility token. Mirrors the reference viewer, which
    /// treats only `"default"` as shown and maps `"hidden"` — and any
    /// unrecognised value — to [`Hidden`](Self::Hidden) (a conservative
    /// fallback).
    #[must_use]
    pub fn from_wire(visibility: &str) -> Self {
        match visibility {
            "default" => Self::Default,
            _ => Self::Hidden,
        }
    }
}

/// The agent's own account contact preferences (`UserInfoReply`), sent in reply
/// to a `UserInfoRequest`: whether offline IMs are forwarded to email, the
/// agent's directory (search) visibility, and the email address on file.
/// Surfaced as [`Event::UserInfo`](crate::Event::UserInfo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
    /// Whether offline instant messages are forwarded to the agent's email.
    pub im_via_email: bool,
    /// The agent's directory/search visibility setting.
    pub directory_visibility: DirectoryVisibility,
    /// The email address on file for the account.
    pub email: String,
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarClassified, ClassifiedCategory, ClassifiedKey, Friend, FriendKey, FriendRights,
        ParcelKey, PickKey, PickUpdate, Uuid,
    };
    use pretty_assertions::assert_eq;

    /// The Phase 5 profile/directory keys (`ClassifiedKey`/`ParcelKey`/
    /// `FriendKey`) are transparent wrappers over the wire `Uuid`: wrapping a raw
    /// id and reading it back is the identity, so a carrier keyed by them puts
    /// the exact same 16 bytes on the wire the old raw `Uuid` field did.
    #[test]
    fn profile_keys_round_trip_raw_uuid() {
        let raw = Uuid::from_u128(0x1234_5678_9abc_def0_1122_3344_5566_7788);
        assert_eq!(ClassifiedKey::from(raw).uuid(), raw);
        assert_eq!(ParcelKey::from(raw).uuid(), raw);
        assert_eq!(FriendKey::from(raw).uuid(), raw);

        let classified = AvatarClassified {
            classified_id: ClassifiedKey::from(raw),
            name: "ad".to_owned(),
        };
        assert_eq!(classified.classified_id.uuid(), raw);

        let friend = Friend {
            id: FriendKey::from(raw),
            rights_granted: FriendRights(FriendRights::CAN_SEE_ONLINE),
            rights_received: FriendRights::default(),
        };
        assert_eq!(friend.id.uuid(), raw);

        let pick = PickUpdate {
            parcel_id: Some(ParcelKey::from(raw)),
            ..PickUpdate::default()
        };
        assert_eq!(pick.parcel_id.map(|parcel| parcel.uuid()), Some(raw));
        assert_eq!(PickUpdate::default().parcel_id, None);

        // The nil default round-trips too.
        assert_eq!(ParcelKey::from(Uuid::nil()).uuid(), Uuid::nil());
    }

    /// The new client-local `PickKey` is a transparent wrapper over the wire
    /// `Uuid`, distinct from the existing `ClassifiedKey` (so a pick id and a
    /// classified id can't be transposed), and its `Display` matches the raw
    /// id's.
    #[test]
    fn pick_key_round_trips_raw_uuid() {
        let raw = Uuid::from_u128(0x0bad_f00d_0bad_f00d_0bad_f00d_0bad_f00d);
        assert_eq!(PickKey::from(raw).uuid(), raw);
        assert_eq!(PickKey::from(raw).to_string(), raw.to_string());
        // The create-new sentinel (nil) round-trips too.
        assert_eq!(PickKey::from(Uuid::nil()).uuid(), Uuid::nil());
        assert_eq!(PickUpdate::default().pick_id, PickKey::from(Uuid::nil()));
    }

    /// [`ClassifiedCategory`] maps every named classified-directory code to its
    /// exact `u32` wire value and back, so the typed `category` field puts the
    /// same bytes on the wire the old raw `u32` did. Unrecognised codes survive
    /// verbatim through [`ClassifiedCategory::Unknown`], and the default is the
    /// `0` "any category" sentinel.
    #[test]
    fn classified_category_round_trips_raw_u32() {
        for (code, category) in [
            (0, ClassifiedCategory::AnyCategory),
            (1, ClassifiedCategory::Shopping),
            (2, ClassifiedCategory::LandRental),
            (3, ClassifiedCategory::PropertyRental),
            (4, ClassifiedCategory::SpecialAttraction),
            (5, ClassifiedCategory::NewProducts),
            (6, ClassifiedCategory::Employment),
            (7, ClassifiedCategory::Wanted),
            (8, ClassifiedCategory::Service),
            (9, ClassifiedCategory::Personal),
        ] {
            assert_eq!(ClassifiedCategory::from_u32(code), category);
            assert_eq!(category.to_u32(), code);
        }

        // An unrecognised code is preserved verbatim, and the default is "any".
        assert_eq!(
            ClassifiedCategory::from_u32(42),
            ClassifiedCategory::Unknown(42)
        );
        assert_eq!(ClassifiedCategory::Unknown(42).to_u32(), 42);
        assert_eq!(
            ClassifiedCategory::default(),
            ClassifiedCategory::AnyCategory
        );
        assert_eq!(ClassifiedCategory::default().to_u32(), 0);
    }
}
