//! Public value types of the sans-I/O session: its inputs and outputs.
//!
//! The types are grouped into feature submodules (object, parcel, group, …) and
//! re-exported here, so the crate's public surface (`crate::types::*`, re-exported
//! again from `lib.rs`) is unchanged by that internal split.

mod alert;
mod appearance;
mod asset;
mod avatar_profile;
mod chat;
mod diagnostic;
pub(crate) mod directory;
mod economy;
mod editing;
mod environment;
mod event;
mod group;
mod inventory;
mod map;
mod name;
mod nearby;
mod object;
mod parcel;
mod region;
mod report;
mod script;
mod session;
mod terrain;

/// Build an [`OwnerKey`](sl_types::key::OwnerKey) from the wire's raw owner UUID
/// and its accompanying group flag.
///
/// This is the codec boundary for the agent-or-group owner fields: when the
/// group flag is set the UUID names a [`GroupKey`](sl_types::key::GroupKey),
/// otherwise an [`AgentKey`](sl_types::key::AgentKey). The inverse on encode is
/// `owner.uuid()` for the id and `owner.is_group()` for the flag.
pub(crate) fn owner_key_from_wire(uuid: uuid::Uuid, is_group: bool) -> sl_types::key::OwnerKey {
    if is_group {
        sl_types::key::OwnerKey::Group(sl_types::key::GroupKey::from(uuid))
    } else {
        sl_types::key::OwnerKey::Agent(sl_types::key::AgentKey::from(uuid))
    }
}

/// Decode a wire `GroupID` UUID into an optional [`GroupKey`](sl_types::key::GroupKey).
///
/// A nil UUID means "no group set" and maps to `None`; any other value is a
/// [`GroupKey`](sl_types::key::GroupKey). The inverse on encode is
/// `group.map_or_else(uuid::Uuid::nil, |g| g.uuid())`.
pub(crate) fn group_from_wire(uuid: uuid::Uuid) -> Option<sl_types::key::GroupKey> {
    if uuid.is_nil() {
        None
    } else {
        Some(sl_types::key::GroupKey::from(uuid))
    }
}

/// Encode an optional [`GroupKey`](sl_types::key::GroupKey) back to a wire
/// `GroupID` UUID, mapping `None` to the nil UUID.
pub(crate) fn group_to_wire(group: Option<sl_types::key::GroupKey>) -> uuid::Uuid {
    group.map_or_else(uuid::Uuid::nil, |g| g.uuid())
}

/// Build an [`OwnerKey`](sl_types::key::OwnerKey) for the types that signal group
/// ownership via a *null* `OwnerID`, carrying the owning group in the separate
/// `GroupID` slot (`ObjectProperties` and friends): a nil `OwnerID` alongside a
/// non-nil `GroupID` is a group-owned object, otherwise an agent owner.
pub(crate) fn object_owner_from_wire(
    owner_id: uuid::Uuid,
    group_id: uuid::Uuid,
) -> sl_types::key::OwnerKey {
    if owner_id.is_nil() && !group_id.is_nil() {
        sl_types::key::OwnerKey::Group(sl_types::key::GroupKey::from(group_id))
    } else {
        sl_types::key::OwnerKey::Agent(sl_types::key::AgentKey::from(owner_id))
    }
}

/// Build an [`OwnerKey`](sl_types::key::OwnerKey) for the inventory types, which
/// carry an explicit `GroupOwned` flag plus the owning group in `GroupID` (the
/// `OwnerID` is nil when group-owned).
pub(crate) fn inventory_owner_from_wire(
    owner_id: uuid::Uuid,
    group_id: uuid::Uuid,
    group_owned: bool,
) -> sl_types::key::OwnerKey {
    if group_owned {
        sl_types::key::OwnerKey::Group(sl_types::key::GroupKey::from(group_id))
    } else {
        sl_types::key::OwnerKey::Agent(sl_types::key::AgentKey::from(owner_id))
    }
}

