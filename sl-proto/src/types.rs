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
mod display_name;
mod economy;
mod editing;
mod environment;
mod event;
mod generic;
mod group;
mod inventory;
mod land;
mod map;
mod name;
mod nearby;
mod object;
mod open_region;
mod parcel;
mod pathfinding;
mod region;
mod report;
mod script;
mod server_error;
mod session;
mod terrain;
mod voice;

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

/// Decode a wire UUID into an optional typed key: a nil UUID is the in-band
/// "absent" sentinel and maps to `None`, any other value to `Some(K::from(..))`.
///
/// This is the codec boundary for the many nil-means-unset id fields (parcel
/// snapshot/media textures, optional task/folder ids, …). The inverse on encode
/// is [`optional_key_to_wire`].
pub(crate) fn optional_key_from_wire<K>(uuid: uuid::Uuid) -> Option<K>
where
    K: From<uuid::Uuid>,
{
    (!uuid.is_nil()).then(|| K::from(uuid))
}

/// Encode an optional typed key back to a wire UUID, mapping `None` to the nil
/// UUID (the inverse of [`optional_key_from_wire`]). The closure extracts the
/// inner UUID (the keys have no shared trait for it).
pub(crate) fn optional_key_to_wire<K>(
    key: Option<K>,
    to_uuid: impl FnOnce(K) -> uuid::Uuid,
) -> uuid::Uuid {
    key.map_or_else(uuid::Uuid::nil, to_uuid)
}

/// Decode a wire UUID into an optional raw [`Uuid`](uuid::Uuid): a nil UUID is
/// the in-band "absent" sentinel and maps to `None`. For the nil-means-unset id
/// fields that were deliberately left untyped (no agent/group/object family
/// fits, or the id is a raw correlation value). The inverse on encode is
/// [`optional_uuid_to_wire`].
pub(crate) fn optional_uuid_from_wire(uuid: uuid::Uuid) -> Option<uuid::Uuid> {
    (!uuid.is_nil()).then_some(uuid)
}

/// Encode an optional raw [`Uuid`](uuid::Uuid) back to a wire UUID, mapping
/// `None` to the nil UUID (the inverse of [`optional_uuid_from_wire`]).
pub(crate) fn optional_uuid_to_wire(uuid: Option<uuid::Uuid>) -> uuid::Uuid {
    uuid.unwrap_or_else(uuid::Uuid::nil)
}

/// Decode a wire `u32` into an optional value: `0` is the in-band "unset"
/// sentinel and maps to `None`, any other value to `Some(..)`. The codec
/// boundary for the numeric `0`-means-unset fields (IM/invitation timestamps,
/// the async inventory callback id). The inverse on encode is
/// [`optional_u32_to_wire`].
pub(crate) fn optional_u32_from_wire(value: u32) -> Option<u32> {
    (value != 0).then_some(value)
}

/// Encode an optional `u32` back to a wire value, mapping `None` to the `0`
/// sentinel (the inverse of [`optional_u32_from_wire`]).
pub(crate) fn optional_u32_to_wire(value: Option<u32>) -> u32 {
    value.unwrap_or(0)
}

