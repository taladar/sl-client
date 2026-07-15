#![doc = include_str!("../README.md")]

mod appearance;
mod asset_keys;
mod bookkeeping_ids;
mod chat_log;
mod command;
mod error;
mod extra_params;
pub mod j2c;
pub mod mesh_lod;
mod object_update;
mod particles;
mod scoped_id;
mod session;
mod sim_session;
mod terrain;
mod types;

pub use appearance::{MAX_FACES, decode_texture_entry, encode_texture_entry};
pub use asset_keys::{AnimationKey, AssetKey};
pub use bookkeeping_ids::{
    GroupRequestId, ImSessionId, InventoryCallbackId, InvoiceId, LureId, PingId, QueryId,
    TransactionId, TransferId, XferId,
};
pub use chat_log::{
    CONVERSATION_LOG_RETENTION_DAYS, ChatLogConfig, ClientDirectories, ClockStyle,
    ConversationKind, InventoryCacheConfig, LOG_RECALL_SIZE, LogLineTime, LoggedChatType,
    ParsedLogLine, SYSTEM_SENDER_NAME, TimestampFormat, clean_file_name, conference_log_file_name,
    conversation_log_file, conversation_log_line, conversation_log_unix, format_log_line,
    group_log_file_name, im_log_file_name, nearby_log_file_name, parse_log_lines,
};
pub use command::Command;
pub use error::Error;
pub use extra_params::encode_extra_params;
pub use j2c::{DiscardLevel, MAX_DISCARD_LEVEL};
pub use mesh_lod::{DEFAULT_LOD_FACTOR, MESH_LOD_COUNT, MeshLod};
pub use object_update::{
    TerseUpdate, encode_compressed_object, encode_object_motion, encode_terse_object_data,
    encode_terse_texture_entry,
};
pub use particles::{
    decode_particle_system, decode_texture_anim, encode_particle_system, encode_texture_anim,
};
pub use scoped_id::{CircuitId, ScopedObjectId, ScopedParcelId};
pub use session::{
    CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES, CAP_ATTACHMENT_RESOURCES,
    CAP_CHAT_SESSION_REQUEST, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_OBJECT_COST,
    CAP_GET_OBJECT_PHYSICS_DATA, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA,
    CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_LAND_RESOURCES, CAP_LIBRARY_API_V3, CAP_MODIFY_MATERIAL_PARAMS,
    CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_ANIMATION, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT,
    CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_RESOURCE_COST_SELECTED, CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    CAP_SIMULATOR_FEATURES, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE,
    CAP_UPDATE_GESTURE_AGENT_INVENTORY, CAP_UPDATE_MATERIAL_AGENT_INVENTORY,
    CAP_UPDATE_NOTECARD_AGENT_INVENTORY, CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK,
    CAP_UPDATE_SETTINGS_AGENT_INVENTORY, CAP_UPLOAD_BAKED_TEXTURE, CAP_VIEWER_ASSET,
    CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE, CHAT_SESSION_DECLINE_P2P_VOICE,
    ChatLifecycleView, ChatSessionInfo, ChatSessionKind, ChatSessionLifecycle, FolderState,
    FriendPresence, INVENTORY_CACHE_VERSION, INVENTORY_FETCH_MAX_IN_FLIGHT, InventoryOwner,
    InviteChannel, LAND_RESOURCE_DETAIL_TAG, LAND_RESOURCE_SUMMARY_TAG, MessageCursor,
    PendingInvite, RECV_BUFFER_SIZE, REQUESTED_CAPABILITIES, Session, SessionMessage,
    VoiceChannelInfo, VoiceChannelState, agent_drop_group_to_llsd, agent_state_update_to_llsd,
    ais_inventory_update_to_llsd, build_map_block_reply, build_map_item_reply,
    build_map_layer_reply, bulk_update_inventory_to_llsd, chat_session_request_body,
    chatterbox_invitation_to_llsd, created_category_to_llsd, crossed_region_to_caps_llsd,
    display_name_update_to_llsd, enable_simulator_to_caps_llsd, environment_to_llsd,
    establish_agent_communication_to_llsd, group_members_to_caps_llsd,
    group_memberships_to_caps_llsd, inventory_descendents_to_llsd, nav_mesh_status_to_llsd,
    offline_messages_to_llsd, open_region_info_to_llsd, parcel_info_to_llsd,
    required_voice_version_to_llsd, server_appearance_update_to_llsd,
    set_display_name_reply_to_llsd, sim_console_response_to_llsd, teleport_finish_to_llsd,
    windlight_refresh_to_llsd,
};
pub use sim_session::{AgentUpdateInfo, ServerEvent, SimSession};
pub use terrain::encode_layer;
pub use types::{
    ActiveGroup, AgentOrObjectKey, AlertInfo, Asset, AssetType, AttachmentMode, AttachmentPoint,
    AvatarAppearance, AvatarAttachment, AvatarClassified, AvatarGroupMembership, AvatarInterests,
    AvatarName, AvatarPick, AvatarPickerResult, AvatarProperties, Camera, CameraError, ChatAudible,
    ChatMessage, ChatSource, ChatSourceType, ChatType, ChatTypeNotAVolume, Child,
    ClassifiedCategory, ClassifiedInfo, ClassifiedUpdate, ClickAction, CloudPosDensity,
    CoarseLocation, Color, ColorAlpha, CreateGroupParams, DEFAULT_LSL_SCRIPT, DEFAULT_LUAU_SCRIPT,
    DEFAULT_TERRAIN_DETAIL_TEXTURES, DayCycle, DayCycleFrame, DeRezDestination, DetachOrder,
    Diagnostic, DirClassifiedResult, DirEventResult, DirFindFlags, DirGroupResult, DirLandResult,
    DirPeopleResult, DirPlaceResult, DirectoryVisibility, DisconnectReason, DisplayNameUpdate,
    EconomyData, EjectAction, EnvironmentSettings, EstateAccessDelta, EstateAccessKind,
    EstateCovenant, EstateInfo, Event, EventId, EventInfo, ExtendedMesh, FeatureDisabled,
    FlexibleData, FolderInfo, FolderType, FollowCamProperty, FollowCamPropertyValue, FreezeAction,
    Friend, FriendRights, GenericMessage, GenericStreamingMessage, GestureActivation, Glow,
    GodRegionUpdate, GroupAccountDetails, GroupAccountDetailsEntry, GroupAccountSummary,
    GroupAccountTransaction, GroupAccountTransactions, GroupActiveProposalItem, GroupMember,
    GroupMembership, GroupName, GroupNotice, GroupNoticeAttachment, GroupNoticeKey, GroupProfile,
    GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, GroupVote, GroupVoteHistoryItem,
    ImDialog, ImageCodec, InstantMessage, InterestsUpdate, InventoryCursor, InventoryFolder,
    InventoryItem, InventoryItemMove, InventoryItemOrFolderKey, InventoryOffer, InventoryType,
    ItemInfo, Kick, Kilobits, LandArea, LandBrushAction, LandBrushSize, LandEdit, LandImpact,
    LandSearchType, LandStatItem, LandStatReportType, LandingType, LightData, LightImage,
    LindenBalance, LoadUrlRequest, LoginAccount, LoginHttpRequest, LoginParams, LookAtType,
    MapItem, MapItemType, MapLayer, MapRegionInfo, MapRequestFlags, Material, Maturity,
    MeanCollision, MeanCollisionType, MeshKey, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MovementMode, MuteEntry, MuteFlags, MuteType, NameValue,
    NavMeshBuildStatus, NavMeshStatus, NegativeBalanceError, NeighborInfo, NewInventoryItem,
    NewInventoryLink, NotUpdatableAssetType, NotecardRez, Object, ObjectBuyItem, ObjectExtraParams,
    ObjectFlagSettings, ObjectMotion, ObjectPlayingAnimation, ObjectProperties,
    ObjectPropertiesFamily, ObjectTransform, OpenRegionInfo, ParcelAccessEntry, ParcelAccessFlags,
    ParcelAccessScope, ParcelCategory, ParcelDetails, ParcelInfo, ParcelMediaCommand,
    ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate, ParticleSystem, PermissionField, PermissionRole,
    PickInfo, PickKey, PickUpdate, PlacesResult, PlayingAnimation, PointAtType, Postcard,
    PrimShape, PrimShapeParams, ProductType, ProfileUpdate, ProposalCandidateId, ProposalVoteId,
    ReflectionProbe, RegionChatSettings, RegionCombatSettings, RegionIdentity, RegionInfoUpdate,
    RegionLimits, RegionStats, RegionTerrainComposition, Reliability, RenderMaterialRef,
    RequiredVoiceVersion, RestoreItem, RezAttachment, RezObjectParams, RezScriptParams, SaleType,
    Scale, ScriptCompileError, ScriptControl, ScriptControlAction, ScriptControlsInfo,
    ScriptDialog, ScriptGrantInfo, ScriptLanguage, ScriptPermissionRequest, ScriptPermissionState,
    ScriptPermissionStatus, ScriptPermissions, ScriptTarget, ScriptTeleportRequest,
    ScriptUploadLocation, SculptData, SculptOrMeshKey, ServerError, SetDisplayNameReply, SimStatId,
    SimWideDeleteFlags, SimulatorTime, SkySettings, SoundFlags, SoundPreload, StartLocationSlot,
    SurfaceInfo, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, TelehubInfo,
    TeleportFlags, TerraformArea, TerrainLayerType, TerrainPatch, Texture, TextureAnimation,
    TextureEntry, TextureFace, Throttle, ThrottleBuilder, ThrottleError, TransferStatus, Transmit,
    UpdatableAssetType, UpdateGroupInfoParams, UserInfo, ViewerEffect, ViewerEffectData,
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
    AbuseReport, AbuseReportType, AgentPreferences, AisCategoryCreate, AisItemUpdate, AisUpdate,
    AnimatedObjects, AnyMessage, AssetUploadResponse, AttachmentLocation,
    AttachmentResourcesReport, CircuitCode, ControlFlags, CreateInventoryCategoryRequest,
    Direction, DisplayName, EventQueueEvent, EventQueueResponse, ExperienceInfo,
    ExperiencePermission, ExperienceProperties, ExperienceUpdate, GLTF_MATERIAL_OVERRIDE_METHOD,
    GlobalCoordinates, GltfMaterialOverride, HomeLocation, IceCandidate, LandResourcesUrls,
    LegacyMaterial, Llsd, LoginFailure, LoginRejectKind, LoginRequest, LoginResponse,
    MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER,
    MaterialOverrideUpdate, MediaEntry, MessageId, MfaChallenge, ObjectCost, ObjectMediaResponse,
    ObjectPermMasks, ObjectPhysicsData, OpenSimExtras, ParcelFlags, ParcelScriptResources,
    ParcelVoiceInfo, Permissions, Permissions5, PhysicsShapeType, PhysicsShapeTypes,
    ReflectionProbeFlags, RegionFlags, RegionHandle, RegionLocalObjectId, RegionLocalParcelId,
    RemoteParcelRequest, RenderMaterialEntry, ResourceAmount, ResourceSummary, ScriptedObjectInfo,
    ScriptedObjectResources, SelectedCostKind, SelectedResourceCost, SequenceNumber,
    SimulatorFeatures, StartLocation, StartLocationParseError, VOICE_SERVER_TYPE_VIVOX,
    VOICE_SERVER_TYPE_WEBRTC, VoiceAccountInfo, VoiceProvisionRequest, WireError,
    ais_category_children_fetch_url, ais_category_children_url, ais_category_url,
    ais_create_category_url, ais_item_url, build_agent_preferences_request,
    build_agent_preferences_response, build_ais_create_category_body, build_ais_move_body,
    build_ais_rename_category_body, build_ais_update_item_body, build_ais_update_response,
    build_attachment_resources_response, build_create_inventory_category_request,
    build_create_inventory_category_response, build_display_names_response,
    build_event_queue_request, build_event_queue_response, build_experience_ids_response,
    build_experience_infos_response, build_experience_permissions_response,
    build_experience_status_response, build_fetch_inventory_request, build_get_object_cost_request,
    build_get_object_cost_response, build_get_object_physics_data_request,
    build_get_object_physics_data_response, build_gltf_material_override,
    build_group_member_data_request, build_land_resource_detail_response,
    build_land_resource_summary_response, build_land_resources_request,
    build_land_resources_response, build_login_request, build_modify_material_params_request,
    build_new_file_agent_inventory_request, build_object_media_get_request,
    build_object_media_navigate_request, build_object_media_update_request,
    build_object_physics_properties, build_parcel_voice_info_request,
    build_parcel_voice_info_response, build_provision_voice_account_request,
    build_provision_voice_account_response, build_region_experiences_request,
    build_region_experiences_response, build_remote_parcel_request, build_remote_parcel_response,
    build_render_materials_request, build_render_materials_response,
    build_resource_cost_selected_request, build_resource_cost_selected_response,
    build_seed_request, build_send_user_report, build_set_experience_permission_request,
    build_simulator_features_response, build_update_avatar_appearance_request,
    build_update_experience_request, build_update_item_asset_request,
    build_update_script_agent_request, build_update_script_task_request,
    build_upload_baked_texture_request, build_voice_signaling_request, combine_uuids,
    display_names_query, experience_id_query, experience_info_query, find_experience_query,
    forget_experience_query, group_experiences_query, parse_agent_preferences,
    parse_ais_category_children_fetch_url, parse_ais_category_children_url, parse_ais_category_url,
    parse_ais_create_category_body, parse_ais_create_category_url, parse_ais_item_url,
    parse_ais_move_body, parse_ais_rename_category_body, parse_ais_update_item_body,
    parse_asset_upload_response, parse_attachment_resources,
    parse_create_inventory_category_request, parse_display_names, parse_display_names_query,
    parse_event_queue_response, parse_experience_id_query, parse_experience_ids,
    parse_experience_info_query, parse_experience_infos, parse_experience_permissions,
    parse_experience_status, parse_find_experience_query, parse_forget_experience_query,
    parse_get_object_cost, parse_get_object_physics_data, parse_get_object_physics_data_request,
    parse_gltf_material_override, parse_group_experiences_query, parse_land_resource_detail,
    parse_land_resource_summary, parse_land_resources_reply, parse_land_resources_request,
    parse_llsd_xml, parse_login_response, parse_modify_material_params_request,
    parse_object_physics_properties, parse_provision_voice_account_request,
    parse_region_experiences, parse_region_experiences_request, parse_remote_parcel_reply,
    parse_remote_parcel_request, parse_render_materials_response, parse_resource_cost_selected,
    parse_resource_cost_selected_request, parse_seed_response, parse_send_user_report,
    parse_set_experience_permission_request, parse_simulator_features,
    parse_update_experience_request, parse_voice_signaling_request, region_name_from_wire,
    region_name_to_wire, sim_access,
};
// Re-export the chat channel type used by the local-chat / script-dialog APIs.
pub use sl_types::chat::ChatChannel;
// Re-export the region-name and map-geometry types used by the region/map/
// teleport reply types. `GridCoordinates` carries a region's grid index pair;
// `RegionCoordinates` carries a region-local position (the teleport target).
pub use sl_types::map::{
    Distance, GridCoordinates, GridRectangle, GridRectangleLike, RegionCoordinates, RegionName,
};
// Re-export the vector and rotation types used by the teleport and movement APIs.
pub use sl_types::lsl::{Rotation, Vector};
// Re-export the L$ amount type used by the money balance/transfer APIs.
pub use sl_types::key::{
    AgentKey, ClassifiedKey, ExperienceKey, FriendKey, GroupKey, InventoryFolderKey, InventoryKey,
    Key, ObjectKey, OwnerKey, ParcelKey, TextureKey,
};
pub use sl_types::money::LindenAmount;