/// Split an [`OwnerKey`](sl_types::key::OwnerKey) and its separate set-to group
/// back into the wire's `(OwnerID, GroupID)` pair for the null-`OwnerID` types
/// (objects and inventory): a group owner writes a nil `OwnerID` and puts its
/// group in `GroupID`; an agent owner writes its id and the set-to group (or nil)
/// in `GroupID`. The accompanying `GroupOwned` flag, where present, is
/// `owner.is_group()`.
pub(crate) fn object_owner_to_wire(
    owner: sl_types::key::OwnerKey,
    group: Option<sl_types::key::GroupKey>,
) -> (uuid::Uuid, uuid::Uuid) {
    match owner {
        sl_types::key::OwnerKey::Group(g) => (uuid::Uuid::nil(), g.uuid()),
        sl_types::key::OwnerKey::Agent(a) => (a.uuid(), group_to_wire(group)),
    }
}

pub use alert::{MeanCollision, MeanCollisionType};
pub use appearance::{
    AttachmentMode, AttachmentPoint, AvatarAppearance, AvatarAttachment, DetachOrder,
    PlayingAnimation, RezAttachment, SoundFlags, SoundPreload, TextureEntry, TextureFace, Wearable,
    WearableType, avatar_texture,
};
pub use asset::{Asset, AssetType, ImageCodec, InventoryType, Texture, TransferStatus};
pub use avatar_profile::{
    AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties,
    ClassifiedInfo, ClassifiedUpdate, Friend, FriendRights, InterestsUpdate, LoginAccount,
    PickInfo, PickUpdate, ProfileUpdate,
};
pub use chat::{
    ChatAudible, ChatMessage, ChatSourceType, ChatType, ImDialog, InstantMessage, InventoryOffer,
};
pub use diagnostic::Diagnostic;
pub use directory::{
    AvatarPickerResult, DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult,
    DirLandResult, DirPeopleResult, DirPlaceResult, EventId, EventInfo, LandSearchType,
    PlacesResult,
};
pub use economy::{EconomyData, MoneyBalance, MoneyTransaction, MoneyTransactionType};
pub use editing::{
    ClickAction, DeRezDestination, Material, Maturity, NotecardRez, ObjectBuyItem,
    ObjectFlagSettings, ObjectTransform, PermissionField, PrimShape, ProductType, RestoreItem,
    SaleType, TeleportFlags,
};
pub use environment::{DayCycle, DayCycleFrame, EnvironmentSettings, SkySettings, WaterSettings};
pub use event::Event;
pub use group::{
    ActiveGroup, CreateGroupParams, GroupAccountDetails, GroupAccountDetailsEntry,
    GroupAccountSummary, GroupAccountTransaction, GroupAccountTransactions,
    GroupActiveProposalItem, GroupMember, GroupMembership, GroupNotice, GroupNoticeAttachment,
    GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, GroupVote, GroupVoteHistoryItem,
    group_powers,
};
pub use inventory::{
    GestureActivation, InventoryFolder, InventoryItem, NewInventoryItem, global_to_handle,
    grid_to_handle, handle_to_global, handle_to_grid,
};
pub use map::{
    EstateAccessDelta, EstateAccessKind, EstateCovenant, EstateInfo, MapItem, MapItemType,
    MapLayer, MapRegionInfo, MapRequestFlags, NeighborInfo, RegionInfoUpdate, TelehubInfo,
};
pub use name::{AvatarName, GroupName};
pub use nearby::{
    CoarseLocation, LookAtType, PointAtType, ViewerEffect, ViewerEffectData, ViewerEffectType,
};
pub use object::{
    ExtendedMesh, FlexibleData, LightData, LightImage, NameValue, Object, ObjectExtraParams,
    ObjectMotion, ObjectProperties, ObjectPropertiesFamily, ParticleSystem, PrimShapeParams,
    ReflectionProbe, RenderMaterialRef, SculptData, TextureAnimation, particle_pattern, pcode,
    texture_anim_mode,
};
pub use parcel::{
    LandStatItem, LandStatReportType, LandingType, ParcelAccessEntry, ParcelAccessFlags,
    ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelInfo, ParcelMediaCommand,
    ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate,
};
pub use region::{RegionChatSettings, RegionCombatSettings, RegionIdentity, RegionLimits};
pub use report::Postcard;
pub use script::{
    AlertInfo, FollowCamProperty, FollowCamPropertyValue, LoadUrlRequest, MuteEntry, MuteFlags,
    MuteType, ScriptControl, ScriptControlAction, ScriptDialog, ScriptPermissionRequest,
    ScriptPermissions, ScriptTeleportRequest,
};
pub use session::{
    Camera, CameraError, DisconnectReason, Kilobits, LoginHttpRequest, LoginParams, MovementMode,
    Reliability, Throttle, ThrottleBuilder, ThrottleError, Transmit,
};
pub use terrain::{TerrainLayerType, TerrainPatch};