/// Decode a wire `i32` into an optional value: `0` is the in-band "unset"
/// sentinel and maps to `None`, any other value to `Some(..)`. The codec
/// boundary for the numeric `0`-means-unset/native fields (parcel media
/// width/height, which the client only ever decodes).
pub(crate) fn optional_i32_from_wire(value: i32) -> Option<i32> {
    (value != 0).then_some(value)
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

/// Decode a non-negative L$ wire field (a signed 32-bit integer) into a
/// [`LindenAmount`](sl_types::money::LindenAmount).
///
/// This is the codec boundary for the L$ *price* fields a conforming peer only
/// ever sends non-negative (sale prices, upload/claim/rent prices, the listing
/// fee, the per-metre land price, …). A negative value is rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than silently coerced, so a malformed message is dropped (and surfaced as a
/// diagnostic) instead of masquerading as `0`. The inverse on encode is
/// [`linden_to_wire`].
pub(crate) fn linden_from_wire(
    field: &'static str,
    value: i32,
) -> Result<sl_types::money::LindenAmount, sl_wire::WireError> {
    match u64::try_from(value) {
        Ok(magnitude) => Ok(sl_types::money::LindenAmount(magnitude)),
        Err(_negative) => Err(sl_wire::WireError::ValueOutOfRange {
            field,
            value: i64::from(value),
        }),
    }
}

/// Decode a non-negative Land Impact wire field (a signed 32-bit integer) into
/// a [`LandImpact`](crate::types::LandImpact).
///
/// This is the codec boundary for the region object-budget fields (the
/// `ObjectCapacity` / `ObjectCount` of an `EconomyData` reply), which a
/// conforming simulator only ever sends non-negative. A negative value is
/// rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than silently coerced, so a malformed message is dropped (and surfaced as a
/// diagnostic) instead of masquerading as `0`.
pub(crate) fn land_impact_from_wire(
    field: &'static str,
    value: i32,
) -> Result<crate::types::LandImpact, sl_wire::WireError> {
    match u32::try_from(value) {
        Ok(magnitude) => Ok(crate::types::LandImpact(magnitude)),
        Err(_negative) => Err(sl_wire::WireError::ValueOutOfRange {
            field,
            value: i64::from(value),
        }),
    }
}

/// Encode a [`LindenAmount`](sl_types::money::LindenAmount) back into a signed
/// 32-bit L$ wire field, the inverse of [`linden_from_wire`].
///
/// An amount that exceeds the signed 32-bit range a wire price field can hold
/// is rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than clamped, so an out-of-range value fails the send loudly instead of
/// silently changing on the wire.
pub(crate) fn linden_to_wire(
    field: &'static str,
    amount: &sl_types::money::LindenAmount,
) -> Result<i32, sl_wire::WireError> {
    let sl_types::money::LindenAmount(value) = *amount;
    match i32::try_from(value) {
        Ok(wire) => Ok(wire),
        Err(_too_large) => Err(sl_wire::WireError::ValueOutOfRange {
            field,
            value: i64::try_from(value).unwrap_or(i64::MAX),
        }),
    }
}

/// Decode an optional L$ *sale* price: `Some` (validated) when the companion
/// for-sale field says the item is for sale, `None` otherwise.
///
/// The for-sale state lives in its own wire field (a `sale_type`, the parcel
/// `FOR_SALE` flag, …); a not-for-sale item carries no meaningful price, so it
/// maps to `None` (a for-sale item may still be free → `Some(LindenAmount(0))`).
/// On a for-sale item a negative price is still rejected via [`linden_from_wire`].
pub(crate) fn linden_price_from_wire(
    for_sale: bool,
    field: &'static str,
    value: i32,
) -> Result<Option<sl_types::money::LindenAmount>, sl_wire::WireError> {
    if for_sale {
        Ok(Some(linden_from_wire(field, value)?))
    } else {
        Ok(None)
    }
}

/// Encode an optional L$ *sale* price back to its signed 32-bit wire field: the
/// amount when `Some`, or `0` (the not-for-sale wire sentinel) when `None`.
pub(crate) fn linden_price_to_wire(
    field: &'static str,
    price: Option<&sl_types::money::LindenAmount>,
) -> Result<i32, sl_wire::WireError> {
    match price {
        Some(amount) => linden_to_wire(field, amount),
        None => Ok(0),
    }
}

/// Decode an optional L$ event *cover charge* from its unsigned 32-bit wire
/// field, gated on the companion `cover` flag.
///
/// An event carries a cover charge only when its `cover` field is non-zero; a
/// zero `cover` means no charge, so the amount maps to `None` regardless of the
/// wire value (the dataserver sends `0`). The wire field is unsigned, so — unlike
/// the signed price fields — every value is in range and the decode is total.
/// The inverse on encode is [`linden_cover_to_wire`].
pub(crate) fn linden_cover_from_wire(
    cover: u32,
    amount: u32,
) -> Option<sl_types::money::LindenAmount> {
    if cover == 0 {
        None
    } else {
        Some(sl_types::money::LindenAmount(u64::from(amount)))
    }
}

/// Encode an optional L$ event cover charge back to its unsigned 32-bit wire
/// field: the amount when `Some`, or `0` (the no-cover wire sentinel) when
/// `None`.
///
/// An amount that exceeds the unsigned 32-bit range the wire field can hold is
/// rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than clamped, so an out-of-range value fails the send loudly.
pub(crate) fn linden_cover_to_wire(
    field: &'static str,
    amount: Option<&sl_types::money::LindenAmount>,
) -> Result<u32, sl_wire::WireError> {
    match amount {
        Some(charge) => {
            let sl_types::money::LindenAmount(value) = *charge;
            u32::try_from(value).map_err(|_too_large| sl_wire::WireError::ValueOutOfRange {
                field,
                value: i64::try_from(value).unwrap_or(i64::MAX),
            })
        }
        None => Ok(0),
    }
}

/// Decode a non-negative land-area wire field (a signed 32-bit count of square
/// metres) into a [`LandArea`].
///
/// This is the codec boundary for the land-area fields a conforming peer only
/// ever sends non-negative (a group land contribution, a parcel's
/// actual/billable area, an avatar's land credit/commitment). A negative value
/// is rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than masked, so a malformed message is dropped (and surfaced as a diagnostic)
/// instead of reading as `0`. The inverse on encode is [`land_area_to_wire`].
pub(crate) fn land_area_from_wire(
    field: &'static str,
    value: i32,
) -> Result<LandArea, sl_wire::WireError> {
    match u32::try_from(value) {
        Ok(square_metres) => Ok(LandArea(square_metres)),
        Err(_negative) => Err(sl_wire::WireError::ValueOutOfRange {
            field,
            value: i64::from(value),
        }),
    }
}

/// Encode a [`LandArea`] back into a signed 32-bit square-metre wire field, the
/// inverse of [`land_area_from_wire`].
///
/// An area that exceeds the signed 32-bit range a wire field can hold is
/// rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than clamped, so an out-of-range value fails the send loudly.
pub(crate) fn land_area_to_wire(
    field: &'static str,
    area: &LandArea,
) -> Result<i32, sl_wire::WireError> {
    let LandArea(square_metres) = *area;
    match i32::try_from(square_metres) {
        Ok(wire) => Ok(wire),
        Err(_too_large) => Err(sl_wire::WireError::ValueOutOfRange {
            field,
            value: i64::from(square_metres),
        }),
    }
}

/// Encode a [`LindenBalance`] back into a signed 32-bit L$ wire field.
///
/// This is the encode boundary for the legitimately *signed* L$ fields (a
/// group's current balance, a group-accounting transaction delta). The decode
/// direction is total — every `i32` is a valid balance — so it is just
/// [`LindenBalance::from_i32`] at the call site; the encode direction can fail
/// when a balance exceeds the signed 32-bit range the wire field holds, and is
/// rejected with
/// [`WireError::ValueOutOfRange`](sl_wire::WireError::ValueOutOfRange) rather
/// than clamped, so an out-of-range value fails the send loudly.
pub(crate) fn linden_balance_to_wire(
    field: &'static str,
    balance: &LindenBalance,
) -> Result<i32, sl_wire::WireError> {
    balance
        .to_i32()
        .ok_or_else(|| sl_wire::WireError::ValueOutOfRange {
            field,
            value: balance.to_i64().unwrap_or(i64::MAX),
        })
}

pub use alert::{MeanCollision, MeanCollisionType};
pub use appearance::{
    AttachmentMode, AttachmentPoint, AvatarAppearance, AvatarAttachment, DetachOrder,
    PlayingAnimation, RezAttachment, SoundFlags, SoundPreload, TextureEntry, TextureFace, Wearable,
    WearableType, avatar_texture,
};
pub use asset::{
    Asset, AssetType, ImageCodec, InventoryType, NotUpdatableAssetType, Texture, TransferStatus,
    UpdatableAssetType,
};
pub use avatar_profile::{
    AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties,
    ClassifiedInfo, ClassifiedUpdate, DirectoryVisibility, Friend, FriendRights, InterestsUpdate,
    LoginAccount, PickInfo, PickKey, PickUpdate, ProfileUpdate, UserInfo,
};
pub use chat::{
    ChatAudible, ChatMessage, ChatSource, ChatSourceType, ChatType, ChatTypeNotAVolume, ImDialog,
    InstantMessage, InventoryOffer,
};
pub use diagnostic::Diagnostic;
pub use directory::{
    AvatarPickerResult, DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult,
    DirLandResult, DirPeopleResult, DirPlaceResult, EventInfo, LandSearchType, PlacesResult,
};
pub use display_name::{DisplayNameUpdate, SetDisplayNameReply};
pub use economy::{EconomyData, LandImpact, MoneyBalance, MoneyTransaction, MoneyTransactionType};
pub use editing::{
    ClickAction, DeRezDestination, Material, Maturity, NotecardRez, ObjectBuyItem,
    ObjectFlagSettings, ObjectTransform, PermissionField, PrimShape, ProductType, RestoreItem,
    RezObjectParams, RezScriptParams, SaleType, SurfaceInfo, TaskInventoryKey, TeleportFlags,
};
pub use environment::{
    CloudPosDensity, Color, ColorAlpha, DayCycle, DayCycleFrame, EnvironmentSettings, Glow, Scale,
    SkySettings, WaterSettings,
};
pub use event::Event;
pub use generic::{GenericMessage, GenericStreamingMessage};
pub use group::{
    ActiveGroup, CreateGroupParams, GroupAccountDetails, GroupAccountDetailsEntry,
    GroupAccountSummary, GroupAccountTransaction, GroupAccountTransactions,
    GroupActiveProposalItem, GroupMember, GroupMembership, GroupNotice, GroupNoticeAttachment,
    GroupNoticeKey, GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, GroupVote, GroupVoteHistoryItem,
    ProposalCandidateId, ProposalVoteId, UpdateGroupInfoParams, group_powers,
};
pub use inventory::{
    Child, FolderInfo, FolderType, GestureActivation, InventoryCursor, InventoryFolder,
    InventoryItem, InventoryItemMove, ItemInfo, NewInventoryItem, NewInventoryLink,
    global_to_handle, grid_to_handle, handle_to_global, handle_to_grid,
};
pub use land::{LandBrushAction, LandBrushSize, LandEdit, TerraformArea};
pub use map::{
    EjectAction, EstateAccessDelta, EstateAccessKind, EstateCovenant, EstateInfo, FreezeAction,
    GodRegionUpdate, MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, NeighborInfo,
    RegionInfoUpdate, SimWideDeleteFlags, TelehubInfo,
};
pub use name::{AvatarName, GroupName};
pub use nearby::{
    CoarseLocation, LookAtType, PointAtType, ViewerEffect, ViewerEffectData, ViewerEffectType,
};
pub use object::{
    ExtendedMesh, FlexibleData, LightData, LightImage, NameValue, Object, ObjectExtraParams,
    ObjectMotion, ObjectPlayingAnimation, ObjectProperties, ObjectPropertiesFamily, ParticleSystem,
    PrimShapeParams, ReflectionProbe, RenderMaterialRef, SculptData, TaskInventoryItem,
    TaskInventoryReply, TextureAnimation, particle_pattern, pcode, texture_anim_mode,
};
pub use open_region::OpenRegionInfo;
pub use parcel::{
    DEFAULT_GRIDS_PER_EDGE, LandStatItem, LandStatReportType, LandingType, PARCEL_GRID_STEP_METRES,
    ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelDetails,
    ParcelInfo, ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayCell,
    ParcelOverlayError, ParcelOverlayGrid, ParcelOverlayInfo, ParcelOwnership, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate,
};
pub use pathfinding::{NavMeshBuildStatus, NavMeshStatus};
pub use region::{
    DEFAULT_TERRAIN_DETAIL_TEXTURES, RegionChatSettings, RegionCombatSettings, RegionIdentity,
    RegionLimits, RegionStats, RegionTerrainComposition, SimStatId, SimulatorTime,
};
pub use report::Postcard;
pub use script::{
    AlertInfo, DEFAULT_LSL_SCRIPT, DEFAULT_LUAU_SCRIPT, FollowCamProperty, FollowCamPropertyValue,
    LoadUrlRequest, MuteEntry, MuteFlags, MuteType, PermissionRole, ScriptCompileError,
    ScriptControl, ScriptControlAction, ScriptControlsInfo, ScriptDialog, ScriptGrantInfo,
    ScriptLanguage, ScriptPermissionRequest, ScriptPermissionState, ScriptPermissionStatus,
    ScriptPermissions, ScriptTarget, ScriptTeleportRequest, ScriptUploadLocation,
};
pub use server_error::{FeatureDisabled, Kick, ServerError};
pub use session::{
    Camera, CameraError, DisconnectReason, Kilobits, LoginHttpRequest, LoginParams, MovementMode,
    Reliability, StartLocationSlot, Throttle, ThrottleBuilder, ThrottleError, Transmit,
};
pub use terrain::{TerrainLayerType, TerrainPatch};
pub use voice::RequiredVoiceVersion;

// Value types migrated to the shared `sl-types` crate, re-exported here so the
// flat `crate::types::*` surface (and the in-crate `crate::types::X` references)
// keep resolving after the move.
pub use sl_types::key::{
    AgentOrObjectKey, GroupRoleKey, InventoryItemOrFolderKey, MeshKey, SculptOrMeshKey,
};
pub use sl_types::map::LandArea;
pub use sl_types::money::{LindenBalance, NegativeBalanceError};
pub use sl_types::search::{ClassifiedCategory, EventId};

#[cfg(test)]
mod owner_codec_tests {
    use super::{
        group_from_wire, group_to_wire, inventory_owner_from_wire, land_area_from_wire,
        land_area_to_wire, linden_cover_from_wire, linden_cover_to_wire, linden_from_wire,
        linden_price_from_wire, linden_price_to_wire, linden_to_wire, object_owner_from_wire,
        object_owner_to_wire, optional_i32_from_wire, optional_key_from_wire, optional_key_to_wire,
        optional_u32_from_wire, optional_u32_to_wire, optional_uuid_from_wire,
        optional_uuid_to_wire, owner_key_from_wire,
    };
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, GroupKey, OwnerKey, TextureKey};
    use sl_types::money::LindenAmount;
    use uuid::Uuid;

    #[test]
    fn land_area_wire_round_trips_and_rejects_negative() -> Result<(), sl_wire::WireError> {
        // Non-negative square-metre counts round-trip bit-identically.
        for wire in [0_i32, 512, 0x1_0000, i32::MAX] {
            let area = land_area_from_wire("Test", wire)?;
            assert_eq!(land_area_to_wire("Test", &area)?, wire);
        }
        // A negative land area (which a conforming peer never sends) is rejected,
        // not masked to `0`.
        assert_eq!(
            land_area_from_wire("Test", -1),
            Err(sl_wire::WireError::ValueOutOfRange {
                field: "Test",
                value: -1,
            })
        );
        Ok(())
    }

    #[test]
    fn linden_price_gates_on_for_sale() -> Result<(), sl_wire::WireError> {
        // Not for sale → `None`, regardless of the (meaningless) wire price.
        assert_eq!(linden_price_from_wire(false, "SalePrice", 999)?, None);
        // For sale → `Some` (a for-sale item may still be free).
        assert_eq!(
            linden_price_from_wire(true, "SalePrice", 0)?,
            Some(LindenAmount(0))
        );
        assert_eq!(
            linden_price_from_wire(true, "SalePrice", 250)?,
            Some(LindenAmount(250))
        );
        // Encode: `None` writes the `0` not-for-sale wire sentinel; `Some` writes
        // the amount.
        assert_eq!(linden_price_to_wire("SalePrice", None)?, 0);
        assert_eq!(
            linden_price_to_wire("SalePrice", Some(&LindenAmount(250)))?,
            250
        );
        Ok(())
    }

    #[test]
    fn linden_cover_gates_on_cover_flag() -> Result<(), sl_wire::WireError> {
        // No cover charge (`cover == 0`) → `None`, regardless of the wire amount.
        assert_eq!(linden_cover_from_wire(0, 999), None);
        // A cover charge applies → `Some` (the U32 wire is always in range).
        assert_eq!(linden_cover_from_wire(1, 0), Some(LindenAmount(0)));
        assert_eq!(linden_cover_from_wire(1, 50), Some(LindenAmount(50)));
        // Encode: `None` writes the `0` no-cover sentinel; `Some` writes the
        // amount.
        assert_eq!(linden_cover_to_wire("Amount", None)?, 0);
        assert_eq!(linden_cover_to_wire("Amount", Some(&LindenAmount(50)))?, 50);
        // An amount beyond the unsigned 32-bit wire range is rejected.
        assert!(matches!(
            linden_cover_to_wire("Amount", Some(&LindenAmount(u64::from(u32::MAX) + 1))),
            Err(sl_wire::WireError::ValueOutOfRange { .. })
        ));
        Ok(())
    }

    #[test]
    fn linden_wire_round_trips_non_negative_values() -> Result<(), sl_wire::WireError> {
        // Every non-negative wire price decodes losslessly and re-encodes to the
        // exact same `i32`, so the codec boundary is byte-identical.
        for wire in [0_i32, 1, 50, 1000, i32::MAX] {
            let amount = linden_from_wire("Test", wire)?;
            assert_eq!(linden_to_wire("Test", &amount)?, wire);
        }
        // The `0` price (off-sale sentinel) decodes to the zero amount.
        assert_eq!(linden_from_wire("Test", 0)?, LindenAmount(0));
        Ok(())
    }

    #[test]
    fn linden_from_wire_rejects_negative() {
        // A negative L$ value (which a conforming peer never sends) is rejected
        // rather than masked to `0`.
        for wire in [-1_i32, -1000, i32::MIN] {
            assert_eq!(
                linden_from_wire("Test", wire),
                Err(sl_wire::WireError::ValueOutOfRange {
                    field: "Test",
                    value: i64::from(wire),
                })
            );
        }
    }

    #[test]
    fn linden_to_wire_rejects_values_above_the_wire_range() {
        // An amount larger than the signed 32-bit wire field can hold fails the
        // encode loudly instead of silently clamping.
        let too_large = LindenAmount(u64::from(u32::MAX));
        assert_eq!(
            linden_to_wire("Test", &too_large),
            Err(sl_wire::WireError::ValueOutOfRange {
                field: "Test",
                value: i64::from(u32::MAX),
            })
        );
    }

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
    fn optional_key_wire_maps_nil_to_none() {
        // A nil wire UUID is the in-band "absent" sentinel and decodes to `None`;
        // any other value round-trips bit-identically through `Some(K)`.
        let raw = Uuid::from_u128(0xF00D);
        assert_eq!(
            optional_key_from_wire::<TextureKey>(Uuid::nil()),
            None,
            "nil decodes to None"
        );
        assert_eq!(
            optional_key_from_wire::<TextureKey>(raw),
            Some(TextureKey::from(raw)),
        );
        // Encode is the exact inverse: `None` -> nil, `Some(k)` -> the raw bytes.
        assert_eq!(
            optional_key_to_wire(None::<TextureKey>, |k| k.uuid()),
            Uuid::nil()
        );
        assert_eq!(
            optional_key_to_wire(Some(TextureKey::from(raw)), |k| k.uuid()),
            raw,
        );
    }

    #[test]
    fn optional_uuid_wire_maps_nil_to_none() {
        let raw = Uuid::from_u128(0xBEEF);
        assert_eq!(optional_uuid_from_wire(Uuid::nil()), None);
        assert_eq!(optional_uuid_from_wire(raw), Some(raw));
        assert_eq!(optional_uuid_to_wire(None), Uuid::nil());
        assert_eq!(optional_uuid_to_wire(Some(raw)), raw);
    }

    #[test]
    fn optional_numeric_wire_maps_zero_to_none() {
        // `0` is the in-band "unset" sentinel for these numeric fields and
        // decodes to `None`; any other value round-trips bit-identically.
        assert_eq!(optional_u32_from_wire(0), None);
        assert_eq!(optional_u32_from_wire(1_700_000_000), Some(1_700_000_000));
        assert_eq!(optional_u32_to_wire(None), 0);
        assert_eq!(optional_u32_to_wire(Some(1_700_000_000)), 1_700_000_000);

        assert_eq!(optional_i32_from_wire(0), None);
        assert_eq!(optional_i32_from_wire(1024), Some(1024));
        // A negative value is not a sentinel — it round-trips as `Some`.
        assert_eq!(optional_i32_from_wire(-1), Some(-1));
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
