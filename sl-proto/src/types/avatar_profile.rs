//! Avatar profile and relationships: properties, picks, classifieds, friends.

use super::Maturity;
use sl_types::key::{
    AgentKey, ClassifiedKey, FriendKey, GroupKey, InventoryFolderKey, ParcelKey, TextureKey,
};
use uuid::Uuid;

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
    pub pick_id: Uuid,
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
    pub pick_id: Uuid,
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
    pub snapshot_id: TextureKey,
    /// The owner's account name, as the grid resolves it.
    pub user: String,
    /// The parcel's original name.
    pub original_name: String,
    /// The region name the pick is in.
    pub sim_name: String,
    /// The pick's global position (metres, grid-wide coordinates).
    pub pos_global: (f64, f64, f64),
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
    pub category: u32,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at.
    pub parcel_id: ParcelKey,
    /// The parent estate id.
    pub parent_estate: u32,
    /// The classified snapshot texture id.
    pub snapshot_id: TextureKey,
    /// The region name the classified is in.
    pub sim_name: String,
    /// The classified's global position (metres, grid-wide coordinates).
    pub pos_global: (f64, f64, f64),
    /// The parcel name.
    pub parcel_name: String,
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount paid to list this classified (L$).
    pub price_for_listing: i32,
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
    pub pick_id: Uuid,
    /// The parcel the pick points at, or `None` to let the simulator fill in the
    /// agent's current parcel.
    pub parcel_id: Option<ParcelKey>,
    /// The pick name.
    pub name: String,
    /// The pick description.
    pub description: String,
    /// The pick snapshot texture id.
    pub snapshot_id: TextureKey,
    /// The pick's global position (metres; nil/zero to use the agent's).
    pub pos_global: (f64, f64, f64),
    /// The sort order (only meaningful for top picks; normally `0`).
    pub sort_order: i32,
    /// Whether the pick is enabled (shown in the profile).
    pub enabled: bool,
}

impl Default for PickUpdate {
    fn default() -> Self {
        Self {
            pick_id: Uuid::nil(),
            parcel_id: None,
            name: String::new(),
            description: String::new(),
            snapshot_id: TextureKey::from(Uuid::nil()),
            pos_global: (0.0, 0.0, 0.0),
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
    pub category: u32,
    /// The classified name.
    pub name: String,
    /// The classified description.
    pub description: String,
    /// The parcel the classified points at, or `None` to let the simulator fill
    /// in the agent's current parcel.
    pub parcel_id: Option<ParcelKey>,
    /// The classified snapshot texture id.
    pub snapshot_id: TextureKey,
    /// The classified's global position (metres; nil/zero to use the agent's).
    pub pos_global: (f64, f64, f64),
    /// The classified flags bitfield (e.g. mature, auto-renew).
    pub classified_flags: u8,
    /// The amount to pay to list this classified (L$).
    pub price_for_listing: i32,
}

impl Default for ClassifiedUpdate {
    fn default() -> Self {
        Self {
            classified_id: ClassifiedKey::from(Uuid::nil()),
            category: 0,
            name: String::new(),
            description: String::new(),
            parcel_id: None,
            snapshot_id: TextureKey::from(Uuid::nil()),
            pos_global: (0.0, 0.0, 0.0),
            classified_flags: 0,
            price_for_listing: 0,
        }
    }
}

/// The rights one party grants the other in a Second Life friendship: a
/// bitfield shared by the login `buddy-list`, `GrantUserRights`, and
/// `ChangeUserRights`. The flag values match the viewer's `RIGHTS_*`/`GRANT_*`
/// constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FriendRights(pub i32);

impl FriendRights {
    /// The other party may see when this party is online (`GRANT_ONLINE_STATUS`).
    pub const CAN_SEE_ONLINE: i32 = 1 << 0;
    /// The other party may see this party's location on the world map
    /// (`GRANT_MAP_LOCATION`).
    pub const CAN_SEE_ON_MAP: i32 = 1 << 1;
    /// The other party may modify this party's objects (`GRANT_MODIFY_OBJECTS`).
    pub const CAN_MODIFY_OBJECTS: i32 = 1 << 2;

    /// Whether the see-online bit is set.
    #[must_use]
    pub const fn can_see_online(self) -> bool {
        self.0 & Self::CAN_SEE_ONLINE != 0
    }

    /// Whether the see-on-map bit is set.
    #[must_use]
    pub const fn can_see_on_map(self) -> bool {
        self.0 & Self::CAN_SEE_ON_MAP != 0
    }

    /// Whether the modify-objects bit is set.
    #[must_use]
    pub const fn can_modify_objects(self) -> bool {
        self.0 & Self::CAN_MODIFY_OBJECTS != 0
    }
}

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
    pub look_at: Option<[f32; 3]>,
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
    pub library_owner: Option<Uuid>,
}

#[cfg(test)]
mod tests {
    use super::{
        AvatarClassified, ClassifiedKey, Friend, FriendKey, FriendRights, ParcelKey, PickUpdate,
        Uuid,
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
}
