#![doc = include_str!("../README.md")]

mod appearance;
mod error;
pub mod j2c;
mod session;
mod terrain;
mod types;

pub use appearance::{MAX_FACES, decode_texture_entry};
pub use error::Error;
pub use session::{
    CAP_FETCH_INVENTORY, CAP_GET_ASSET, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE,
    CAP_GROUP_MEMBER_DATA, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY, CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPLOAD_BAKED_TEXTURE, REQUESTED_CAPABILITIES, Session,
};
pub use types::{
    ActiveGroup, Asset, AssetType, AvatarAppearance, AvatarAttachment, AvatarGroupMembership,
    AvatarInterests, AvatarPick, AvatarProperties, ChatAudible, ChatMessage, ChatSourceType,
    ChatType, ClickAction, CreateGroupParams, DeRezDestination, DisconnectReason, EconomyData,
    EstateAccessDelta, EstateAccessKind, EstateInfo, Event, Friend, FriendRights, GroupMember,
    GroupMembership, GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle, ImDialog,
    ImageCodec, InstantMessage, InventoryFolder, InventoryItem, InventoryType, LoadUrlRequest,
    LoginHttpRequest, LoginParams, MapItem, MapItemType, MapRegionInfo, Material, Maturity,
    MoneyBalance, MoneyTransaction, MoneyTransactionType, MuteEntry, MuteFlags, MuteType,
    NeighborInfo, Object, ObjectFlagSettings, ObjectMotion, ObjectProperties, ObjectTransform,
    ParcelAccessEntry, ParcelAccessScope, ParcelCategory, ParcelInfo, ParcelMediaCommand,
    ParcelMediaUpdateInfo, ParcelOverlayInfo, ParcelReturnType, ParcelUpdate, PermissionField,
    PlayingAnimation, PrimShape, ProductType, RegionIdentity, RegionInfoUpdate, RegionLimits,
    Reliability, SaleType, ScriptDialog, ScriptPermissionRequest, ScriptPermissions,
    ScriptTeleportRequest, SoundFlags, SoundPreload, TerrainLayerType, TerrainPatch, Texture,
    TextureEntry, TextureFace, Throttle, TransferStatus, Transmit, Wearable, WearableType,
    avatar_texture, grid_to_handle, handle_to_global, handle_to_grid, pcode,
};

// Re-export `Uuid`: it appears in public types (chat/IM ids) and command APIs,
// so consumers can name it without depending on `uuid` directly.
pub use uuid::Uuid;

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AnyMessage, AssetUploadResponse, ControlFlags, EventQueueEvent, EventQueueResponse, Llsd,
    LoginRequest, LoginResponse, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP,
    MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MediaEntry, MfaChallenge, ObjectMediaResponse, ParcelFlags,
    RegionFlags, WireError, build_event_queue_request, build_fetch_inventory_request,
    build_group_member_data_request, build_login_request, build_new_file_agent_inventory_request,
    build_object_media_get_request, build_object_media_navigate_request,
    build_object_media_update_request, build_seed_request, build_update_avatar_appearance_request,
    build_update_item_asset_request, build_upload_baked_texture_request, combine_uuids,
    parse_asset_upload_response, parse_event_queue_response, parse_llsd_xml, parse_login_response,
    parse_seed_response, sim_access,
};
// Re-export the vector and rotation types used by the teleport and movement APIs.
pub use sl_types::lsl::{Rotation, Vector};
// Re-export the L$ amount type used by the money balance/transfer APIs.
pub use sl_types::money::LindenAmount;
