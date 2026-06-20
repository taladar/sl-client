#![doc = include_str!("../README.md")]

mod appearance;
mod command;
mod error;
mod extra_params;
pub mod j2c;
mod object_update;
mod particles;
mod session;
mod sim_session;
mod terrain;
mod types;

pub use appearance::{MAX_FACES, decode_texture_entry, encode_texture_entry};
pub use command::Command;
pub use error::Error;
pub use extra_params::encode_extra_params;
pub use object_update::{
    TerseUpdate, encode_compressed_object, encode_object_motion, encode_terse_object_data,
    encode_terse_texture_entry,
};
pub use particles::{
    decode_particle_system, decode_texture_anim, encode_particle_system, encode_texture_anim,
};
pub use session::{
    CAP_AGENT_EXPERIENCES, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_ASSET, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE,
    CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_LIBRARY_API_V3, CAP_MODIFY_MATERIAL_PARAMS,
    CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS,
    CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE, CAP_UPDATE_GESTURE_AGENT_INVENTORY,
    CAP_UPDATE_MATERIAL_AGENT_INVENTORY, CAP_UPDATE_NOTECARD_AGENT_INVENTORY,
    CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPLOAD_BAKED_TEXTURE,
    CAP_VOICE_SIGNALING, RECV_BUFFER_SIZE, REQUESTED_CAPABILITIES, Session,
    ais_inventory_update_to_llsd, build_map_block_reply, build_map_item_reply,
    bulk_update_inventory_to_llsd, chatterbox_invitation_to_llsd, created_category_to_llsd,
    crossed_region_to_caps_llsd, enable_simulator_to_caps_llsd, environment_to_llsd,
    establish_agent_communication_to_llsd, group_members_to_caps_llsd,
    group_memberships_to_caps_llsd, inventory_descendents_to_llsd, offline_messages_to_llsd,
    parcel_info_to_llsd, server_appearance_update_to_llsd, teleport_finish_to_llsd,
};
pub use sim_session::{AgentUpdateInfo, ServerEvent, SimSession};
pub use terrain::encode_layer;
pub use types::{
    ActiveGroup, AlertInfo, Asset, AssetType, AttachmentPoint, AvatarAppearance, AvatarAttachment,
    AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarName, AvatarPick,
    AvatarPickerResult, AvatarProperties, Camera, ChatAudible, ChatMessage, ChatSourceType,
    ChatType, ClassifiedInfo, ClassifiedUpdate, ClickAction, CoarseLocation, CreateGroupParams,
    DayCycle, DayCycleFrame, DeRezDestination, Diagnostic, DirClassifiedResult, DirEventResult,
    DirFindFlags, DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult, DisconnectReason,
    EconomyData, EnvironmentSettings, EstateAccessDelta, EstateAccessKind, EstateCovenant,
    EstateInfo, Event, EventInfo, ExtendedMesh, FlexibleData, Friend, FriendRights, GroupMember,
    GroupMembership, GroupName, GroupNotice, GroupNoticeAttachment, GroupProfile, GroupRole,
    GroupRoleChange, GroupRoleEdit, GroupRoleMember, GroupRoleMemberChange, GroupRoleUpdateType,
    GroupTitle, ImDialog, ImageCodec, InstantMessage, InterestsUpdate, InventoryFolder,
    InventoryItem, InventoryOffer, InventoryType, LandSearchType, LandingType, LightData,
    LightImage, LoadUrlRequest, LoginAccount, LoginHttpRequest, LoginParams, LookAtType, MapItem,
    MapItemType, MapRegionInfo, Material, Maturity, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NameValue, NeighborInfo,
    NewInventoryItem, NotecardRez, Object, ObjectBuyItem, ObjectExtraParams, ObjectFlagSettings,
    ObjectMotion, ObjectProperties, ObjectPropertiesFamily, ObjectTransform, ParcelAccessEntry,
    ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelInfo,
    ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo,
    ParcelRequestResult, ParcelReturnType, ParcelStatus, ParcelUpdate, ParticleSystem,
    PermissionField, PickInfo, PickUpdate, PlacesResult, PlayingAnimation, PointAtType, PrimShape,
    PrimShapeParams, ProductType, ProfileUpdate, ReflectionProbe, RegionChatSettings,
    RegionCombatSettings, RegionIdentity, RegionInfoUpdate, RegionLimits, Reliability,
    RenderMaterialRef, RestoreItem, RezAttachment, SaleType, ScriptDialog, ScriptPermissionRequest,
    ScriptPermissions, ScriptTeleportRequest, SculptData, SkySettings, SoundFlags, SoundPreload,
    TelehubInfo, TeleportFlags, TerrainLayerType, TerrainPatch, Texture, TextureAnimation,
    TextureEntry, TextureFace, Throttle, TransferStatus, Transmit, ViewerEffect, ViewerEffectData,
    ViewerEffectType, WaterSettings, Wearable, WearableType, avatar_texture, global_to_handle,
    grid_to_handle, group_powers, handle_to_global, handle_to_grid, particle_pattern, pcode,
    texture_anim_mode,
};