#[cfg(test)]
mod owner_codec_tests {
    use super::{
        group_from_wire, group_to_wire, inventory_owner_from_wire, object_owner_from_wire,
        object_owner_to_wire, owner_key_from_wire,
    };
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, GroupKey, OwnerKey};
    use uuid::Uuid;

    #[test]
    fn owner_key_from_wire_tags_by_group_flag() {
        let id = Uuid::from_u128(0xA1);
        let agent = owner_key_from_wire(id, false);
        assert_eq!(agent, OwnerKey::Agent(AgentKey::from(id)));
        assert_eq!(agent.uuid(), id);
        assert!(!agent.is_group());
        let group = owner_key_from_wire(id, true);
        assert_eq!(group, OwnerKey::Group(GroupKey::from(id)));
        assert_eq!(group.uuid(), id);
        assert!(group.is_group());
    }

    #[test]
    fn group_from_wire_maps_nil_to_none() {
        assert_eq!(group_from_wire(Uuid::nil()), None);
        let g = Uuid::from_u128(0xB2);
        assert_eq!(group_from_wire(g), Some(GroupKey::from(g)));
        assert_eq!(group_to_wire(None), Uuid::nil());
        assert_eq!(group_to_wire(Some(GroupKey::from(g))), g);
    }

    #[test]
    fn object_owner_wire_round_trips() {
        let owner_a = Uuid::from_u128(0xC3);
        let group_g = Uuid::from_u128(0xD4);

        // Agent-owned with a set-to group.
        let owner = object_owner_from_wire(owner_a, group_g);
        let group = group_from_wire(group_g);
        assert_eq!(owner, OwnerKey::Agent(AgentKey::from(owner_a)));
        assert_eq!(group, Some(GroupKey::from(group_g)));
        assert_eq!(object_owner_to_wire(owner, group), (owner_a, group_g));

        // Group-owned: nil OwnerID, the owning group lives in GroupID.
        let owner = object_owner_from_wire(Uuid::nil(), group_g);
        let group = group_from_wire(group_g);
        assert_eq!(owner, OwnerKey::Group(GroupKey::from(group_g)));
        assert_eq!(object_owner_to_wire(owner, group), (Uuid::nil(), group_g));

        // Agent-owned, no group set.
        let owner = object_owner_from_wire(owner_a, Uuid::nil());
        let group = group_from_wire(Uuid::nil());
        assert_eq!(owner, OwnerKey::Agent(AgentKey::from(owner_a)));
        assert_eq!(group, None);
        assert_eq!(object_owner_to_wire(owner, group), (owner_a, Uuid::nil()));
    }

    #[test]
    fn inventory_owner_wire_round_trips() {
        let owner_a = Uuid::from_u128(0xE5);
        let group_g = Uuid::from_u128(0xF6);

        // Agent-owned (GroupOwned=false): id from OwnerID, set-to group from GroupID.
        let owner = inventory_owner_from_wire(owner_a, group_g, false);
        let group = group_from_wire(group_g);
        assert_eq!(owner, OwnerKey::Agent(AgentKey::from(owner_a)));
        assert!(!owner.is_group());
        assert_eq!(object_owner_to_wire(owner, group), (owner_a, group_g));

        // Group-owned (GroupOwned=true): group from GroupID, nil OwnerID on encode.
        let owner = inventory_owner_from_wire(Uuid::nil(), group_g, true);
        let group = group_from_wire(group_g);
        assert_eq!(owner, OwnerKey::Group(GroupKey::from(group_g)));
        assert!(owner.is_group());
        assert_eq!(object_owner_to_wire(owner, group), (Uuid::nil(), group_g));
    }
}
