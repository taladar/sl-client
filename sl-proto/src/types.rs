//! Public value types of the sans-I/O session: its inputs and outputs.
//!
//! The types are grouped into feature submodules (object, parcel, group, …) and
//! re-exported here, so the crate's public surface (`crate::types::*`, re-exported
//! again from `lib.rs`) is unchanged by that internal split.

mod appearance;
mod asset;
mod avatar_profile;
mod chat;
mod diagnostic;
mod economy;
mod editing;
mod environment;
mod event;
mod group;
mod inventory;
mod map;
mod name;
mod object;
mod parcel;
mod region;
mod script;
mod session;
mod terrain;

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
pub use economy::{EconomyData, MoneyBalance, MoneyTransaction, MoneyTransactionType};
pub use editing::{
    ClickAction, DeRezDestination, Material, Maturity, ObjectFlagSettings, ObjectTransform,
    PermissionField, PrimShape, ProductType, SaleType, TeleportFlags,
};
pub use environment::{DayCycle, DayCycleFrame, EnvironmentSettings, SkySettings, WaterSettings};
pub use event::Event;
pub use group::{
    ActiveGroup, CreateGroupParams, GroupMember, GroupMembership, GroupNotice,
    GroupNoticeAttachment, GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit,
    GroupRoleMember, GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, group_powers,
};
pub use inventory::{
    InventoryFolder, InventoryItem, NewInventoryItem, global_to_handle, grid_to_handle,
    handle_to_global, handle_to_grid,
};
pub use map::{
    EstateAccessDelta, EstateAccessKind, EstateInfo, MapItem, MapItemType, MapRegionInfo,
    NeighborInfo, RegionInfoUpdate,
};
pub use name::{AvatarName, GroupName};
pub use object::{
    ExtendedMesh, FlexibleData, LightData, LightImage, NameValue, Object, ObjectExtraParams,
    ObjectMotion, ObjectProperties, ParticleSystem, PrimShapeParams, ReflectionProbe,
    RenderMaterialRef, SculptData, TextureAnimation, particle_pattern, pcode, texture_anim_mode,
};
pub use parcel::{
    LandingType, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory,
    ParcelInfo, ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate,
};
pub use region::{RegionChatSettings, RegionCombatSettings, RegionIdentity, RegionLimits};
pub use script::{
    AlertInfo, LoadUrlRequest, MuteEntry, MuteFlags, MuteType, ScriptDialog,
    ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest,
};
pub use session::{
    Camera, DisconnectReason, LoginHttpRequest, LoginParams, Reliability, Throttle, Transmit,
};
pub use terrain::{TerrainLayerType, TerrainPatch};