// Re-export `Uuid`: it appears in public types (chat/IM ids) and command APIs,
// so consumers can name it without depending on `uuid` directly.
pub use uuid::Uuid;

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AisCategoryCreate, AisItemUpdate, AisUpdate, AnyMessage, AssetUploadResponse, ControlFlags,
    CreateInventoryCategoryRequest, DisplayName, EventQueueEvent, EventQueueResponse,
    ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
    GLTF_MATERIAL_OVERRIDE_METHOD, GltfMaterialOverride, HomeLocation, IceCandidate,
    LegacyMaterial, Llsd, LoginRequest, LoginResponse, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE,
    MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MaterialOverrideUpdate, MediaEntry,
    MessageId, MfaChallenge, ObjectMediaResponse, ParcelFlags, ParcelVoiceInfo, RegionFlags,
    RemoteParcelRequest, RenderMaterialEntry, VOICE_SERVER_TYPE_VIVOX, VOICE_SERVER_TYPE_WEBRTC,
    VoiceAccountInfo, VoiceProvisionRequest, WireError, ais_category_children_fetch_url,
    ais_category_children_url, ais_category_url, ais_create_category_url, ais_item_url,
    build_ais_create_category_body, build_ais_move_body, build_ais_rename_category_body,
    build_ais_update_item_body, build_ais_update_response, build_create_inventory_category_request,
    build_create_inventory_category_response, build_display_names_response,
    build_event_queue_request, build_event_queue_response, build_experience_ids_response,
    build_experience_infos_response, build_experience_permissions_response,
    build_experience_status_response, build_fetch_inventory_request, build_gltf_material_override,
    build_group_member_data_request, build_login_request, build_modify_material_params_request,
    build_new_file_agent_inventory_request, build_object_media_get_request,
    build_object_media_navigate_request, build_object_media_update_request,
    build_parcel_voice_info_request, build_parcel_voice_info_response,
    build_provision_voice_account_request, build_provision_voice_account_response,
    build_region_experiences_request, build_region_experiences_response,
    build_remote_parcel_request, build_remote_parcel_response, build_render_materials_request,
    build_render_materials_response, build_seed_request, build_set_experience_permission_request,
    build_update_avatar_appearance_request, build_update_experience_request,
    build_update_item_asset_request, build_upload_baked_texture_request,
    build_voice_signaling_request, combine_uuids, display_names_query, experience_id_query,
    experience_info_query, find_experience_query, forget_experience_query, group_experiences_query,
    parse_ais_category_children_fetch_url, parse_ais_category_children_url, parse_ais_category_url,
    parse_ais_create_category_body, parse_ais_create_category_url, parse_ais_item_url,
    parse_ais_move_body, parse_ais_rename_category_body, parse_ais_update_item_body,
    parse_asset_upload_response, parse_create_inventory_category_request, parse_display_names,
    parse_display_names_query, parse_event_queue_response, parse_experience_id_query,
    parse_experience_ids, parse_experience_info_query, parse_experience_infos,
    parse_experience_permissions, parse_experience_status, parse_find_experience_query,
    parse_forget_experience_query, parse_gltf_material_override, parse_group_experiences_query,
    parse_llsd_xml, parse_login_response, parse_modify_material_params_request,
    parse_provision_voice_account_request, parse_region_experiences,
    parse_region_experiences_request, parse_remote_parcel_reply, parse_remote_parcel_request,
    parse_render_materials_response, parse_seed_response, parse_set_experience_permission_request,
    parse_update_experience_request, parse_voice_signaling_request, sim_access,
};
// Re-export the vector and rotation types used by the teleport and movement APIs.
pub use sl_types::lsl::{Rotation, Vector};
// Re-export the L$ amount type used by the money balance/transfer APIs.
pub use sl_types::money::LindenAmount;
