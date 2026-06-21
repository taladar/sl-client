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

pub use alert::{MeanCollision, MeanCollisionType};
pub use appearance::{
    AttachmentPoint, AvatarAppearance, AvatarAttachment, PlayingAnimation, RezAttachment,
    SoundFlags, SoundPreload, TextureEntry, TextureFace, Wearable, WearableType, avatar_texture,
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
    DirLandResult, DirPeopleResult, DirPlaceResult, EventInfo, LandSearchType, PlacesResult,
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
    GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, GroupVote, GroupVoteHistoryItem,
    group_powers,
};
pub use inventory::{
    GestureActivation, InventoryFolder, InventoryItem, NewInventoryItem, global_to_handle,
    grid_to_handle, handle_to_global, handle_to_grid,
};
pub use map::{
    EstateAccessDelta, EstateAccessKind, EstateCovenant, EstateInfo, MapItem, MapItemType,
    MapLayer, MapRegionInfo, NeighborInfo, RegionInfoUpdate, TelehubInfo,
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
    MuteType, ScriptControl, ScriptDialog, ScriptPermissionRequest, ScriptPermissions,
    ScriptTeleportRequest,
};
pub use session::{
    Camera, DisconnectReason, LoginHttpRequest, LoginParams, Reliability, Throttle, Transmit,
};
pub use terrain::{TerrainLayerType, TerrainPatch};
