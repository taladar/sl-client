#![doc = include_str!("../README.md")]

mod appearance;
mod error;
mod extra_params;
pub mod j2c;
mod session;
mod terrain;
mod types;

pub use appearance::{MAX_FACES, decode_texture_entry};
pub use error::Error;
pub use session::{
    CAP_AGENT_EXPERIENCES, CAP_EXPERIENCE_PREFERENCES, CAP_FETCH_INVENTORY,
    CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES, CAP_GET_ASSET,
    CAP_GET_CREATOR_EXPERIENCES, CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_MESH,
    CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA,
    CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_MODIFY_MATERIAL_PARAMS,
    CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS,
    CAP_REGION_EXPERIENCES, CAP_RENDER_MATERIALS, CAP_UPDATE_AVATAR_APPEARANCE,
    CAP_UPDATE_EXPERIENCE, CAP_UPDATE_GESTURE_AGENT_INVENTORY, CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY, CAP_UPDATE_SCRIPT_AGENT,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING,
    REQUESTED_CAPABILITIES, Session,
};
pub use types::{
    ActiveGroup, Asset, AssetType, AvatarAppearance, AvatarAttachment, AvatarClassified,
    AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties, ChatAudible, ChatMessage,
    ChatSourceType, ChatType, ClassifiedInfo, ClassifiedUpdate, ClickAction, CreateGroupParams,
    DeRezDestination, DisconnectReason, EconomyData, EstateAccessDelta, EstateAccessKind,
    EstateInfo, Event, ExtendedMesh, FlexibleData, Friend, FriendRights, GroupMember,
    GroupMembership, GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle, ImDialog,
    ImageCodec, InstantMessage, InterestsUpdate, InventoryFolder, InventoryItem, InventoryOffer,
    InventoryType, LightData, LightImage, LoadUrlRequest, LoginHttpRequest, LoginParams, MapItem,
    MapItemType, MapRegionInfo, Material, Maturity, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NeighborInfo, Object, ObjectExtraParams,
    ObjectFlagSettings, ObjectMotion, ObjectProperties, ObjectTransform, ParcelAccessEntry,
    ParcelAccessScope, ParcelCategory, ParcelInfo, ParcelMediaCommand, ParcelMediaUpdateInfo,
    ParcelOverlayInfo, ParcelReturnType, ParcelUpdate, PermissionField, PickInfo, PickUpdate,
    PlayingAnimation, PrimShape, ProductType, ProfileUpdate, ReflectionProbe, RegionIdentity,
    RegionInfoUpdate, RegionLimits, Reliability, RenderMaterialRef, SaleType, ScriptDialog,
    ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest, SculptData, SoundFlags,
    SoundPreload, TerrainLayerType, TerrainPatch, Texture, TextureEntry, TextureFace, Throttle,
    TransferStatus, Transmit, Wearable, WearableType, avatar_texture, grid_to_handle,
    handle_to_global, handle_to_grid, pcode,
};

// Re-export `Uuid`: it appears in public types (chat/IM ids) and command APIs,
// so consumers can name it without depending on `uuid` directly.
pub use uuid::Uuid;

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AnyMessage, AssetUploadResponse, ControlFlags, EventQueueEvent, EventQueueResponse,
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
    GLTF_MATERIAL_OVERRIDE_METHOD, GltfMaterialOverride, IceCandidate, LegacyMaterial, Llsd,
    LoginRequest, LoginResponse, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP,
    MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MaterialOverrideUpdate, MediaEntry, MfaChallenge,
    ObjectMediaResponse, ParcelFlags, ParcelVoiceInfo, RegionFlags, RenderMaterialEntry,
    VOICE_SERVER_TYPE_VIVOX, VOICE_SERVER_TYPE_WEBRTC, VoiceAccountInfo, VoiceProvisionRequest,
    WireError, build_event_queue_request, build_fetch_inventory_request,
    build_group_member_data_request, build_login_request, build_modify_material_params_request,
    build_new_file_agent_inventory_request, build_object_media_get_request,
    build_object_media_navigate_request, build_object_media_update_request,
    build_parcel_voice_info_request, build_provision_voice_account_request,
    build_region_experiences_request, build_render_materials_request, build_seed_request,
    build_set_experience_permission_request, build_update_avatar_appearance_request,
    build_update_experience_request, build_update_item_asset_request,
    build_upload_baked_texture_request, build_voice_signaling_request, combine_uuids,
    experience_id_query, experience_info_query, find_experience_query, forget_experience_query,
    group_experiences_query, parse_asset_upload_response, parse_event_queue_response,
    parse_experience_ids, parse_experience_infos, parse_experience_permissions,
    parse_experience_status, parse_gltf_material_override, parse_llsd_xml, parse_login_response,
    parse_region_experiences, parse_render_materials_response, parse_seed_response, sim_access,
};
// Re-export the vector and rotation types used by the teleport and movement APIs.
pub use sl_types::lsl::{Rotation, Vector};
// Re-export the L$ amount type used by the money balance/transfer APIs.
pub use sl_types::money::LindenAmount;
