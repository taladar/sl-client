#![doc = include_str!("../README.md")]

use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use bevy::prelude::*;

use std::collections::{BTreeSet, HashMap};

use sl_proto::{
    AVATAR_PICKER_PAGE_SIZE, CAP_ACCEPT_GROUP_INVITE, CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES,
    CAP_ATTACHMENT_RESOURCES, CAP_AVATAR_PICKER_SEARCH, CAP_CHAT_SESSION_REQUEST,
    CAP_COPY_INVENTORY_FROM_NOTECARD, CAP_CREATE_INVENTORY_CATEGORY, CAP_DECLINE_GROUP_INVITE,
    CAP_DIRECT_DELIVERY, CAP_EXPERIENCE_PREFERENCES, CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY,
    CAP_FETCH_LIBRARY, CAP_FIND_EXPERIENCE_BY_NAME, CAP_GET_ADMIN_EXPERIENCES,
    CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES, CAP_GET_EXPERIENCE_INFO,
    CAP_GET_EXPERIENCES, CAP_GET_OBJECT_PHYSICS_DATA, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA,
    CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_LAND_RESOURCES, CAP_LSL_SYNTAX, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO,
    CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES,
    CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS, CAP_RESOURCE_COST_SELECTED,
    CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT, CAP_SIMULATOR_FEATURES,
    CAP_UPDATE_EXPERIENCE, CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK, CAP_USER_INFO,
    CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE, CHAT_SESSION_DECLINE_P2P_VOICE,
    CHAT_SESSION_FETCH_HISTORY, CHAT_SESSION_INVITE, CHAT_SESSION_START_CONFERENCE,
    Event as SessionEvent, INVENTORY_FETCH_MAX_IN_FLIGHT, Llsd, LoginResponse, RECV_BUFFER_SIZE,
    SelectedCostKind, Session, SessionMessage, UserInfoUpdate, ais_category_children_fetch_url,
    ais_category_children_url, ais_category_url, ais_create_category_url, ais_item_url,
    associate_inventory_request, avatar_picker_search_query, build_agent_preferences_request,
    build_ais_create_category_body, build_ais_create_link_body, build_ais_move_body,
    build_ais_rename_category_body, build_ais_update_item_body,
    build_create_inventory_category_request, build_environment_update_request,
    build_get_object_cost_request, build_get_object_physics_data_request,
    build_modify_material_params_request, build_object_media_navigate_request,
    build_object_media_update_request, build_parcel_voice_info_request,
    build_provision_voice_account_request, build_region_experiences_request,
    build_remote_parcel_request, build_render_materials_put_request,
    build_resource_cost_selected_request, build_send_user_report,
    build_set_experience_permission_request, build_update_experience_request,
    build_update_item_asset_request, build_update_script_agent_request,
    build_update_script_task_request, build_update_task_item_asset_request,
    build_upload_baked_texture_request, build_user_info_update, build_voice_signaling_request,
    chat_session_agents_body, chat_session_request_body, copy_inventory_from_notecard_body,
    create_listing_request, delete_listing_request, display_names_query, experience_id_query,
    experience_info_query, find_experience_query, forget_experience_query, group_experiences_query,
    group_invite_response_body, listing_request, listings_request, merchant_status_request,
    parse_login_response, update_listing_request,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    ActiveGroup, AgentKey, AgentOrObjectKey, AgentPreferences, AnimatedObjects, AnimationKey,
    AnyMessage, AssetKey, AssetUpdateLocation, AssociateInventory, AttachmentMode, AttachmentPoint,
    AvatarAppearance, AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarName,
    AvatarPick, AvatarPickerResult, AvatarProperties, Camera, CameraError, ChatAudible,
    ChatChannel, ChatLogConfig, ChatMessage, ChatSessionKind, ChatSource, ChatSourceType, ChatType,
    ChatTypeNotAVolume, Child, CircuitCode, CircuitId, ClassifiedCategory, ClassifiedInfo,
    ClassifiedKey, ClassifiedUpdate, ClickAction, ClientDirectories, ClockStyle, CoarseLocation,
    Color, ColorAlpha, Command, ControlFlags, ConversationKind, CreateGroupParams, CreateListing,
    DayCycle, DayCycleFrame, DeRezDestination, DetachOrder, Diagnostic, DirClassifiedResult,
    DirEventResult, DirFindFlags, DirGroupResult, DirLandResult, DirPeopleResult, DirPlaceResult,
    Direction, DirectoryVisibility, DisconnectReason, DisplayName, DisplayNameUpdate, Distance,
    EconomyData, EnvironmentAsset, EnvironmentSettings, EstateAccessDelta, EstateAccessKind,
    EstateCovenant, EstateFlags, EstateInfo, EstateInfoUpdate, EventId, EventInfo, ExperienceInfo,
    ExperienceKey, ExperiencePermission, ExperienceProperties, ExperienceUpdate, ExtendedMesh,
    FaceMaterialPut, FlexibleData, FolderInfo, FolderState, FolderType, Friend, FriendKey,
    FriendPresence, FriendRights, GestureActivation, GlobalCoordinates, Glow, GltfMaterialOverride,
    GridCoordinates, GroupInvitationReceived, GroupKey, GroupMember, GroupMembership, GroupNotice,
    GroupNoticeAttachment, GroupNoticeItem, GroupNoticeKey, GroupNoticeReceived, GroupProfile,
    GroupRequestId, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, HomeLocation, IceCandidate, ImDialog,
    ImSessionId, InstantMessage, InterestsUpdate, InventoryCacheConfig, InventoryCallbackId,
    InventoryCursor, InventoryFolder, InventoryFolderKey, InventoryItem, InventoryItemOrFolderKey,
    InventoryKey, InventoryOffer, InventoryOwner, InventoryType, ItemInfo, Key, Kilobits, LandArea,
    LandImpact, LandSearchType, LandingType, LegacyMaterial, LightData, LightImage, LindenAmount,
    LindenBalance, Listing, ListingId, LoadUrlRequest, LoggedChatType, LoginAccount, LoginFailure,
    LoginParams, LoginRejectKind, LoginRequest, LookAtType, LureId, MAX_FACES, MEDIA_PERM_ALL,
    MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MapItem, MapItemType,
    MapRegionInfo, MarketplaceApiError, MarketplaceApiErrorKind, MarketplaceAssociateInventoryInfo,
    MarketplaceInventoryInfo, MarketplaceOperation, Material, MaterialOverrideUpdate, Maturity,
    MediaEntry, MerchantStatus, MeshKey, MessageCursor, MfaChallenge, MoneyBalance,
    MoneyTransaction, MoneyTransactionType, MovementMode, MuteEntry, MuteFlags, MuteType,
    NearbyHistoryLine, NegativeBalanceError, NeighborInfo, NewInventoryItem, NewInventoryLink,
    Object, ObjectExtraParams, ObjectFlagSettings, ObjectKey, ObjectMediaResponse, ObjectMotion,
    ObjectPermMasks, ObjectPhysicsData, ObjectPlayingAnimation, ObjectProperties,
    ObjectPropertiesFamily, ObjectTransform, OpenRegionInfo, OpenSimExtras, OwnerKey,
    ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelDetails,
    ParcelFlags, ParcelInfo, ParcelKey, ParcelMediaCommand, ParcelMediaUpdateInfo,
    ParcelObjectOwner, ParcelOverlayCell, ParcelOverlayGrid, ParcelOverlayInfo, ParcelOwnership,
    ParcelRequestResult, ParcelReturnType, ParcelStatus, ParcelUpdate, ParcelVoiceInfo,
    ParticleSystem, PermissionField, Permissions, Permissions5, PhysicsShapeType,
    PhysicsShapeTypes, PickInfo, PickKey, PickUpdate, PingId, PlayingAnimation, PointAtType,
    PrimShape, PrimShapeParams, ProductType, ProfileUpdate, ProposalCandidateId, ProposalVoteId,
    QueryId, ReflectionProbe, ReflectionProbeFlags, RegionChatSettings, RegionCombatSettings,
    RegionCoordinates, RegionDebugUpdate, RegionFlags, RegionHandle, RegionIdentity,
    RegionInfoUpdate, RegionLimits, RegionLocalObjectId, RegionLocalParcelId, RegionName,
    RegionTerrainComposition, RegionTerrainUpdate, Reliability, RenderMaterialEntry,
    RenderMaterialRef, RestoreItem, RezAttachment, RezObjectParams, RezScriptParams, Rotation,
    SaleType, ScopedObjectId, ScopedParcelId, ScriptCompileError, ScriptControl,
    ScriptControlAction, ScriptDialog, ScriptLanguage, ScriptPermissionRequest, ScriptPermissions,
    ScriptTarget, ScriptTeleportRequest, ScriptUploadLocation, SculptData, SculptOrMeshKey,
    SequenceNumber, ServerHistoryMessage, SetDisplayNameReply, SimulatorFeatures, SkySettings,
    SoundFlags, SoundPreload, StartLocation, StartLocationParseError, StartLocationSlot,
    SurfaceInfo, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, TerrainLayerType,
    TerrainPatch, TextureAnimation, TextureEntry, TextureFace, TextureKey, Throttle,
    ThrottleBuilder, ThrottleError, TimestampFormat, TransactionId, TransferId, Transmit,
    UpdatableAssetType, UpdateGroupInfoParams, UpdateListing, UserInfo, Uuid, Vector, ViewerEffect,
    ViewerEffectData, ViewerEffectType, VoiceAccountInfo, VoiceProvisionRequest, WaterSettings,
    Wearable, WearableType, XferId, avatar_texture, azimuth_altitude_to_rotation,
    decode_particle_system, decode_texture_anim, decode_texture_entry, encode_texture_entry,
    environment_asset_from_bytes, grid_to_handle, group_powers, handle_to_global, handle_to_grid,
    particle_pattern, pcode, sim_access, texture_anim_mode,
};
#[doc(no_inline)]
pub use sl_proto::{Asset, AssetType, ImageCodec, Texture, TransferStatus};
pub use sl_proto::{WireLandmarkAsset, landmark_to_wire, parse_landmark};
// The `GetTexture` capability name, so a frontend driving the texture store
// directly (rather than the `Command::FetchTexture` path) can resolve the cap
// URL from an [`SlCapabilities`] map and hand it to a [`BevyTextureFetcher`].
pub use sl_proto::CAP_GET_TEXTURE;
// The `UpdateAvatarAppearance` capability name, so a frontend can detect the
// modern server-side (central) bake and trigger it — a POST driven by the
// `Command::RequestServerAppearanceUpdate` command — from an [`SlCapabilities`]
// map, without depending on `sl_proto` directly.
pub use sl_proto::CAP_UPDATE_AVATAR_APPEARANCE;
// The `GetMesh2` / `GetMesh` capability names, the mesh counterpart of
// [`CAP_GET_TEXTURE`]: a frontend driving the mesh store directly (rather than the
// `Command::FetchMesh` path) resolves the cap URL from an [`SlCapabilities`] map
// and hands it to a [`BevyMeshFetcher`].
pub use sl_proto::{CAP_GET_MESH, CAP_GET_MESH2};
// The `ViewerAsset` capability name, the generic-asset counterpart of
// [`CAP_GET_TEXTURE`]: a frontend driving the [`AssetStore`] directly resolves
// the cap URL from an [`SlCapabilities`] map and hands it to a
// [`BevyAssetFetcher`] (used to fetch worn wearable assets for client-side
// baking).
pub use sl_proto::CAP_GET_OBJECT_COST;
pub use sl_proto::CAP_UPLOAD_BAKED_TEXTURE;
pub use sl_proto::CAP_VIEWER_ASSET;
pub use sl_proto::{DisconnectReason as SessionDisconnectReason, Event as SlSessionEvent};
// The decoding, LOD-aware texture store, re-exported so a Bevy app can build and
// drive one (`sl_texture::TextureEntry`/`TextureReadLease` stay accessible as
// `sl_texture::…` to avoid colliding with `sl_proto`'s prim-face `TextureEntry`).
pub use sl_proto::DiscardLevel;
// `StoreStats` / `GateStats` (the pipeline-status snapshots) are the same shared
// `sl-asset-sched` types across the texture / mesh / asset stores, so they are
// re-exported once here (from `sl_texture`) rather than three times.
pub use sl_texture::{
    AssetFetcher, CacheLimits, DecodedImage as DecodedTexture, FULL_RESOLUTION_PIXEL_AREA,
    FetchChunk, GateStats, NotRemotelyFetchable, Priority, RemoteTextureSource, ScreenMetrics,
    StoreStats, TextureError, TextureFetchType, TextureFetcher, TextureProgress, TextureRequest,
    TextureStore,
};
// The decoding, LOD-aware mesh store (the mesh counterpart of the texture
// store). `Priority` and `MeshKey` are already re-exported (from `sl_texture` /
// `sl_proto`); the mesh `CacheLimits` is aliased so it does not collide with the
// texture one.
pub use sl_mesh::{
    AssetBytes, CacheLimits as MeshCacheLimits, DEFAULT_LOD_FACTOR, DecodedMesh, MeshDiskCache,
    MeshEntry, MeshError, MeshFetcher, MeshHeader, MeshLod, MeshPhysics, MeshProgress,
    MeshReadLease, MeshRequest, MeshSkin, MeshStore, PhysicsConvex, Submesh, VertexWeights,
};

// The generic-asset store (the opaque-blob counterpart of the texture/mesh
// stores), fetched whole over the `ViewerAsset` capability. Its `CacheLimits` is
// aliased so it does not collide with the texture/mesh ones; `Priority`,
// `AssetKey`, and `AssetType` are already re-exported.
pub use sl_asset::{
    AssetDiskCache, AssetEntry, AssetError, AssetProgress, AssetRef, AssetStore, BlobFetcher,
    CacheLimits as AssetCacheLimits,
};

// The GLTF (PBR) render-material asset decoder (`AT_MATERIAL`), the material
// counterpart of `sl_mesh` / `sl_texture`: the viewer fetches a material asset
// over the `ViewerAsset` capability (the generic `AssetStore` above) and decodes
// it into a `GltfMaterial` it maps onto a Bevy `StandardMaterial`, sourcing each
// referenced texture from the shared `TextureStore` (P27.1). `MaterialOverride`
// (P27.2) is the per-face delta the viewer layers on the base material, decoded
// from a GLTF material-override `GenericStreamingMessage`.
pub use sl_material::{
    GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform,
    MaterialError as GltfMaterialError, MaterialOverride, TextureOverride,
    TextureTransformOverride, encode_material_asset, encode_override_gltf_json,
    parse_gltf_material_document, parse_material_asset, parse_material_override,
};

// The pure prim-tessellation geometry (no store/fetcher — a prim is tessellated
// on the CPU from its shape parameters, not fetched). Re-exported so the viewer
// can dequantize a `PrimShapeParams` into a float shape, tessellate it at a
// `PrimLod`, and feed the resulting faces through `to_bevy_prim_mesh`. The
// dequantized float shape is aliased `PrimShapeFloat` so it does not collide
// with `sl_proto`'s quantized rez-params `PrimShape`.
pub use sl_prim::{
    FlexiAttributes, FlexiChain, HoleType, PRIM_LOD_COUNT, PathCurve, PrimFace, PrimFaceId,
    PrimLod, PrimMesh, PrimShape as PrimShapeFloat, ProfileCurve, lod_triangle_counts, tessellate,
    tessellate_with_path,
};

// The pure sculpt-texture tessellation geometry (the sculpt counterpart of
// `sl_prim`; likewise no store/fetcher — a sculpt is stitched on the CPU from a
// decoded sculpt map, which the viewer sources from the shared `TextureStore`).
// Re-exported so the viewer can feed a `DecodedTexture` (= `sl_texture`'s
// `DecodedImage`) plus the object's `sculpt_type` byte into the stitcher and feed
// the resulting `PrimMesh` faces through `to_bevy_prim_mesh`. Its `tessellate` is
// aliased `tessellate_sculpt` so it does not collide with `sl_prim`'s prim
// `tessellate`; `PrimFace` / `PrimMesh` are already re-exported (from `sl_prim`).
// The function is taken by its module-qualified path (`tessellate::tessellate`)
// so only the value is aliased — `sl_sculpt::tessellate` is *both* a module and a
// function, and a bare `tessellate as …` would rename both and make doc links to
// the name ambiguous.
pub use sl_sculpt::tessellate::tessellate as tessellate_sculpt;
pub use sl_sculpt::{SculptParams, SculptStitch};

// The pure Linden-tree geometry (the `LLVOTree` counterpart of `sl_prim` /
// `sl_sculpt`; likewise no store/fetcher — a tree's branch / leaf geometry is
// generated on the CPU from its species-table entry, selected by the object's
// `state` byte). Re-exported so the viewer can look up a `TreeSpecies`, generate
// its `TreeMesh` at a `TreeLod` (or a distance `billboard_geometry` imposter),
// and feed it through [`to_bevy_tree_mesh`]. `RADIUS_SCALE_FACTOR` / `YAW_DEGREES`
// are the outer-scale placement constants the viewer applies at the transform.
pub use sl_tree::{
    RADIUS_SCALE_FACTOR as TREE_RADIUS_SCALE_FACTOR, TreeLod, TreeMesh, TreeSpecies,
    YAW_DEGREES as TREE_YAW_DEGREES, billboard_geometry as tree_billboard_geometry, tree_geometry,
    tree_species,
};

// The pure Linden-grass geometry (the `LLVOGrass` counterpart of the tree path
// above; likewise no store/fetcher — a grass clump's blade geometry is generated
// on the CPU from its species-table entry, selected by the object's `state` byte,
// and the object scale). Re-exported so the viewer can look up a `GrassSpecies`,
// generate its `GrassMesh`, and feed it through [`to_bevy_grass_mesh`].
pub use sl_tree::{GRASS_MAX_BLADES, GrassMesh, GrassSpecies, grass_geometry, grass_species};

// The pure system-avatar decoders (skeleton / base body / visual params), the
// avatar counterpart of `sl_mesh` / `sl_texture`. Re-exported so the viewer can
// parse the standard Linden `character/` assets and drive them through
// [`to_bevy_base_mesh`] / [`BevySkeleton`]. `AttachmentPoint` is already
// re-exported (from `sl_proto`, which `sl_avatar` re-exports too).
pub use sl_avatar::{
    AppearanceValues, AttachmentPointDef, AttachmentPoints, BaseMesh, BaseMeshError, BodyPhysics,
    BodyPhysicsState, BoneDeform, CollisionVolume, ColorOp, ColorRamp, HAND_POSE_MORPH_PARAMS,
    Joint, JointSample, MaskTexture, MorphMask, MorphMasks, MorphWeights, MorphedMesh,
    PHYSICS_MORPH_PARAMS, ParamEffect, ParamError, ParamGroup, ParamSex, PartMorphMask,
    PhysicsDrivenParam, PhysicsMotion, PhysicsMotionConfig, PhysicsSettings, RUNTIME_MORPH_PARAMS,
    ResolvedParams, SaleType as WearableSaleType, SkeletalDeformations, Skeleton, SkeletonError,
    VisualParam, VisualParams, VolumeDeform, VolumeDeformations, VolumeMorph, WearableAsset,
    WearableError, WearablePermissions, combine_layer_color, global_color, global_color_params,
    hand_pose_morph_param, is_runtime_morph_param,
};

// The client-side avatar baker (`sl-bake`, the OpenSim / legacy path): compose a
// bake region from ordered wearable layers, and the per-region layer plan (P15).
pub use sl_bake::{
    BakeRegion, BakedImage, Layer, LayerKind, LayerSource, LayerTint, PlannedLayer, ShapeMask,
    ShapeMaskSpec, TexGen, composite_region, region_layers, region_plan, shape_mask_files,
    static_layer_files,
};

pub use crate::animations::{SampledJoint, sample_motion};
pub use crate::assets::BevyAssetFetcher;
pub use crate::avatars::{
    AnimationPose, BaseMeshSkin, BevySkeleton, BodySizeMetrics, JointOverrides,
    RuntimeMorphTargets, joint_position_overrides, to_bevy_base_mesh, to_bevy_morphed_mesh,
    to_bevy_runtime_morph_targets,
};
#[cfg(feature = "bevy_pbr")]
pub use crate::clouds::{CloudMaterial, CloudMaterialPlugin, CloudParams};
pub use crate::grass::to_bevy_grass_mesh;
pub use crate::meshes::{
    BevyMeshFetcher, rigged_inverse_bindposes, to_bevy_mesh, to_bevy_meshes, to_bevy_rigged_mesh,
};
pub use crate::prims::{to_bevy_prim_mesh, to_bevy_prim_meshes};
#[cfg(feature = "bevy_pbr")]
pub use crate::sky::{SkyMaterial, SkyMaterialPlugin, SkyParams};
#[cfg(feature = "bevy_pbr")]
pub use crate::stars::{StarMaterial, StarMaterialPlugin, StarParams};
#[cfg(feature = "bevy_pbr")]
pub use crate::sun_disc::{SunDiscMaterial, SunDiscMaterialPlugin, SunDiscParams};
#[cfg(feature = "bevy_pbr")]
pub use crate::terrain::{
    ATTRIBUTE_TERRAIN_WEIGHTS, TerrainLighting, TerrainMaterial, TerrainMaterialPlugin,
};
pub use crate::textures::{
    BevyTextureFetcher, planar_texgen_uv, texture_face_uv_transform, texture_uv_transform,
    to_bevy_image,
};
pub use crate::tree::to_bevy_tree_mesh;
#[cfg(feature = "bevy_pbr")]
pub use crate::water::{WaterMaterial, WaterMaterialPlugin, WaterParams};

pub mod animations;
pub mod assets;
mod async_http;
mod async_runtime;
pub mod avatars;
mod caps;
mod chat_log;
#[cfg(feature = "bevy_pbr")]
pub mod clouds;
mod experiences;
mod fetch;
pub mod grass;
mod http;
pub mod http_proxy;
mod inventory;
mod inventory_cache;
mod lsl_syntax_cache;
mod marketplace;
mod materials;
mod media;
pub mod meshes;
pub mod prims;
mod retry;
#[cfg(feature = "bevy_pbr")]
pub mod sky;
#[cfg(feature = "bevy_pbr")]
pub mod stars;
#[cfg(feature = "bevy_pbr")]
pub mod sun_disc;
#[cfg(feature = "bevy_pbr")]
pub mod terrain;
pub mod textures;
pub mod tree;
mod upload;
mod voice;
#[cfg(feature = "bevy_pbr")]
pub mod water;
mod world;

/// Override a material pipeline's **alpha** blend component to keep the destination
/// (`Zero, One`), so an alpha-blended material's coverage does not overwrite the
/// scene alpha channel — which the viewer's glow pass (`glow.rs`) uses as the
/// per-face glow mask. The colour blend (coverage) is left untouched, so the
/// material renders identically; only the target alpha (the glow mask, contributed
/// by the opaque surface behind) is preserved, so a transparent surface (water,
/// clouds, the sun / moon disc, stars, particles, parcel borders) does not bloom.
/// Call from an alpha-blended material's `Material::specialize`.
///
/// Gated behind `bevy_pbr` like the material modules that use it — only the
/// windowed viewer renders.
#[cfg(feature = "bevy_pbr")]
pub fn preserve_glow_mask_alpha(
    descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
) {
    use bevy::render::render_resource::{BlendComponent, BlendFactor, BlendOperation};
    let Some(fragment) = descriptor.fragment.as_mut() else {
        return;
    };
    for target in fragment.targets.iter_mut().flatten() {
        if let Some(blend) = target.blend.as_mut() {
            blend.alpha = BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
        }
    }
}

use crate::caps::{CAPS_FAILURE_PREFIX, post_neighbour_seed, start_caps};
use crate::chat_log::ChatLog;
use crate::experiences::{run_experience_status, run_group_experiences};
use crate::fetch::{run_asset_fetch, run_generic_asset_fetch, run_texture_fetch};
use crate::http::{
    run_avatar_picker_search, run_caps_oneway, run_chat_session_fetch_history,
    run_chat_session_request, run_delete_caps_llsd, run_fetch_lsl_syntax, run_get_caps_llsd,
    run_land_resources, run_patch_caps_llsd, run_put_caps_llsd,
};
use crate::inventory::{
    fetch_folder_contents, run_group_members_fetch, run_inventory_fetch,
    run_server_appearance_update,
};
use crate::inventory_cache::InventoryCache;
use crate::lsl_syntax_cache::LslSyntaxCache;
use crate::marketplace::dispatch_marketplace_request;
use crate::materials::{
    run_modify_material_params, run_render_materials_fetch, run_set_render_materials,
};
use crate::media::{post_caps_llsd_oneway, run_object_media_fetch};
use crate::upload::{
    emit_upload_failure, emit_upload_unavailable, run_caps_upload, run_report_screenshot_upload,
    run_script_upload, spawn_new_file_upload,
};
use crate::voice::{run_voice_cap, run_voice_signaling};
use crate::world::{SlRegionIndex, maintain_world};

pub use crate::world::{
    SlAgentParcel, SlCurrentRegion, SlIdentity, SlNeighbor, SlParcel, SlParcelOverlay, SlRegion,
    SlRegionIdentity, SlRegionLimits,
};

/// How long to wait for a single CAPS event-queue long-poll before retrying.
const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// Per-avatar directory derivation for the disk features, keyed by grid + avatar
/// name with UUID-based rename discovery (see [`sl_account_dirs`]).
///
/// When set on [`SlClientPlugin`], the driver resolves the avatar's directory at
/// login (once the login response yields the agent UUID, before any disk feature
/// is touched) and points the chat-log and inventory-cache directories at
/// `<base>/<grid>/<name>/` under each feature's own accounts root — overriding
/// any explicit [`ClientDirectories::agent_chat_log_dir`] /
/// [`ClientDirectories::agent_cache_dir`]. The two roots are kept separate so
/// each feature honours its XDG category (chat logs under a state root, the
/// regenerable inventory cache under a cache root). This is how the reconcile
/// (and a paid name change) is handled inline at the synchronous login point,
/// rather than the host pre-supplying a fixed per-account path before the UUID is
/// known.
#[derive(Debug, Clone)]
pub struct AccountDirsConfig {
    /// The grid segment (from `sl_account_dirs::grid_dir_name`).
    pub grid: String,
    /// The readable avatar segment (from `sl_account_dirs::avatar_dir_name`).
    pub avatar: String,
    /// The accounts root the per-avatar chat-log directory lives under (e.g. an
    /// XDG state dir's `accounts` subdirectory), or `None` to leave chat logging
    /// to the fixed [`ClientDirectories::agent_chat_log_dir`].
    pub chat_log_base: Option<PathBuf>,
    /// The accounts root the per-avatar inventory-cache directory lives under
    /// (e.g. an XDG cache dir's `accounts` subdirectory), or `None` to leave the
    /// cache to the fixed [`ClientDirectories::agent_cache_dir`].
    pub inventory_cache_base: Option<PathBuf>,
}

/// The Bevy plugin that drives a sans-I/O [`Session`] from ECS systems.
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are independent, orthogonal feature toggles — diagnostics, the inventory \
              crawl, the server chat-backlog fetch, offline mode — that a consumer sets in any \
              combination; each mirrors a bool-shaped Session/Client switch, so an enum per flag \
              would only obscure plainly-named yes/no options"
)]
#[derive(Debug, Clone)]
pub struct SlClientPlugin {
    /// The login parameters used to start the session.
    pub params: LoginParams,
    /// Whether to collect protocol diagnostics. Off by default; while enabled,
    /// the session records [`Diagnostic`]s for anomalies it would otherwise
    /// silently drop, surfaced as [`SlDiagnostic`] events.
    pub diagnostics: bool,
    /// The local chat-log configuration (default off). When any text-chat type is
    /// enabled, the driver writes Firestorm-compatible transcripts and serves the
    /// older, file-backed pages of `QueryChatHistoryPage`.
    pub chat_log_config: ChatLogConfig,
    /// The per-account filesystem directories the driver persists its optional
    /// features under (chat-log transcripts, the inventory disk-cache). Default
    /// all-`None`, disabling every disk feature; a `None` field disables that
    /// feature.
    pub directories: ClientDirectories,
    /// Optional per-avatar directory derivation for the disk features. When set,
    /// the chat-log and inventory-cache directories are resolved to the avatar's
    /// `<accounts_base>/<grid>/<name>/` directory at login (with rename
    /// discovery), overriding the corresponding fixed
    /// [`directories`](Self::directories) fields. Default `None` (use the fixed
    /// directories as-is).
    pub account_dirs: Option<AccountDirsConfig>,
    /// The inventory disk-cache configuration (default off). Once enabled (and
    /// paired with [`ClientDirectories::agent_cache_dir`]), the driver loads the
    /// per-account `<agent-uuid>.inv.llsd.gz` cache at login, reconciles it
    /// against the skeleton so version-matching folders skip the background
    /// refetch, and writes it back on logout and on a dirty/idle tick.
    pub inventory_cache_config: InventoryCacheConfig,
    /// Whether to run the automatic background inventory crawl (off by default).
    /// While enabled, the driver breadth-first fetches the agent's inventory tree
    /// in the background (a bounded number of folder-contents requests in flight).
    /// While disabled, no folder fetches are issued unless the driver asks for one
    /// (`RequestFolderContents` / `FetchInventoryFolders`), so a consumer that
    /// ignores inventory pays nothing.
    pub background_inventory_fetch: bool,
    /// Whether to auto-fetch a joined group / conference session's server-side
    /// chat backlog (`ChatSessionRequest` `fetch history`; **on** matches the
    /// reference viewer's `FetchGroupChatHistory` default). While enabled, the
    /// driver POSTs one fetch per session that reaches joined — when the grid
    /// serves the capability; stock OpenSim does not, so nothing is ever sent
    /// there — and the backlog surfaces as
    /// [`Event::SessionServerHistory`](sl_proto::Event::SessionServerHistory).
    /// The explicit [`Command::FetchSessionHistory`] works regardless.
    pub fetch_server_chat_history: bool,
    /// Run the plugin **offline**: register the same event/resource substrate
    /// ([`SlEvent`], [`SlCommand`], [`SlIdentity`], …) but never perform the
    /// XML-RPC login or open a circuit, so nothing touches the network. The
    /// driver goes straight to its finished state and the session is fed entirely
    /// by whatever writes synthetic [`SlEvent`]s instead (the viewer's
    /// avatar-state **replay** mode). Default `false` (the normal live login).
    pub offline: bool,
}

impl Plugin for SlClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SlEvent>()
            .add_message::<SlDiagnostic>()
            .add_message::<SlCapabilities>()
            .add_message::<SlMfaChallenge>()
            .add_message::<SlLoginRejected>()
            .add_message::<SlCommand>()
            .insert_resource(SlConfig {
                params: self.params.clone(),
                diagnostics: self.diagnostics,
                chat_log_config: self.chat_log_config.clone(),
                directories: self.directories.clone(),
                account_dirs: self.account_dirs.clone(),
                inventory_cache_config: self.inventory_cache_config,
                background_inventory_fetch: self.background_inventory_fetch,
                fetch_server_chat_history: self.fetch_server_chat_history,
                offline: self.offline,
            })
            .init_resource::<SlIdentity>()
            .init_resource::<SlAgentParcel>()
            .init_resource::<SlParcelOverlay>()
            .init_resource::<SlRegionIndex>()
            .add_systems(Startup, start_login)
            // `maintain_world` reads the events `drive` writes, so chain it after.
            .add_systems(Update, (drive, maintain_world).chain());
    }
}

/// A high-level session event, emitted as a Bevy event.
#[derive(Message, Debug, Clone)]
pub struct SlEvent(pub SessionEvent);

/// A protocol diagnostic, emitted as a Bevy event. Surfaces anomalies the
/// session would otherwise silently drop (decode failures, unhandled messages,
/// unknown CAPS events, missing expected replies). Only produced when
/// diagnostics are enabled via [`SlClientPlugin::diagnostics`]; kept strictly
/// separate from [`SlEvent`].
#[derive(Message, Debug, Clone)]
pub struct SlDiagnostic(pub Diagnostic);

/// The region's capability map (cap name → URL), emitted as a Bevy event each
/// time the driver discovers it: once after the seed capability is fetched at
/// startup and again after every region change. Useful for resolving or
/// symbolizing `$cap:Name` placeholders in a REPL or diagnostic consumer.
#[derive(Message, Debug, Clone)]
pub struct SlCapabilities(pub HashMap<String, String>);

/// Emitted when the grid requires a multi-factor one-time code. To answer it,
/// re-add the plugin with login parameters prepared via
/// `LoginRequest::with_mfa`.
#[derive(Message, Debug, Clone)]
pub struct SlMfaChallenge(pub MfaChallenge);

/// Emitted when the grid rejected the login with a *retryable* "already logged
/// in" presence ([`LoginRejectKind::AlreadyLoggedIn`]) — typically a prior
/// session that did not log out cleanly, which the grid evicts on the next
/// attempt. Unlike a fatal rejection (which arrives as a
/// [`DisconnectReason::LoginFailed`]), this is surfaced as its own event,
/// mirroring [`SlMfaChallenge`], so a driver can consult the user and re-add the
/// plugin to retry the same login. The carried [`LoginFailure`] keeps the raw
/// reason/message for display.
#[derive(Message, Debug, Clone)]
pub struct SlLoginRejected(pub LoginFailure);

/// A command to a running session, sent as a Bevy event. Wraps the shared
/// [`Command`] vocabulary (defined in `sl-proto`) so it can be read as a Bevy
/// event; match on its `.0` to dispatch.
#[derive(Message, Debug)]
pub struct SlCommand(pub Command);

/// The plugin configuration resource.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the resource mirror of `SlClientPlugin`'s independent feature toggles (see its \
              matching expect)"
)]
#[derive(Resource, Debug)]
struct SlConfig {
    /// The login parameters.
    params: LoginParams,
    /// Whether to collect protocol diagnostics.
    diagnostics: bool,
    /// The local chat-log configuration (default off).
    chat_log_config: ChatLogConfig,
    /// The per-account filesystem directories the optional disk features use.
    directories: ClientDirectories,
    /// Optional per-avatar directory derivation, resolved at login.
    account_dirs: Option<AccountDirsConfig>,
    /// The inventory disk-cache configuration (default off).
    inventory_cache_config: InventoryCacheConfig,
    /// Whether the automatic background inventory crawl is enabled (default off).
    background_inventory_fetch: bool,
    /// Whether the automatic server-side chat-backlog fetch is enabled (the
    /// convention is on — see [`SlClientPlugin::fetch_server_chat_history`]).
    fetch_server_chat_history: bool,
    /// Whether to run offline (skip login; feed the session synthetic events).
    offline: bool,
}

/// The driver's runtime state resource: the channel pair to the session's
/// dedicated network thread, or `None` in offline (replay) mode, where
/// synthetic [`SlEvent`]s are injected instead of a live session.
#[derive(Resource)]
struct SlState {
    /// The live network-thread link, absent offline.
    link: Option<NetLink>,
}

/// The Bevy side of the network thread: [`drive`] forwards each frame's
/// [`SlCommand`]s into `command_tx` and drains `outbound_rx` into the Bevy
/// messages / resources. Dropping this (app teardown) closes the command
/// channel, which the thread notices within one tick and exits on.
struct NetLink {
    /// Commands to the network thread.
    command_tx: Sender<Command>,
    /// Everything the network thread reports back.
    outbound_rx: Receiver<NetOutbound>,
}

/// One message from the network thread to the Bevy side, drained by
/// [`drive`] into the corresponding message writer or resource.
enum NetOutbound {
    /// A session event, surfaced as [`SlEvent`].
    Event(SessionEvent),
    /// A protocol diagnostic, surfaced as [`SlDiagnostic`].
    Diagnostic(Diagnostic),
    /// A freshly discovered capability map, surfaced as [`SlCapabilities`].
    Capabilities(HashMap<String, String>),
    /// The login-derived identity, mirrored into the [`SlIdentity`] resource.
    Identity(Box<SlIdentity>),
    /// The agent's parcel / fly / seat mirror, sent whenever it changes and
    /// mirrored into the [`SlAgentParcel`] resource.
    AgentParcel(Box<SlAgentParcel>),
    /// The grid requires a multi-factor one-time code ([`SlMfaChallenge`]).
    Mfa(MfaChallenge),
    /// A retryable "already logged in" rejection ([`SlLoginRejected`]).
    Rejected(LoginFailure),
}

/// The configuration the network thread carries away from [`SlConfig`] (the
/// resource itself stays on the Bevy side).
struct NetThreadConfig {
    /// The local chat-log configuration (default off).
    chat_log_config: ChatLogConfig,
    /// The per-account filesystem directories the optional disk features use.
    directories: ClientDirectories,
    /// Optional per-avatar directory derivation, resolved at login.
    account_dirs: Option<AccountDirsConfig>,
    /// The inventory disk-cache configuration (default off).
    inventory_cache_config: InventoryCacheConfig,
}

/// The running session's owned state, stepped once per network-thread tick by
/// [`advance_running`].
struct RunningSession {
    /// The driven session.
    session: Box<Session>,
    /// The UDP socket, in blocking mode with a [`NET_TICK`] read timeout —
    /// the thread sleeps *in* `recv_from`, so an inbound datagram is parsed
    /// (and ACKed) the moment it arrives instead of on the next render frame.
    socket: UdpSocket,
    /// A reusable receive buffer.
    recv_buf: Vec<u8>,
    /// The CAPS subsystem for the current region, if a seed capability is
    /// known. Re-targeted on each region change.
    caps: Option<Caps>,
    /// The local chat-log writer/reader (a no-op when disabled).
    chat_log: Box<ChatLog>,
    /// The inventory disk-cache reader/writer (a no-op when disabled).
    inventory_cache: Box<InventoryCache>,
    /// The `LSLSyntax` fetch/cache state.
    lsl_syntax: Box<LslSyntaxState>,
    /// The last agent parcel / fly / seat mirror sent to the app, so only a
    /// change crosses the channel.
    agent_parcel: SlAgentParcel,
}

/// The `LSLSyntax` state carried across ticks: the by-id disk cache plus the
/// last syntax id resolved, so an unchanged id costs nothing and a change
/// triggers exactly one fetch. Persists across region changes (the language
/// definition rarely differs between them). Boxed into [`RunningSession`].
struct LslSyntaxState {
    /// The `LSLSyntax` document disk-cache, keyed by syntax id (a no-op when
    /// disabled).
    cache: LslSyntaxCache,
    /// The last `LSLSyntaxId` fetched or loaded, or `None` before the first.
    last_id: Option<Uuid>,
}

/// The CAPS subsystem for one region: a background thread fetches the capability
/// map (reported over `map_rx`) then long-polls `EventQueueGet`, forwarding each
/// decoded event over `events_rx`. One-shot CAPS fetches (inventory) run on their
/// own threads and report back over the same `events_tx`. Dropping it signals the
/// poller thread to stop after its in-flight request returns.
pub(crate) struct Caps {
    /// Receives decoded event-queue events and CAPS responses (e.g. inventory).
    pub(crate) events_rx: Receiver<(String, Llsd)>,
    /// A sender clone for spawning one-shot CAPS fetches.
    pub(crate) events_tx: Sender<(String, Llsd)>,
    /// Receives fully-formed session events from one-shot binary asset fetches
    /// (the HTTP texture/mesh/asset caps, which return raw bytes rather than
    /// LLSD), to be surfaced directly as [`SlEvent`]s.
    pub(crate) asset_rx: Receiver<SessionEvent>,
    /// A sender clone for spawning one-shot binary asset fetches.
    pub(crate) asset_tx: Sender<SessionEvent>,
    /// Receives the region's capability map once the poller has fetched it, or a
    /// readable error if the seed-capabilities fetch failed (which fails the login
    /// while still awaiting initial caps, and merely degrades a region change).
    pub(crate) map_rx: Receiver<Result<HashMap<String, String>, String>>,
    /// The cached capability map (cap name → URL), empty until discovered.
    pub(crate) map: HashMap<String, String>,
    /// Commands the single long-lived event-queue worker thread — a
    /// [`EqCommand::Switch`](crate::caps::EqCommand) on every region change
    /// re-targets it at the new root's seed instead of spawning a second poller.
    /// Dropping the [`Caps`] closes this channel, so the worker exits after its
    /// current poll.
    pub(crate) command_tx: crossbeam_channel::Sender<crate::caps::EqCommand>,
}

/// How long one network-thread tick blocks in `recv_from` waiting for a
/// datagram before it services timers, CAPS payloads, and queued commands
/// anyway. Small enough that outbound commands and retransmits never wait
/// noticeably; large enough that an idle session barely spins.
const NET_TICK: Duration = Duration::from_millis(15);

/// Startup system: builds the session and spawns its dedicated network
/// thread, which performs the blocking XML-RPC login and then pumps the
/// socket / CAPS / timers / commands continuously — LLUDP parse, ACKs and
/// retransmits no longer wait for a render frame, and the chat-log /
/// inventory-cache disk writes stay off the frame thread.
fn start_login(mut commands: Commands, config: Res<SlConfig>) {
    // Offline (replay) mode: register nothing on the wire — no thread, so
    // `drive` is a no-op and the session is fed only by synthetic `SlEvent`s
    // injected by the replay loader.
    if config.offline {
        commands.insert_resource(SlState { link: None });
        return;
    }
    let mut session = Session::new(config.params.clone());
    session.set_diagnostics(config.diagnostics);
    session.set_background_inventory_fetch(config.background_inventory_fetch);
    session.set_fetch_server_chat_history(config.fetch_server_chat_history);
    let (command_tx, command_rx) = unbounded();
    let (outbound_tx, outbound_rx) = unbounded();
    let net_config = NetThreadConfig {
        chat_log_config: config.chat_log_config.clone(),
        directories: config.directories.clone(),
        account_dirs: config.account_dirs.clone(),
        inventory_cache_config: config.inventory_cache_config,
    };
    let spawned = std::thread::Builder::new()
        .name("sl-session-net".to_owned())
        .spawn(move || {
            run_network_thread(Box::new(session), &net_config, &command_rx, &outbound_tx);
        });
    if let Err(error) = spawned {
        tracing::error!("could not spawn the session network thread: {error}");
        commands.insert_resource(SlState { link: None });
        return;
    }
    commands.insert_resource(SlState {
        link: Some(NetLink {
            command_tx,
            outbound_rx,
        }),
    });
}

/// The dedicated session network thread: the blocking XML-RPC login, then the
/// running pump until the session ends or the command channel closes (app
/// teardown).
fn run_network_thread(
    session: Box<Session>,
    config: &NetThreadConfig,
    commands: &Receiver<Command>,
    outbound: &Sender<NetOutbound>,
) {
    let Some(mut running) = login_phase(session, config, outbound) else {
        return;
    };
    // Sleep *inside* `recv_from`: a datagram wakes the tick immediately, and
    // an idle tick still services timers / CAPS / commands every NET_TICK.
    running.socket.set_read_timeout(Some(NET_TICK)).ok();
    loop {
        // Drain this tick's commands; a disconnected channel means the app is
        // shutting down (the `SlState` resource dropped), so stop pumping.
        let mut pending: Vec<Command> = Vec::new();
        loop {
            match commands.try_recv() {
                Ok(command) => pending.push(command),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match advance_running(running, pending, Instant::now(), outbound) {
            Some(next) => running = next,
            None => return,
        }
    }
}

/// Performs the blocking XML-RPC login POST, returning the response body.
fn perform_login(url: &str, user_agent: &str, body: String) -> Result<String, String> {
    crate::http_proxy::blocking_client_builder()
        .build()
        .map_err(|error| error.to_string())?
        .post(url)
        .header("Content-Type", "text/xml")
        .header("User-Agent", user_agent)
        .body(body)
        .send()
        .and_then(reqwest::blocking::Response::text)
        .map_err(|error| error.to_string())
}

/// Update system: the thin pump between the ECS and the session's network
/// thread — forwards this frame's [`SlCommand`]s (cloned, so other in-process
/// observers still read the originals) and drains everything the thread
/// reported since last frame into the Bevy messages / resources. All protocol
/// work (LLUDP parse, CAPS ingestion, timers, disk caches) happens on the
/// thread; see [`run_network_thread`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and the message \
              writers the network thread's reports fan out into"
)]
fn drive(
    state: Res<SlState>,
    mut events: MessageWriter<SlEvent>,
    mut diagnostics: MessageWriter<SlDiagnostic>,
    mut capabilities: MessageWriter<SlCapabilities>,
    mut identity: ResMut<SlIdentity>,
    mut agent_parcel: ResMut<SlAgentParcel>,
    mut mfa: MessageWriter<SlMfaChallenge>,
    mut rejected: MessageWriter<SlLoginRejected>,
    mut commands: MessageReader<SlCommand>,
    mut session_ended: Local<bool>,
) {
    let Some(link) = &state.link else {
        return;
    };
    for command in commands.read() {
        // A send failure means the thread is gone; the drain below surfaces it.
        if link.command_tx.send(command.0.clone()).is_err() {
            break;
        }
    }
    loop {
        match link.outbound_rx.try_recv() {
            Ok(NetOutbound::Event(event)) => {
                if matches!(
                    event,
                    SessionEvent::Disconnected(_) | SessionEvent::LoggedOut
                ) {
                    *session_ended = true;
                }
                events.write(SlEvent(event));
            }
            Ok(NetOutbound::Diagnostic(diagnostic)) => {
                diagnostics.write(SlDiagnostic(diagnostic));
            }
            Ok(NetOutbound::Capabilities(map)) => {
                capabilities.write(SlCapabilities(map));
            }
            Ok(NetOutbound::Identity(new_identity)) => *identity = *new_identity,
            Ok(NetOutbound::AgentParcel(parcel)) => *agent_parcel = *parcel,
            Ok(NetOutbound::Mfa(challenge)) => {
                mfa.write(SlMfaChallenge(challenge));
            }
            Ok(NetOutbound::Rejected(failure)) => {
                rejected.write(SlLoginRejected(failure));
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                // The thread ended. After a clean LoggedOut / Disconnected
                // this is expected teardown; anything else (a panic) must
                // still surface as a disconnect rather than a silent hang.
                if !*session_ended {
                    *session_ended = true;
                    events.write(SlEvent(SessionEvent::Disconnected(
                        DisconnectReason::ProtocolError,
                    )));
                }
                break;
            }
        }
    }
}

/// Send a synthetic [`SessionEvent::Disconnected`] to the app side.
fn send_disconnect(outbound: &Sender<NetOutbound>, reason: DisconnectReason) {
    outbound
        .send(NetOutbound::Event(SessionEvent::Disconnected(reason)))
        .ok();
}

/// The maximum number of login redirects (`login = "indeterminate"`) the
/// login phase follows before giving up, protecting against a grid that
/// redirects in a loop.
const MAX_LOGIN_REDIRECTS: u32 = 5;

/// Performs the blocking XML-RPC login — following bounded redirects — and
/// handles the response (on the network thread), building the
/// [`RunningSession`] on success; `None` ends the thread (a failure, an MFA
/// challenge, or a retryable rejection — each surfaced over `outbound`).
fn login_phase(
    mut session: Box<Session>,
    config: &NetThreadConfig,
    outbound: &Sender<NetOutbound>,
) -> Option<RunningSession> {
    let mut redirects: u32 = 0;
    // A redirect response re-arms the session's pending login request at its
    // `next_url` (see `Session::handle_login_response`), so the loop simply
    // performs the request again until a terminal response arrives.
    let response = loop {
        let request = session.login_http_request()?;
        let body = perform_login(request.url.as_str(), &request.user_agent, request.body);
        let Ok(body) = body else {
            send_disconnect(outbound, DisconnectReason::ProtocolError);
            return None;
        };
        match parse_login_response(&body) {
            Ok(LoginResponse::Redirect(redirect)) => {
                if redirects >= MAX_LOGIN_REDIRECTS {
                    send_disconnect(
                        outbound,
                        DisconnectReason::LoginFailed {
                            reason: "indeterminate".to_owned(),
                            message: format!(
                                "login redirected more than {MAX_LOGIN_REDIRECTS} times \
                                 (next: {})",
                                redirect.next_url
                            ),
                        },
                    );
                    return None;
                }
                redirects = redirects.saturating_add(1);
                tracing::info!(
                    "login redirected to {} (hop {redirects})",
                    redirect.next_url
                );
                if session
                    .handle_login_response(LoginResponse::Redirect(redirect), Instant::now())
                    .is_err()
                {
                    send_disconnect(outbound, DisconnectReason::ProtocolError);
                    return None;
                }
            }
            other => break other,
        }
    };
    let now = Instant::now();
    match response {
        Ok(LoginResponse::Success(success)) => {
            if session
                .handle_login_response(LoginResponse::Success(success), now)
                .is_err()
            {
                send_disconnect(outbound, DisconnectReason::ProtocolError);
                return None;
            }
            match bind_socket() {
                Ok(socket) => {
                    outbound
                        .send(NetOutbound::Identity(Box::new(SlIdentity {
                            agent_id: session.agent_id(),
                            session_id: session.session_id(),
                            circuit_code: session.circuit_code(),
                            seed_capability: session.seed_capability().cloned(),
                            agent_appearance_service: session.agent_appearance_service().cloned(),
                            map_server_url: session.map_server_url().cloned(),
                            openid_url: session.openid_url().cloned(),
                            openid_token: session.openid_token().map(str::to_owned),
                            region_handle: session.region_handle(),
                            circuit_id: session.root_circuit_id(),
                        })))
                        .ok();
                    let caps = start_caps(&session);
                    // Resolve the per-avatar directory now that the login
                    // response has yielded the agent UUID — inline, before
                    // any disk feature is built, so nothing races.
                    let effective_directories = resolve_account_directories(
                        &config.directories,
                        config.account_dirs.as_ref(),
                        session.agent_id(),
                    );
                    let chat_log = Box::new(ChatLog::new(
                        config.chat_log_config.clone(),
                        effective_directories.agent_chat_log_dir.clone(),
                        session.agent_legacy_name(),
                        session.agent_id(),
                    ));
                    let inventory_cache = Box::new(InventoryCache::new(
                        config.inventory_cache_config,
                        effective_directories.agent_cache_dir.clone(),
                        session.agent_id(),
                        now,
                    ));
                    let lsl_syntax = Box::new(LslSyntaxState {
                        cache: LslSyntaxCache::new(effective_directories.shared_cache_dir.clone()),
                        last_id: None,
                    });
                    Some(RunningSession {
                        session,
                        socket,
                        recv_buf: vec![0u8; RECV_BUFFER_SIZE],
                        caps,
                        chat_log,
                        inventory_cache,
                        lsl_syntax,
                        agent_parcel: SlAgentParcel::default(),
                    })
                }
                Err(()) => {
                    send_disconnect(outbound, DisconnectReason::ProtocolError);
                    None
                }
            }
        }
        Ok(LoginResponse::MfaChallenge(challenge)) => {
            outbound.send(NetOutbound::Mfa(challenge)).ok();
            None
        }
        Ok(LoginResponse::Failure(failure)) => {
            // A retryable "already logged in" rejection is surfaced like an
            // MFA challenge — its own event the driver can act on (consult
            // the user, re-add the plugin) — rather than a fatal disconnect.
            if failure.kind() == LoginRejectKind::AlreadyLoggedIn {
                outbound.send(NetOutbound::Rejected(failure)).ok();
            } else {
                send_disconnect(
                    outbound,
                    DisconnectReason::LoginFailed {
                        reason: failure.reason,
                        message: failure.message,
                    },
                );
            }
            None
        }
        // The redirect loop above consumes every redirect before breaking, so
        // one cannot reach this match; handle it defensively all the same.
        Ok(LoginResponse::Redirect(_redirect)) => {
            send_disconnect(outbound, DisconnectReason::ProtocolError);
            None
        }
        Err(_parse) => {
            send_disconnect(outbound, DisconnectReason::ProtocolError);
            None
        }
    }
}

/// Resolve the effective per-account directories at login.
///
/// With no [`AccountDirsConfig`] (or no agent id), the fixed `base` directories
/// are used unchanged. Otherwise the avatar's `<accounts_base>/<grid>/<name>/`
/// directory is reconciled (creating it, or renaming it if the agent UUID shows
/// a name change) and the chat-log / inventory-cache directories are pointed at
/// its `chat` / `inventorycache` subdirectories. A reconcile failure logs and
/// falls back to `base`, so a disk-permission problem never blocks login.
fn resolve_account_directories(
    base: &ClientDirectories,
    account_dirs: Option<&AccountDirsConfig>,
    agent_id: Option<AgentKey>,
) -> ClientDirectories {
    let (Some(account), Some(agent)) = (account_dirs, agent_id) else {
        return base.clone();
    };
    let uuid = agent.uuid();
    // Reconcile one feature's accounts root to this avatar's directory, falling
    // back to the root as-is on a filesystem error so the feature still works
    // (un-keyed) rather than being silently disabled.
    let resolve = |accounts_base: &Option<PathBuf>| -> Option<PathBuf> {
        let accounts_base = accounts_base.as_ref()?;
        match sl_account_dirs::reconcile_account_dir(
            accounts_base,
            &account.grid,
            &account.avatar,
            uuid,
        ) {
            Ok(dir) => Some(dir),
            Err(error) => {
                tracing::warn!(
                    "could not resolve account directory under {}: {error}",
                    accounts_base.display()
                );
                Some(accounts_base.clone())
            }
        }
    };
    ClientDirectories {
        agent_chat_log_dir: resolve(&account.chat_log_base),
        agent_cache_dir: resolve(&account.inventory_cache_base),
        shared_cache_dir: base.shared_cache_dir.clone(),
    }
}

/// Binds a non-blocking UDP socket on an ephemeral port.
fn bind_socket() -> Result<UdpSocket, ()> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_ignored| ())?;
    socket.set_nonblocking(true).map_err(|_ignored| ())?;
    Ok(socket)
}

/// The HTTP verb for an AIS3 inventory request ([`route_ais3`]).
enum Ais3Verb {
    /// `POST` — create (a folder, a link).
    Post,
    /// `PATCH` — mutate an existing folder / item (rename, move).
    Patch,
    /// `DELETE` — remove a folder / item / a folder's children.
    Delete,
}

/// Whether the region offers the modern AIS3 inventory API (`InventoryAPIv3`) —
/// Second Life does, stock OpenSim does not. Inventory mutations route through
/// AIS3 when it is present and fall back to the legacy UDP messages otherwise, so
/// this gates the branch (mirroring the reference viewer's `AISAPI::isAvailable`).
fn has_ais3(caps: Option<&Caps>) -> bool {
    caps.is_some_and(|caps| caps.map.contains_key(CAP_INVENTORY_API_V3))
}

/// Route an inventory mutation through AIS3 (Second Life): spawn its HTTP request
/// on a background thread, feeding the reply back through the caps event channel
/// (parsed as an inventory update). Returns `true` when it dispatched, `false` when
/// the `InventoryAPIv3` capability is absent (OpenSim) and the caller must fall
/// back to the legacy UDP path. `suffix` is the AIS3 URL suffix under the cap base;
/// `body` is the LLSD request body (ignored for [`Ais3Verb::Delete`]).
fn route_ais3(caps: Option<&Caps>, suffix: &str, verb: Ais3Verb, body: Option<String>) -> bool {
    let Some(caps) = caps else {
        return false;
    };
    let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned() else {
        return false;
    };
    let events_tx = caps.events_tx.clone();
    let url = format!("{base}{suffix}");
    let _handle = std::thread::spawn(move || match verb {
        Ais3Verb::Post => {
            run_voice_cap(
                &url,
                body.unwrap_or_default(),
                CAP_INVENTORY_API_V3,
                &events_tx,
            );
        }
        Ais3Verb::Patch => {
            run_patch_caps_llsd(
                &url,
                body.unwrap_or_default(),
                CAP_INVENTORY_API_V3,
                &events_tx,
            );
        }
        Ais3Verb::Delete => run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx),
    });
    true
}

/// One tick of the running session (on the network thread): receive UDP and
/// CAPS events, apply the tick's queued commands, time out, transmit, and
/// report events / diagnostics / the agent-parcel mirror over `outbound`.
/// Returns the state for the next tick, or `None` once the session finished
/// (persisting the inventory cache on the way out).
fn advance_running(
    state: RunningSession,
    commands: Vec<Command>,
    now: Instant,
    outbound: &Sender<NetOutbound>,
) -> Option<RunningSession> {
    let RunningSession {
        mut session,
        socket,
        mut recv_buf,
        mut caps,
        mut chat_log,
        mut inventory_cache,
        mut lsl_syntax,
        mut agent_parcel,
    } = state;
    // Wait for inbound data with ONE blocking receive (its [`NET_TICK`] read
    // timeout is the thread's tick cadence — a datagram wakes the tick
    // immediately, an idle tick still runs timers / commands), then drain the
    // rest of the backlog non-blocking so the ACK flush below is never
    // delayed by a second timeout wait.
    match socket.recv_from(&mut recv_buf) {
        Ok((len, from)) => {
            if let Some(datagram) = recv_buf.get(..len) {
                session.handle_datagram(from, datagram, now).ok();
            }
            socket.set_nonblocking(true).ok();
            while let Ok((more_len, more_from)) = socket.recv_from(&mut recv_buf) {
                if let Some(datagram) = recv_buf.get(..more_len) {
                    session.handle_datagram(more_from, datagram, now).ok();
                }
            }
            socket.set_nonblocking(false).ok();
            socket.set_read_timeout(Some(NET_TICK)).ok();
        }
        Err(_timeout_or_other) => {}
    }

    // Cache the capability map once the poller discovers it, then drain any CAPS
    // payloads (event-queue events plus inventory responses).
    if let Some(caps) = caps.as_mut() {
        while let Ok(result) = caps.map_rx.try_recv() {
            let map = match result {
                Ok(map) => map,
                Err(reason) => {
                    // The seed-capabilities fetch failed. On the initial login,
                    // abort rather than proceed into a capless session (the deferred
                    // `CompleteAgentMovement` is then never sent); on a later region
                    // change the session is already established, so only degrade.
                    if session.is_awaiting_initial_capabilities() {
                        session.fail_no_capabilities(reason);
                    } else {
                        tracing::warn!("region-change capability fetch failed: {reason}");
                    }
                    continue;
                }
            };
            // The region served its capabilities: release the deferred initial-login
            // `CompleteAgentMovement` now that the simulator has processed our
            // seed-caps request (which advertised animesh support) — so it knows we
            // render animesh before it streams the scene. A no-op on a region change.
            session.notify_capabilities_ready(now).ok();
            // The viewer fetches `SimulatorFeatures` on arriving in a region, so
            // GET it once the capability map is known (at login and on each region
            // change), surfacing the flags as `Event::SimulatorFeatures`.
            if let Some(url) = map.get(CAP_SIMULATOR_FEATURES).cloned() {
                let events_tx = caps.events_tx.clone();
                std::thread::spawn(move || {
                    run_get_caps_llsd(&url, CAP_SIMULATOR_FEATURES, &events_tx);
                });
            }
            outbound.send(NetOutbound::Capabilities(map.clone())).ok();
            caps.map = map;
        }
        while let Ok((message, body)) = caps.events_rx.try_recv() {
            // A CAPS helper reports a failed request by sending the failure
            // sentinel rather than a decoded reply; surface it as a diagnostic
            // instead of feeding the session.
            if let Some(cap) = message.strip_prefix(CAPS_FAILURE_PREFIX) {
                tracing::warn!(capability = cap, "CAPS request failed; no reply surfaced");
                if session.diagnostics_enabled() {
                    outbound
                        .send(NetOutbound::Diagnostic(Diagnostic::ExpectedReplyMissing {
                            request: cap.to_owned(),
                            sequence: None,
                        }))
                        .ok();
                }
            } else {
                session.handle_caps_event(&message, &body, now).ok();
            }
        }
        // Binary asset fetches return fully-formed session events; surface them.
        while let Ok(event) = caps.asset_rx.try_recv() {
            outbound.send(NetOutbound::Event(event)).ok();
        }

        // Background inventory crawl: when enabled, sweep the next bounded batch
        // of unfetched folders and POST a `FetchInventoryDescendents2` for each.
        // Self-gating — `next_inventory_fetch_batch` returns empty when the crawl
        // is off. Only swept while the fetch capability and agent id are known, so
        // folders are never flipped to `Fetching` for a request that cannot be
        // issued. The replies fold in over `events_rx` and the next frame
        // continues the sweep a level deeper.
        if let (Some(url), Some(owner)) = (
            caps.map.get(CAP_FETCH_INVENTORY).cloned(),
            session.agent_id(),
        ) {
            let batch = session.next_inventory_fetch_batch(INVENTORY_FETCH_MAX_IN_FLIGHT);
            // The batch can span both trees: the agent folders go to
            // `FetchInventoryDescendents2` with the agent owner, the Library folders
            // to `FetchLibDescendents2` with the Library owner (or, where the grid
            // does not serve that cap — e.g. OpenSim — over the UDP path instead, so
            // they never stay stuck `Fetching`).
            let (library_folders, agent_folders): (Vec<_>, Vec<_>) =
                batch.into_iter().partition(|folder| {
                    session.inventory_owner(*folder) == Some(InventoryOwner::Library)
                });
            if !agent_folders.is_empty() {
                let events_tx = caps.events_tx.clone();
                std::thread::spawn(move || {
                    run_inventory_fetch(
                        &url,
                        owner.uuid(),
                        &agent_folders,
                        CAP_FETCH_INVENTORY,
                        &events_tx,
                    );
                });
            }
            if !library_folders.is_empty() {
                match (
                    caps.map.get(CAP_FETCH_LIBRARY).cloned(),
                    session.library_owner(),
                ) {
                    (Some(lib_url), Some(lib_owner)) => {
                        let events_tx = caps.events_tx.clone();
                        std::thread::spawn(move || {
                            run_inventory_fetch(
                                &lib_url,
                                lib_owner.uuid(),
                                &library_folders,
                                CAP_FETCH_LIBRARY,
                                &events_tx,
                            );
                        });
                    }
                    _ => {
                        for folder in library_folders {
                            session.request_folder_contents(folder, now).ok();
                        }
                    }
                }
            }
        }

        // Server-side chat backlog: when a group / conference session has
        // reached joined and the `ChatSessionRequest` capability is known, POST
        // one `fetch history` per newly joined session. Self-gating and
        // once-per-session — `next_server_history_fetches` flips each returned
        // session to requested and returns empty while the auto-fetch is
        // disabled. On a grid without the capability (stock OpenSim) the gate
        // never opens, so the fetch silently never fires. Mirrors the tokio run
        // loop's sweep.
        if let (Some(url), Some(own_agent)) = (
            caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned(),
            session.agent_id(),
        ) {
            for kind in session.next_server_history_fetches() {
                let session_id = kind.canonical_session_id(own_agent);
                let body = chat_session_request_body(CHAT_SESSION_FETCH_HISTORY, session_id);
                let from_group = matches!(kind, ChatSessionKind::Group { .. });
                let events_tx = caps.events_tx.clone();
                let fetch_url = url.clone();
                std::thread::spawn(move || {
                    run_chat_session_fetch_history(
                        &fetch_url, body, session_id, from_group, &events_tx,
                    );
                });
            }
        }
    }

    // Apply queued commands.
    for command in &commands {
        match command {
            Command::Send {
                message,
                reliability,
            } => {
                session.enqueue((**message).clone(), *reliability, now).ok();
            }
            Command::Chat {
                message,
                chat_type,
                channel,
            } => {
                session.say(message, *chat_type, *channel, now).ok();
            }
            Command::Typing(typing) => {
                session.set_typing(*typing, now).ok();
            }
            Command::InstantMessage {
                to_agent_id,
                message,
            } => {
                session
                    .send_instant_message(*to_agent_id, message, now)
                    .ok();
                chat_log.log_outbound_im(*to_agent_id, message);
            }
            Command::AutoResponse {
                to_agent_id,
                message,
            } => {
                session.send_auto_response(*to_agent_id, message, now).ok();
            }
            Command::ImTyping {
                to_agent_id,
                typing,
            } => {
                session.send_im_typing(*to_agent_id, *typing, now).ok();
            }
            Command::SetControls(controls) => {
                session.set_controls(*controls, now).ok();
            }
            Command::SetThrottle(throttle) => {
                session.set_throttle(*throttle, now).ok();
            }
            Command::SetRotation { body, head } => {
                session.set_rotation(body.clone(), head.clone(), now).ok();
            }
            Command::SetCamera(camera) => {
                session.set_camera(camera.clone(), now).ok();
            }
            Command::Stand => {
                session.stand(now).ok();
            }
            Command::SitOnGround => {
                session.sit_on_ground(now).ok();
            }
            Command::Sit { target, offset } => {
                session.sit_on(*target, offset.clone(), now).ok();
            }
            Command::Autopilot {
                global_x,
                global_y,
                z,
            } => {
                session.autopilot_to(*global_x, *global_y, *z, now).ok();
            }
            Command::RequestAvatarProperties(target) => {
                session.request_avatar_properties(*target, now).ok();
            }
            Command::RequestAvatarPicks(target) => {
                session.request_avatar_picks(*target, now).ok();
            }
            Command::RequestAvatarNotes(target) => {
                session.request_avatar_notes(*target, now).ok();
            }
            Command::RequestAvatarClassifieds(target) => {
                session.request_avatar_classifieds(*target, now).ok();
            }
            Command::RequestPickInfo {
                creator_id,
                pick_id,
            } => {
                session.request_pick_info(*creator_id, *pick_id, now).ok();
            }
            Command::RequestClassifiedInfo(classified_id) => {
                session.request_classified_info(*classified_id, now).ok();
            }
            Command::UpdateProfile(update) => {
                session.update_profile(update, now).ok();
            }
            Command::UpdateInterests(update) => {
                session.update_interests(update, now).ok();
            }
            Command::UpdateAvatarNotes { target_id, notes } => {
                session.update_avatar_notes(*target_id, notes, now).ok();
            }
            Command::UpdatePick(update) => {
                session.update_pick(update, now).ok();
            }
            Command::DeletePick(pick_id) => {
                session.delete_pick(*pick_id, now).ok();
            }
            Command::GodDeletePick { pick_id, query_id } => {
                session.god_delete_pick(*pick_id, *query_id, now).ok();
            }
            Command::UpdateClassified(update) => {
                session.update_classified(update, now).ok();
            }
            Command::DeleteClassified(classified_id) => {
                session.delete_classified(*classified_id, now).ok();
            }
            Command::GodDeleteClassified {
                classified_id,
                query_id,
            } => {
                session
                    .god_delete_classified(*classified_id, *query_id, now)
                    .ok();
            }
            Command::RequestFolderContents(folder_id) => {
                fetch_folder_contents(&mut session, *folder_id, caps.as_ref(), now);
            }
            Command::FetchInventoryFolders(folder_ids) => {
                if let Some(caps) = caps.as_ref()
                    && let (Some(url), Some(owner)) = (
                        caps.map.get(CAP_FETCH_INVENTORY).cloned(),
                        session.agent_id(),
                    )
                {
                    let events_tx = caps.events_tx.clone();
                    let folders = folder_ids.clone();
                    std::thread::spawn(move || {
                        run_inventory_fetch(
                            &url,
                            owner.uuid(),
                            &folders,
                            CAP_FETCH_INVENTORY,
                            &events_tx,
                        );
                    });
                }
            }
            Command::CreateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .create_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            Command::UpdateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .update_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            Command::MoveInventoryFolder {
                folder_id,
                parent_id,
            } => {
                // Second Life: re-parent via AIS3 (`PATCH /category/<id>`); OpenSim
                // (no cap) keeps the UDP `MoveInventoryFolder`.
                let suffix = ais_category_url(*folder_id);
                let body = build_ais_move_body(*parent_id);
                if !route_ais3(caps.as_ref(), &suffix, Ais3Verb::Patch, Some(body)) {
                    session
                        .move_inventory_folder(*folder_id, *parent_id, now)
                        .ok();
                }
            }
            Command::RemoveInventoryFolders(folder_ids) => {
                // Second Life: delete each folder via AIS3 (`DELETE /category/<id>`);
                // OpenSim (no cap) keeps the UDP batch `RemoveInventoryFolder`.
                if has_ais3(caps.as_ref()) {
                    for folder_id in folder_ids {
                        route_ais3(
                            caps.as_ref(),
                            &ais_category_url(*folder_id),
                            Ais3Verb::Delete,
                            None,
                        );
                    }
                } else {
                    session.remove_inventory_folders(folder_ids, now).ok();
                }
            }
            Command::CreateInventoryItem(new) => {
                session.create_inventory_item(new, now).ok();
            }
            Command::CreateScript {
                folder_id,
                name,
                description,
                next_owner_mask,
                language,
            } => {
                session
                    .create_script(
                        *folder_id,
                        name,
                        description,
                        *next_owner_mask,
                        *language,
                        now,
                    )
                    .ok();
            }
            Command::LinkInventoryItem(new) => {
                // Second Life: create the link via AIS3 (`POST /category/<folder>`
                // with a `links` array). The legacy UDP `LinkInventoryItem` is
                // rejected against the AIS-managed Current Outfit Folder — the
                // "Cannot create requested inventory" alert — so a worn layer's COF
                // link never lands. OpenSim (no cap) keeps the UDP path.
                let suffix = ais_create_category_url(new.folder_id, Uuid::new_v4());
                let body = build_ais_create_link_body(
                    new.linked_id.uuid(),
                    new.link_type.to_code(),
                    new.inv_type.to_code(),
                    &new.name,
                    &new.description,
                );
                if !route_ais3(caps.as_ref(), &suffix, Ais3Verb::Post, Some(body)) {
                    session.link_inventory_item(new, now).ok();
                }
            }
            Command::UpdateInventoryItem {
                item,
                transaction_id,
            } => {
                session
                    .update_inventory_item(item, *transaction_id, now)
                    .ok();
            }
            Command::SaveInventoryAsset {
                item,
                asset_type,
                transaction_id,
                data,
            } => {
                session
                    .save_inventory_asset(item, *asset_type, data.clone(), *transaction_id, now)
                    .ok();
            }
            Command::MoveInventoryItem {
                item_id,
                folder_id,
                new_name,
            } => {
                // Second Life: re-parent via AIS3 (`PATCH /item/<id>` with
                // `{ parent_id }`) — but only a *pure* move: the AIS3 move body
                // carries no name, so a move that also renames stays on UDP rather
                // than silently dropping the rename. OpenSim (no cap) keeps UDP.
                let routed = new_name.is_empty()
                    && route_ais3(
                        caps.as_ref(),
                        &ais_item_url(*item_id),
                        Ais3Verb::Patch,
                        Some(build_ais_move_body(*folder_id)),
                    );
                if !routed {
                    session
                        .move_inventory_item(*item_id, *folder_id, new_name, now)
                        .ok();
                }
            }
            Command::CopyInventoryItem {
                old_agent_id,
                old_item_id,
                new_folder_id,
                new_name,
            } => {
                session
                    .copy_inventory_item(*old_agent_id, *old_item_id, *new_folder_id, new_name, now)
                    .ok();
            }
            Command::RemoveInventoryItems(item_ids) => {
                // Second Life: delete each item via AIS3 (`DELETE /item/<id>`);
                // OpenSim (no cap) keeps the UDP batch `RemoveInventoryItem`.
                if has_ais3(caps.as_ref()) {
                    // Drop the items from the cache optimistically (the AIS3 DELETE
                    // does not), so a taken-off / detached item's Current Outfit link
                    // leaves the model at once; the reply reconverges the folder.
                    session.remove_inventory_items_local(item_ids);
                    for item_id in item_ids {
                        route_ais3(
                            caps.as_ref(),
                            &ais_item_url(*item_id),
                            Ais3Verb::Delete,
                            None,
                        );
                    }
                } else {
                    session.remove_inventory_items(item_ids, now).ok();
                }
            }
            Command::ChangeInventoryItemFlags { item_id, flags } => {
                session
                    .change_inventory_item_flags(*item_id, *flags, now)
                    .ok();
            }
            Command::PurgeInventoryDescendents(folder_id) => {
                // Second Life: empty via AIS3 (`DELETE /category/<id>/children`);
                // OpenSim (no cap) keeps the UDP `PurgeInventoryDescendents`.
                let suffix = ais_category_children_url(*folder_id);
                if !route_ais3(caps.as_ref(), &suffix, Ais3Verb::Delete, None) {
                    session.purge_inventory_descendents(*folder_id, now).ok();
                }
            }
            Command::RemoveInventoryObjects {
                folder_ids,
                item_ids,
            } => {
                session
                    .remove_inventory_objects(folder_ids, item_ids, now)
                    .ok();
            }
            Command::CreateInventoryCategory {
                parent_id,
                folder_type,
                name,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CREATE_INVENTORY_CATEGORY).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let body = build_create_inventory_category_request(
                        InventoryFolderKey::from(Uuid::new_v4()),
                        *parent_id,
                        *folder_type,
                        name,
                    );
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_CREATE_INVENTORY_CATEGORY, &events_tx);
                    });
                }
            }
            Command::Ais3CreateFolder {
                parent_id,
                folder_type,
                name,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!(
                        "{base}{}",
                        ais_create_category_url(*parent_id, Uuid::new_v4())
                    );
                    let body = build_ais_create_category_body(*folder_type, name);
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RenameFolder { folder_id, name } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    let body = build_ais_rename_category_body(name);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3MoveFolder {
                folder_id,
                parent_id,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    let body = build_ais_move_body(*parent_id);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RemoveFolder(folder_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_url(*folder_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3PurgeFolder(folder_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_category_children_url(*folder_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3FetchFolderChildren { folder_id, depth } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!(
                        "{base}{}",
                        ais_category_children_fetch_url(*folder_id, *depth)
                    );
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3UpdateItem {
                item_id,
                name,
                description,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    let body = build_ais_update_item_body(name, description);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3MoveItem { item_id, parent_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    let body = build_ais_move_body(*parent_id);
                    std::thread::spawn(move || {
                        run_patch_caps_llsd(&url, body, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3RemoveItem(item_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    std::thread::spawn(move || {
                        run_delete_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::Ais3FetchItem(item_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_INVENTORY_API_V3).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}{}", ais_item_url(*item_id));
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_INVENTORY_API_V3, &events_tx);
                    });
                }
            }
            Command::FetchGroupMembers(group_id) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GROUP_MEMBER_DATA).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let group = *group_id;
                    std::thread::spawn(move || {
                        run_group_members_fetch(&url, group, &events_tx);
                    });
                }
            }
            Command::OfferFriendship {
                to_agent_id,
                message,
            } => {
                session
                    .send_friendship_offer(*to_agent_id, message, now)
                    .ok();
            }
            Command::GrantUserRights { target, rights } => {
                session.grant_user_rights(*target, *rights, now).ok();
            }
            Command::TerminateFriendship(other) => {
                session.terminate_friendship(*other, now).ok();
            }
            Command::AcceptFriendship {
                transaction_id,
                friend_id,
                calling_card_folder,
            } => {
                session
                    .accept_friendship(*transaction_id, *friend_id, *calling_card_folder, now)
                    .ok();
            }
            Command::DeclineFriendship(transaction_id) => {
                session.decline_friendship(*transaction_id, now).ok();
            }
            Command::OfferCallingCard {
                to_agent_id,
                transaction_id,
            } => {
                session
                    .offer_calling_card(*to_agent_id, *transaction_id, now)
                    .ok();
            }
            Command::AcceptCallingCard {
                transaction_id,
                calling_card_folder,
            } => {
                session
                    .accept_calling_card(*transaction_id, *calling_card_folder, now)
                    .ok();
            }
            Command::DeclineCallingCard(transaction_id) => {
                session.decline_calling_card(*transaction_id, now).ok();
            }
            Command::ActivateGroup(group_id) => {
                session.activate_group(*group_id, now).ok();
            }
            Command::RequestGroupMembers(group_id) => {
                session.request_group_members(*group_id, now).ok();
            }
            Command::RequestGroupRoles(group_id) => {
                session.request_group_roles(*group_id, now).ok();
            }
            Command::RequestGroupRoleMembers(group_id) => {
                session.request_group_role_members(*group_id, now).ok();
            }
            Command::RequestGroupTitles(group_id) => {
                session.request_group_titles(*group_id, now).ok();
            }
            Command::RequestGroupProfile(group_id) => {
                session.request_group_profile(*group_id, now).ok();
            }
            Command::RequestGroupNotices(group_id) => {
                session.request_group_notices(*group_id, now).ok();
            }
            Command::RequestGroupNotice(notice_id) => {
                session.request_group_notice(*notice_id, now).ok();
            }
            Command::CreateGroup(params) => {
                session.create_group(params, now).ok();
            }
            Command::UpdateGroupInfo(params) => {
                session.update_group_info(params, now).ok();
            }
            Command::UpdateGroupTitle {
                group_id,
                title_role_id,
            } => {
                session
                    .update_group_title(*group_id, *title_role_id, now)
                    .ok();
            }
            Command::JoinGroup(group_id) => {
                session.join_group(*group_id, now).ok();
            }
            Command::LeaveGroup(group_id) => {
                session.leave_group(*group_id, now).ok();
            }
            Command::InviteToGroup { group_id, invitees } => {
                session.invite_to_group(*group_id, invitees, now).ok();
            }
            Command::AcceptGroupInvitation {
                group_id,
                transaction_id,
                use_offline_cap,
            } => {
                // An online invitation is answered over UDP; an offline one (null
                // session id) POSTs to the AcceptGroupInvite cap when present.
                if *use_offline_cap {
                    if let Some(caps) = caps.as_ref()
                        && let Some(url) = caps.map.get(CAP_ACCEPT_GROUP_INVITE).cloned()
                    {
                        let body = group_invite_response_body(*group_id);
                        std::thread::spawn(move || run_caps_oneway(&url, body));
                    }
                } else {
                    session
                        .accept_group_invitation(*group_id, *transaction_id, now)
                        .ok();
                }
            }
            Command::DeclineGroupInvitation {
                group_id,
                transaction_id,
                use_offline_cap,
            } => {
                if *use_offline_cap {
                    if let Some(caps) = caps.as_ref()
                        && let Some(url) = caps.map.get(CAP_DECLINE_GROUP_INVITE).cloned()
                    {
                        let body = group_invite_response_body(*group_id);
                        std::thread::spawn(move || run_caps_oneway(&url, body));
                    }
                } else {
                    session
                        .decline_group_invitation(*group_id, *transaction_id, now)
                        .ok();
                }
            }
            Command::SetGroupAcceptNotices {
                group_id,
                accept_notices,
                list_in_profile,
            } => {
                session
                    .set_group_accept_notices(*group_id, *accept_notices, *list_in_profile, now)
                    .ok();
            }
            Command::SetGroupContribution {
                group_id,
                contribution,
            } => {
                session
                    .set_group_contribution(*group_id, *contribution, now)
                    .ok();
            }
            Command::StartGroupSession(group_id) => {
                session.start_group_session(*group_id, now).ok();
            }
            Command::SendGroupMessage { group_id, message } => {
                session.send_group_message(*group_id, message, now).ok();
                if let Some(own) = session.agent_id() {
                    let name = session.agent_legacy_name();
                    chat_log.log_group(*group_id, own, &name, message);
                }
            }
            Command::LeaveGroupSession(group_id) => {
                session.leave_group_session(*group_id, now).ok();
            }
            Command::UpdateGroupRoles { group_id, roles } => {
                session.update_group_roles(*group_id, roles, now).ok();
            }
            Command::ChangeGroupRoleMembers { group_id, changes } => {
                session
                    .change_group_role_members(*group_id, changes, now)
                    .ok();
            }
            Command::EjectGroupMembers {
                group_id,
                member_ids,
            } => {
                session.eject_group_members(*group_id, member_ids, now).ok();
            }
            Command::ActivateGestures { gestures } => {
                session.activate_gestures(gestures, now).ok();
            }
            Command::DeactivateGestures { item_ids } => {
                session.deactivate_gestures(item_ids, now).ok();
            }
            Command::SetAlwaysRun { mode } => {
                session.set_always_run(*mode, now).ok();
            }
            Command::PauseAgent => {
                session.pause_agent(now).ok();
            }
            Command::ResumeAgent => {
                session.resume_agent(now).ok();
            }
            Command::SetAgentFov { vertical_angle } => {
                session.set_agent_fov(*vertical_angle, now).ok();
            }
            Command::SetAgentSize { height, width } => {
                session.set_agent_size(*height, *width, now).ok();
            }
            Command::ReleaseScriptControls => {
                session.release_script_controls(now).ok();
            }
            Command::SendGroupNotice {
                group_id,
                subject,
                message,
                attachment,
            } => {
                session
                    .send_group_notice(*group_id, subject, message, *attachment, now)
                    .ok();
            }
            Command::RequestGroupAccountSummary {
                group_id,
                request_id,
                interval_days,
                current_interval,
            } => {
                session
                    .request_group_account_summary(
                        *group_id,
                        *request_id,
                        *interval_days,
                        *current_interval,
                        now,
                    )
                    .ok();
            }
            Command::RequestGroupAccountDetails {
                group_id,
                request_id,
                interval_days,
                current_interval,
            } => {
                session
                    .request_group_account_details(
                        *group_id,
                        *request_id,
                        *interval_days,
                        *current_interval,
                        now,
                    )
                    .ok();
            }
            Command::RequestGroupAccountTransactions {
                group_id,
                request_id,
                interval_days,
                current_interval,
            } => {
                session
                    .request_group_account_transactions(
                        *group_id,
                        *request_id,
                        *interval_days,
                        *current_interval,
                        now,
                    )
                    .ok();
            }
            Command::RequestGroupActiveProposals {
                group_id,
                transaction_id,
            } => {
                session
                    .request_group_active_proposals(*group_id, *transaction_id, now)
                    .ok();
            }
            Command::RequestGroupVoteHistory {
                group_id,
                transaction_id,
            } => {
                session
                    .request_group_vote_history(*group_id, *transaction_id, now)
                    .ok();
            }
            Command::StartGroupProposal {
                group_id,
                quorum,
                majority,
                duration,
                proposal_text,
            } => {
                session
                    .start_group_proposal(
                        *group_id,
                        *quorum,
                        *majority,
                        *duration,
                        proposal_text,
                        now,
                    )
                    .ok();
            }
            Command::GroupProposalBallot {
                proposal_id,
                group_id,
                vote_cast,
            } => {
                session
                    .cast_group_proposal_ballot(*proposal_id, *group_id, vote_cast, now)
                    .ok();
            }
            Command::ReplyScriptDialog {
                object_id,
                chat_channel,
                button_index,
                button_label,
            } => {
                session
                    .reply_script_dialog(
                        *object_id,
                        *chat_channel,
                        *button_index,
                        button_label,
                        now,
                    )
                    .ok();
            }
            Command::AnswerScriptPermissions {
                task_id,
                item_id,
                permissions,
                experience_id,
            } => {
                session
                    .answer_script_permissions(
                        *task_id,
                        *item_id,
                        *permissions,
                        *experience_id,
                        now,
                    )
                    .ok();
            }
            Command::RequestMuteList => {
                session.request_mute_list(now).ok();
            }
            Command::Mute {
                id,
                name,
                mute_type,
                flags,
            } => {
                session.mute(*id, name, *mute_type, *flags, now).ok();
            }
            Command::Unmute { id, name } => {
                session.unmute(*id, name, now).ok();
            }
            Command::Teleport {
                region_handle,
                position,
                look_at,
            } => {
                session
                    .teleport_to(*region_handle, *position, look_at.clone(), now)
                    .ok();
            }
            Command::RequestRegionInfo => {
                session.request_region_info(now).ok();
            }
            Command::RequestAvatarNames(ids) => {
                session.request_avatar_names(ids, now).ok();
            }
            Command::RequestGroupNames(ids) => {
                session.request_group_names(ids, now).ok();
            }
            Command::RequestEnvironment { parcel_id } => {
                if let Some(caps) = caps.as_ref() {
                    if let Some(base) = caps.map.get(CAP_EXT_ENVIRONMENT).cloned() {
                        let events_tx = caps.events_tx.clone();
                        let url = format!("{base}?parcelid={}", parcel_id.unwrap_or(-1));
                        tracing::info!(
                            target: "sl_client_bevy::environment",
                            "requesting EEP environment from {CAP_EXT_ENVIRONMENT} cap"
                        );
                        std::thread::spawn(move || {
                            run_get_caps_llsd(&url, CAP_EXT_ENVIRONMENT, &events_tx);
                        });
                    } else {
                        tracing::warn!(
                            target: "sl_client_bevy::environment",
                            "RequestEnvironment: the {CAP_EXT_ENVIRONMENT} capability is not \
                             advertised by this region; the sky / cloud / water stack will run \
                             on the legacy WindLight defaults ({} caps available)",
                            caps.map.len()
                        );
                    }
                } else {
                    tracing::warn!(
                        target: "sl_client_bevy::environment",
                        "RequestEnvironment: no CAPS available yet; environment not requested"
                    );
                }
            }
            Command::SetEnvironment {
                parcel_id,
                track_no,
                update,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_EXT_ENVIRONMENT).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let parcel_id = parcel_id.unwrap_or(-1);
                    let url = match track_no {
                        Some(track_no) => {
                            format!("{base}?parcelid={parcel_id}&trackno={track_no}")
                        }
                        None => format!("{base}?parcelid={parcel_id}"),
                    };
                    let body = build_environment_update_request(update);
                    std::thread::spawn(move || {
                        run_put_caps_llsd(&url, body, CAP_EXT_ENVIRONMENT, &events_tx);
                    });
                }
            }
            Command::RequestMoneyBalance => {
                session.request_money_balance(now).ok();
            }
            Command::RequestEconomyData => {
                session.request_economy_data(now).ok();
            }
            Command::SendMoneyTransfer {
                dest,
                amount,
                kind,
                description,
            } => {
                session
                    .send_money_transfer(*dest, amount.clone(), *kind, description, now)
                    .ok();
            }
            Command::RequestParcelProperties {
                west,
                south,
                east,
                north,
                sequence_id,
            } => {
                session
                    .request_parcel_properties(*west, *south, *east, *north, *sequence_id, now)
                    .ok();
            }
            Command::RequestParcelPropertiesById {
                local_id,
                sequence_id,
            } => {
                session
                    .request_parcel_properties_by_id(*local_id, *sequence_id, now)
                    .ok();
            }
            Command::SetParcelOtherCleanTime {
                local_id,
                clean_time,
            } => {
                session
                    .set_parcel_other_clean_time(*local_id, *clean_time, now)
                    .ok();
            }
            Command::ModifyLand(edit) => {
                session.modify_land(edit, now).ok();
            }
            Command::UndoLand => {
                session.undo_land(now).ok();
            }
            Command::SetDrawDistance(far) => session.set_draw_distance(far.clone()),
            Command::RequestMapBlocks {
                min_x,
                max_x,
                min_y,
                max_y,
            } => {
                session
                    .request_map_blocks(*min_x, *max_x, *min_y, *max_y, now)
                    .ok();
            }
            Command::RequestMapByName { name } => {
                session.request_map_by_name(name, now).ok();
            }
            Command::RequestMapItems {
                item_type,
                region_handle,
            } => {
                session
                    .request_map_items(*item_type, *region_handle, now)
                    .ok();
            }
            Command::RequestMapLayer => {
                session.request_map_layer(now).ok();
            }
            Command::SendAbuseReport(report) => {
                session.send_abuse_report(report, now).ok();
            }
            Command::SendAbuseReportViaCaps { report, screenshot } => {
                if let Some(caps) = caps.as_ref() {
                    // With a snapshot and the screenshot cap available, upload the
                    // snapshot over the two-step uploader (filling `screenshot_id`
                    // with a fresh texture asset id) and POST the report referencing
                    // it; otherwise the plain no-screenshot path.
                    let snapshot = screenshot
                        .as_ref()
                        .filter(|bytes| !bytes.is_empty())
                        .and_then(|bytes| {
                            caps.map
                                .get(CAP_SEND_USER_REPORT_WITH_SCREENSHOT)
                                .cloned()
                                .map(|url| (url, bytes.clone()))
                        });
                    match snapshot {
                        Some((url, bytes)) => {
                            let mut report = report.clone();
                            if report.screenshot_id.is_nil() {
                                report.screenshot_id = Uuid::new_v4();
                            }
                            let body = build_send_user_report(&report);
                            std::thread::spawn(move || {
                                run_report_screenshot_upload(&url, body, bytes);
                            });
                        }
                        None => {
                            if let Some(url) = caps.map.get(CAP_SEND_USER_REPORT).cloned() {
                                let body = build_send_user_report(report);
                                std::thread::spawn(move || {
                                    run_caps_oneway(&url, body);
                                });
                            }
                        }
                    }
                }
            }
            Command::SendPostcard(postcard) => {
                session.send_postcard(postcard, now).ok();
            }
            Command::RequestObjects { local_ids } => {
                session.request_objects(local_ids, now).ok();
            }
            Command::ResendCachedObjects { local_ids } => {
                session.resend_cached_objects(local_ids);
            }
            Command::RequestObjectProperties { local_ids } => {
                session.request_object_properties(local_ids, now).ok();
            }
            Command::DeselectObjects { local_ids } => {
                session.deselect_objects(local_ids, now).ok();
            }
            Command::TouchObject { local_id, surface } => {
                session.touch_object(*local_id, surface.as_ref(), now).ok();
            }
            Command::GrabObject {
                local_id,
                grab_offset,
                surface,
            } => {
                session
                    .grab_object(*local_id, grab_offset.clone(), surface.as_ref(), now)
                    .ok();
            }
            Command::GrabObjectUpdate {
                object_id,
                grab_offset_initial,
                grab_position,
                time_since_last,
                surface,
            } => {
                session
                    .grab_object_update(
                        *object_id,
                        grab_offset_initial.clone(),
                        grab_position.clone(),
                        *time_since_last,
                        surface.as_ref(),
                        now,
                    )
                    .ok();
            }
            Command::DegrabObject { local_id, surface } => {
                session.degrab_object(*local_id, surface.as_ref(), now).ok();
            }
            Command::RezObject { shape, group_id } => {
                session.rez_object(shape, *group_id, now).ok();
            }
            Command::DuplicateObjects {
                local_ids,
                offset,
                group_id,
            } => {
                session
                    .duplicate_objects(local_ids, offset.clone(), *group_id, now)
                    .ok();
            }
            Command::DeleteObjects { local_ids } => {
                session.delete_objects(local_ids, now).ok();
            }
            Command::DerezObjects {
                local_ids,
                destination,
                transaction_id,
                group_id,
            } => {
                session
                    .derez_objects(local_ids, *destination, *transaction_id, *group_id, now)
                    .ok();
            }
            Command::UpdateObject {
                local_id,
                transform,
            } => {
                session.update_object(*local_id, transform, now).ok();
            }
            Command::SetObjectName { local_id, name } => {
                session.set_object_name(*local_id, name, now).ok();
            }
            Command::SetObjectDescription {
                local_id,
                description,
            } => {
                session
                    .set_object_description(*local_id, description, now)
                    .ok();
            }
            Command::SetObjectClickAction { local_id, action } => {
                session
                    .set_object_click_action(*local_id, *action, now)
                    .ok();
            }
            Command::SetObjectMaterial { local_id, material } => {
                session.set_object_material(*local_id, *material, now).ok();
            }
            Command::SetObjectFlags { local_id, flags } => {
                session.set_object_flags(*local_id, flags, now).ok();
            }
            Command::SetObjectShape { local_id, shape } => {
                session.set_object_shape(*local_id, shape, now).ok();
            }
            Command::SetObjectImage {
                local_id,
                media_url,
                texture_entry,
            } => {
                session
                    .set_object_image(*local_id, media_url.as_deref(), texture_entry, now)
                    .ok();
            }
            Command::SetObjectExtraParams { local_id, params } => {
                session.set_object_extra_params(*local_id, params, now).ok();
            }
            Command::SetObjectGroup {
                local_ids,
                group_id,
            } => {
                session.set_object_group(local_ids, *group_id, now).ok();
            }
            Command::DeedObjectsToGroup {
                local_ids,
                group_id,
            } => {
                session
                    .deed_objects_to_group(local_ids, *group_id, now)
                    .ok();
            }
            Command::SetObjectPermissions {
                local_ids,
                field,
                set,
                mask,
            } => {
                session
                    .set_object_permissions(local_ids, *field, *set, *mask, now)
                    .ok();
            }
            Command::SetObjectForSale {
                local_id,
                sale_type,
                sale_price,
            } => {
                session
                    .set_object_for_sale(*local_id, *sale_type, sale_price.clone(), now)
                    .ok();
            }
            Command::SetObjectCategory { local_id, category } => {
                session.set_object_category(*local_id, *category, now).ok();
            }
            Command::SetObjectIncludeInSearch { local_id, include } => {
                session
                    .set_object_include_in_search(*local_id, *include, now)
                    .ok();
            }
            Command::LinkObjects { local_ids } => {
                session.link_objects(local_ids, now).ok();
            }
            Command::DelinkObjects { local_ids } => {
                session.delink_objects(local_ids, now).ok();
            }
            Command::UndoObjects { local_ids } => {
                session.undo_objects(local_ids, now).ok();
            }
            Command::RedoObjects { local_ids } => {
                session.redo_objects(local_ids, now).ok();
            }
            Command::UpdateParcel(update) => {
                session.update_parcel(update, now).ok();
            }
            Command::RequestParcelAccessList { local_id, scope } => {
                session
                    .request_parcel_access_list(*local_id, *scope, now)
                    .ok();
            }
            Command::UpdateParcelAccessList {
                local_id,
                scope,
                entries,
            } => {
                session
                    // A fresh transaction id per update, so the simulator clears
                    // the old entries before applying ours rather than appending
                    // (see `update_parcel_access_list`).
                    .update_parcel_access_list(*local_id, *scope, entries, Uuid::new_v4(), now)
                    .ok();
            }
            Command::RequestParcelDwell { local_id } => {
                session.request_parcel_dwell(*local_id, now).ok();
            }
            Command::BuyParcel {
                local_id,
                price,
                area,
                group_id,
                is_group_owned,
            } => {
                session
                    .buy_parcel(*local_id, *price, *area, *group_id, *is_group_owned, now)
                    .ok();
            }
            Command::ReturnParcelObjects {
                local_id,
                return_type,
                owner_ids,
                task_ids,
            } => {
                session
                    .return_parcel_objects(*local_id, *return_type, owner_ids, task_ids, now)
                    .ok();
            }
            Command::SelectParcelObjects {
                local_id,
                return_type,
                object_ids,
            } => {
                session
                    .select_parcel_objects(*local_id, *return_type, object_ids, now)
                    .ok();
            }
            Command::DeedParcelToGroup { local_id, group_id } => {
                session.deed_parcel_to_group(*local_id, *group_id, now).ok();
            }
            Command::ReclaimParcel { local_id } => {
                session.reclaim_parcel(*local_id, now).ok();
            }
            Command::ReleaseParcel { local_id } => {
                session.release_parcel(*local_id, now).ok();
            }
            Command::JoinParcels {
                west,
                south,
                east,
                north,
            } => {
                session.join_parcels(*west, *south, *east, *north, now).ok();
            }
            Command::DivideParcel {
                west,
                south,
                east,
                north,
            } => {
                session
                    .divide_parcel(*west, *south, *east, *north, now)
                    .ok();
            }
            Command::RequestParcelObjectOwners { local_id } => {
                session.request_parcel_object_owners(*local_id, now).ok();
            }
            Command::BuyParcelPass { local_id } => {
                session.buy_parcel_pass(*local_id, now).ok();
            }
            Command::DisableParcelObjects {
                local_id,
                return_type,
                owner_ids,
                task_ids,
            } => {
                session
                    .disable_parcel_objects(*local_id, *return_type, owner_ids, task_ids, now)
                    .ok();
            }
            Command::RequestParcelInfo { parcel_id } => {
                session.request_parcel_info(*parcel_id, now).ok();
            }
            Command::RequestEstateInfo => {
                session.request_estate_info(now).ok();
            }
            Command::RequestRegionTerrainDownload { viewer_filename } => {
                session
                    .request_region_terrain_download(viewer_filename, now)
                    .ok();
            }
            Command::RequestRegionTerrainUpload {
                viewer_filename,
                data,
            } => {
                session
                    .request_region_terrain_upload(viewer_filename, data.clone(), now)
                    .ok();
            }
            Command::UpdateEstateAccess { delta, target } => {
                session.update_estate_access(*delta, *target, now).ok();
            }
            Command::KickEstateUser { target } => {
                session.kick_estate_user(*target, now).ok();
            }
            Command::TeleportHomeUser { target } => {
                session.teleport_home_user(*target, now).ok();
            }
            Command::TeleportHomeAllUsers => {
                session.teleport_home_all_users(now).ok();
            }
            Command::RestartRegion { seconds } => {
                session.restart_region(*seconds, now).ok();
            }
            Command::SendEstateMessage { message } => {
                session.send_estate_message(message, now).ok();
            }
            Command::SetRegionInfo(update) => {
                session.set_region_info(update, now).ok();
            }
            Command::SetRegionDebug(update) => {
                session.set_region_debug(update, now).ok();
            }
            Command::SetRegionTerrain(update) => {
                session.set_region_terrain(update, now).ok();
            }
            Command::SetEstateInfo(update) => {
                session.set_estate_info(update, now).ok();
            }
            Command::RequestEstateCovenant => {
                session.request_estate_covenant(now).ok();
            }
            Command::RequestTelehubInfo => {
                session.request_telehub_info(now).ok();
            }
            Command::ConnectTelehub { object_local_id } => {
                session.connect_telehub(*object_local_id, now).ok();
            }
            Command::DisconnectTelehub => {
                session.disconnect_telehub(now).ok();
            }
            Command::AddTelehubSpawnPoint { object_local_id } => {
                session.add_telehub_spawn_point(*object_local_id, now).ok();
            }
            Command::RemoveTelehubSpawnPoint { spawn_index } => {
                session.remove_telehub_spawn_point(*spawn_index, now).ok();
            }
            Command::GodKickUser { target, reason } => {
                session.god_kick_user(*target, reason, now).ok();
            }
            Command::SendGodlikeMessage { method, params } => {
                let refs: Vec<&str> = params.iter().map(String::as_str).collect();
                session.send_godlike_message(method, &refs, now).ok();
            }
            Command::RequestTexture {
                texture_id,
                discard_level,
                priority,
            } => {
                session
                    .request_texture(*texture_id, *discard_level, *priority, now)
                    .ok();
            }
            Command::FetchTexture {
                texture_id,
                discard_level,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_TEXTURE).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, discard) = (*texture_id, *discard_level);
                    std::thread::spawn(move || {
                        run_texture_fetch(&url, id, discard, &asset_tx);
                    });
                }
            }
            Command::FetchMesh {
                mesh_id,
                byte_range,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps
                        .map
                        .get(CAP_GET_MESH2)
                        .or_else(|| caps.map.get(CAP_GET_MESH))
                        .cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, range) = (mesh_id.uuid(), *byte_range);
                    std::thread::spawn(move || {
                        run_asset_fetch(
                            &url,
                            &format!("?mesh_id={id}"),
                            id,
                            AssetType::Mesh,
                            range,
                            &asset_tx,
                        );
                    });
                }
            }
            Command::FetchAsset {
                asset_id,
                asset_type,
                byte_range,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_VIEWER_ASSET).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, asset_type, range) = (asset_id.uuid(), *asset_type, *byte_range);
                    std::thread::spawn(move || {
                        run_generic_asset_fetch(&url, id, asset_type, range, &asset_tx);
                    });
                }
            }
            Command::RequestWearables => {
                session.request_wearables(now).ok();
            }
            Command::SetWearing(wearables) => {
                session.set_wearing(wearables, now).ok();
            }
            Command::SetAppearance {
                serial,
                size,
                texture_entry,
                visual_params,
                wearable_cache,
            } => {
                session
                    .set_appearance(
                        *serial,
                        size.clone(),
                        texture_entry,
                        visual_params,
                        wearable_cache,
                        now,
                    )
                    .ok();
            }
            Command::RequestCachedTextures { serial, slots } => {
                session.request_cached_textures(*serial, slots, now).ok();
            }
            Command::RequestServerAppearanceUpdate { cof_version } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPDATE_AVATAR_APPEARANCE).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let version = *cof_version;
                    std::thread::spawn(move || {
                        run_server_appearance_update(&url, version, &events_tx);
                    });
                }
            }
            Command::SetAnimations(animations) => {
                session.set_animations(animations, now).ok();
            }
            Command::PlayAnimation(anim_id) => {
                session.play_animation(*anim_id, now).ok();
            }
            Command::StopAnimation(anim_id) => {
                session.stop_animation(*anim_id, now).ok();
            }
            Command::AttachObject {
                local_id,
                attachment_point,
                mode,
                rotation,
            } => {
                session
                    .attach_object(*local_id, *attachment_point, *mode, rotation, now)
                    .ok();
            }
            Command::DetachObjects { local_ids } => {
                session.detach_objects(local_ids, now).ok();
            }
            Command::DropAttachments { local_ids } => {
                session.drop_attachments(local_ids, now).ok();
            }
            Command::RemoveAttachment {
                attachment_point,
                item_id,
            } => {
                session
                    .remove_attachment(*attachment_point, *item_id, now)
                    .ok();
            }
            Command::RezAttachment(rez) => {
                session.rez_attachment(rez, now).ok();
            }
            Command::RezAttachments {
                compound_id,
                detach,
                attachments,
            } => {
                session
                    .rez_attachments(*compound_id, *detach, attachments, now)
                    .ok();
            }
            Command::ViewerEffect(effects) => {
                session.send_viewer_effect(effects, now).ok();
            }
            Command::TrackAgent { prey_id } => {
                session.track_agent(*prey_id, now).ok();
            }
            Command::FindAgent { hunter, prey } => {
                session.find_agent(*hunter, *prey, now).ok();
            }
            Command::DirFindQuery {
                query_id,
                query_text,
                flags,
                query_start,
            } => {
                session
                    .dir_find_query(*query_id, query_text, *flags, *query_start, now)
                    .ok();
            }
            Command::DirPlacesQuery {
                query_id,
                query_text,
                flags,
                category,
                sim_name,
                query_start,
            } => {
                session
                    .dir_places_query(
                        *query_id,
                        query_text,
                        *flags,
                        *category,
                        sim_name,
                        *query_start,
                        now,
                    )
                    .ok();
            }
            Command::DirLandQuery {
                query_id,
                flags,
                search_type,
                price,
                area,
                query_start,
            } => {
                session
                    .dir_land_query(
                        *query_id,
                        *flags,
                        *search_type,
                        *price,
                        *area,
                        *query_start,
                        now,
                    )
                    .ok();
            }
            Command::DirClassifiedQuery {
                query_id,
                query_text,
                flags,
                category,
                query_start,
            } => {
                session
                    .dir_classified_query(
                        *query_id,
                        query_text,
                        *flags,
                        *category,
                        *query_start,
                        now,
                    )
                    .ok();
            }
            Command::AvatarPickerRequest { query_id, name } => {
                // The modern search is the `AvatarPickerSearch` GET, which
                // matches username *and* display name; the legacy UDP message
                // is the fallback for a grid without the cap (on Second Life it
                // answers "no matches" to everything).
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_AVATAR_PICKER_SEARCH).cloned()
                {
                    let url = format!(
                        "{base}{}",
                        avatar_picker_search_query(name, AVATAR_PICKER_PAGE_SIZE)
                    );
                    let events_tx = caps.events_tx.clone();
                    let query_uuid = query_id.get();
                    std::thread::spawn(move || {
                        run_avatar_picker_search(&url, query_uuid, &events_tx);
                    });
                } else {
                    session.avatar_picker_request(*query_id, name, now).ok();
                }
            }
            Command::PlacesQuery {
                query_id,
                transaction_id,
                query_text,
                flags,
                category,
                sim_name,
            } => {
                session
                    .places_query(
                        *query_id,
                        *transaction_id,
                        query_text,
                        *flags,
                        *category,
                        sim_name,
                        now,
                    )
                    .ok();
            }
            Command::EventInfoRequest { event_id } => {
                session.event_info_request(*event_id, now).ok();
            }
            Command::EventNotificationAddRequest { event_id } => {
                session.event_notification_add_request(*event_id, now).ok();
            }
            Command::EventNotificationRemoveRequest { event_id } => {
                session
                    .event_notification_remove_request(*event_id, now)
                    .ok();
            }
            Command::BuyObject {
                group_id,
                category_id,
                objects,
            } => {
                session
                    .buy_object(*group_id, *category_id, objects, now)
                    .ok();
            }
            Command::BuyObjectInventory {
                object_id,
                item_id,
                folder_id,
            } => {
                session
                    .buy_object_inventory(*object_id, *item_id, *folder_id, now)
                    .ok();
            }
            Command::RequestPayPrice { object_id } => {
                session.request_pay_price(*object_id, now).ok();
            }
            Command::RequestObjectPropertiesFamily {
                request_flags,
                object_id,
            } => {
                session
                    .request_object_properties_family(*request_flags, *object_id, now)
                    .ok();
            }
            Command::SpinObjectStart { object_id } => {
                session.spin_object_start(*object_id, now).ok();
            }
            Command::SpinObjectUpdate {
                object_id,
                rotation,
            } => {
                session
                    .spin_object_update(*object_id, rotation.clone(), now)
                    .ok();
            }
            Command::SpinObjectStop { object_id } => {
                session.spin_object_stop(*object_id, now).ok();
            }
            Command::DuplicateObjectsOnRay {
                local_ids,
                group_id,
                ray_start,
                ray_end,
                bypass_raycast,
                ray_end_is_intersection,
                copy_centers,
                copy_rotates,
                ray_target_id,
                duplicate_flags,
            } => {
                session
                    .duplicate_objects_on_ray(
                        local_ids,
                        *group_id,
                        ray_start.clone(),
                        ray_end.clone(),
                        *bypass_raycast,
                        *ray_end_is_intersection,
                        *copy_centers,
                        *copy_rotates,
                        *ray_target_id,
                        *duplicate_flags,
                        now,
                    )
                    .ok();
            }
            Command::RezRestoreToWorld { item } => {
                session.rez_restore_to_world(item, now).ok();
            }
            Command::RezObjectFromNotecard { rez } => {
                session.rez_object_from_notecard(rez, now).ok();
            }
            Command::CopyInventoryFromNotecard {
                notecard_id,
                object_id,
                item_id,
                folder_id,
            } => {
                // Copy an item embedded in a notecard into inventory: a one-way
                // LLSD POST to the cap. The copied item arrives over the normal
                // inventory-update stream, so nothing is awaited here.
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_COPY_INVENTORY_FROM_NOTECARD).cloned()
                {
                    let body = copy_inventory_from_notecard_body(
                        *notecard_id,
                        *object_id,
                        *item_id,
                        *folder_id,
                    );
                    std::thread::spawn(move || {
                        post_caps_llsd_oneway(&url, body);
                    });
                }
            }
            Command::RezObjectFromInventory { params } => {
                session.rez_object_from_inventory(params, now).ok();
            }
            Command::RezScript { target, params } => {
                session.rez_script(*target, params, now).ok();
            }
            Command::RevokeScriptPermissions {
                object_id,
                permissions,
            } => {
                session
                    .revoke_script_permissions(*object_id, *permissions, now)
                    .ok();
            }
            Command::QueryScriptPermissions => {
                // Local query: synthesize the snapshot from the session and surface
                // it on the event stream (no wire send).
                outbound
                    .send(NetOutbound::Event(SessionEvent::ScriptPermissionState(
                        session.script_permission_state(),
                    )))
                    .ok();
            }
            Command::DetachAttachmentIntoInventory { item_id } => {
                session.detach_attachment_into_inventory(*item_id, now).ok();
            }
            Command::RequestTaskInventory { target } => {
                session.request_task_inventory(*target, now).ok();
            }
            Command::FetchTaskInventory { target } => {
                session.fetch_task_inventory(*target, now).ok();
            }
            Command::FetchTaskItemAsset {
                task,
                item_id,
                asset_id,
                asset_type,
            } => {
                session
                    .fetch_task_item_asset(*task, *item_id, *asset_id, *asset_type, now)
                    .ok();
            }
            Command::FetchEstateCovenantAsset => {
                session.fetch_estate_covenant_asset(now).ok();
            }
            Command::RequestXfer { filename } => {
                session.request_xfer(filename, now).ok();
            }
            Command::UpdateTaskInventory { target, key, item } => {
                session.update_task_inventory(*target, *key, item, now).ok();
            }
            Command::MoveTaskInventory {
                target,
                folder_id,
                item_id,
            } => {
                session
                    .move_task_inventory(*target, *folder_id, *item_id, now)
                    .ok();
            }
            Command::RemoveTaskInventory { target, item_id } => {
                session.remove_task_inventory(*target, *item_id, now).ok();
            }
            Command::RequestScriptRunning { object_id, item_id } => {
                session
                    .request_script_running(*object_id, *item_id, now)
                    .ok();
            }
            Command::SetScriptRunning {
                object_id,
                item_id,
                running,
            } => {
                session
                    .set_script_running(*object_id, *item_id, *running, now)
                    .ok();
            }
            Command::ResetScript { object_id, item_id } => {
                session.reset_script(*object_id, *item_id, now).ok();
            }
            Command::UploadAsset { asset_type, .. } if asset_type.is_script() => {
                // Scripts must go through `UploadScript` so the simulator's
                // compile result is surfaced; the generic create-with-body path
                // would discard it.
                emit_upload_failure(
                    caps.as_ref(),
                    "scripts must be uploaded with UploadScript (create the item with \
                        create_inventory_item first)"
                        .to_owned(),
                );
            }
            Command::UploadAsset {
                folder_id,
                asset_type,
                inventory_type,
                name,
                description,
                next_owner_mask,
                group_mask,
                everyone_mask,
                expected_upload_cost,
                data,
            } => {
                // The modern CAPS uploader (the only upload path — the legacy UDP
                // asset-upload fallback was dropped): needs both the region
                // capability and a CAPS name for the asset and inventory classes.
                let caps_available = matches!(
                    (asset_type.caps_asset_name(), inventory_type.caps_name()),
                    (Some(_), Some(_))
                ) && caps
                    .as_ref()
                    .is_some_and(|caps| caps.map.contains_key(CAP_NEW_FILE_AGENT_INVENTORY));
                if caps_available {
                    spawn_new_file_upload(
                        caps.as_ref(),
                        *folder_id,
                        *asset_type,
                        *inventory_type,
                        name,
                        description,
                        *next_owner_mask,
                        *group_mask,
                        *everyone_mask,
                        *expected_upload_cost,
                        data.clone(),
                    );
                } else {
                    emit_upload_failure(
                        caps.as_ref(),
                        "NewFileAgentInventory capability not available".to_owned(),
                    );
                }
            }
            Command::UploadBakedTexture { data } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPLOAD_BAKED_TEXTURE).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let body = build_upload_baked_texture_request();
                    let data = data.clone();
                    std::thread::spawn(move || {
                        let event = run_caps_upload(&url, body, data);
                        asset_tx.send(event).ok();
                    });
                } else {
                    emit_upload_unavailable(caps.as_ref(), "UploadBakedTexture");
                }
            }
            Command::UpdateInventoryAsset {
                location,
                asset_type,
                data,
            } => {
                // `UpdatableAssetType::cap` / `task_cap` are total — scripts
                // (which need the compile-aware `UploadScript`) are excluded from
                // this type by construction. The location picks the agent vs task
                // capability and the metadata body shape.
                let (cap, body) = match location {
                    AssetUpdateLocation::AgentInventory { item_id } => {
                        (asset_type.cap(), build_update_item_asset_request(*item_id))
                    }
                    AssetUpdateLocation::TaskInventory { task_id, item_id } => (
                        asset_type.task_cap(),
                        build_update_task_item_asset_request(*task_id, *item_id),
                    ),
                };
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(cap).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let data = data.clone();
                    std::thread::spawn(move || {
                        let event = run_caps_upload(&url, body, data);
                        asset_tx.send(event).ok();
                    });
                } else {
                    emit_upload_unavailable(caps.as_ref(), cap);
                }
            }
            Command::UploadScript {
                location,
                target,
                source,
            } => {
                // Choose the capability + request body by location; the completion
                // carries the simulator's compile result.
                let target_wire = target.to_wire();
                let (cap, body, running) = match location {
                    ScriptUploadLocation::AgentInventory { item_id } => (
                        CAP_UPDATE_SCRIPT_AGENT,
                        build_update_script_agent_request(*item_id, target_wire),
                        None,
                    ),
                    ScriptUploadLocation::TaskInventory {
                        task_id,
                        item_id,
                        running,
                        experience,
                    } => (
                        CAP_UPDATE_SCRIPT_TASK,
                        build_update_script_task_request(
                            *task_id,
                            *item_id,
                            *running,
                            target_wire,
                            *experience,
                        ),
                        Some(*running),
                    ),
                };
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(cap).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let source = source.clone();
                    std::thread::spawn(move || {
                        asset_tx
                            .send(run_script_upload(&url, body, source, running))
                            .ok();
                    });
                } else {
                    emit_upload_unavailable(caps.as_ref(), cap);
                }
            }
            Command::RequestObjectMedia { object_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let object = *object_id;
                    std::thread::spawn(move || {
                        run_object_media_fetch(&url, object, &events_tx);
                    });
                }
            }
            Command::SetObjectMedia { object_id, faces } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA).cloned()
                {
                    let body = build_object_media_update_request(*object_id, faces);
                    std::thread::spawn(move || {
                        post_caps_llsd_oneway(&url, body);
                    });
                }
            }
            Command::NavigateObjectMedia {
                object_id,
                face,
                url: media_url,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA_NAVIGATE).cloned()
                {
                    let body = build_object_media_navigate_request(*object_id, *face, media_url);
                    std::thread::spawn(move || {
                        post_caps_llsd_oneway(&url, body);
                    });
                }
            }
            Command::RequestRenderMaterials { material_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_RENDER_MATERIALS).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let ids = material_ids.clone();
                    std::thread::spawn(move || {
                        run_render_materials_fetch(&url, ids, &asset_tx);
                    });
                }
            }
            Command::SetRenderMaterials { updates } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_RENDER_MATERIALS).cloned()
                {
                    let body = build_render_materials_put_request(updates);
                    std::thread::spawn(move || {
                        run_set_render_materials(&url, body);
                    });
                }
            }
            Command::ModifyMaterialParams { updates } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_MODIFY_MATERIAL_PARAMS).cloned()
                {
                    let body = build_modify_material_params_request(updates);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_modify_material_params(&url, body, &events_tx);
                    });
                }
            }
            Command::RequestVoiceAccount { request } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_PROVISION_VOICE_ACCOUNT).cloned()
                {
                    let body = build_provision_voice_account_request(request);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_PROVISION_VOICE_ACCOUNT, &events_tx);
                    });
                }
            }
            Command::RequestParcelVoiceInfo => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_PARCEL_VOICE_INFO).cloned()
                {
                    let body = build_parcel_voice_info_request();
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_PARCEL_VOICE_INFO, &events_tx);
                    });
                }
            }
            Command::SendVoiceSignaling {
                viewer_session,
                candidates,
                completed,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_VOICE_SIGNALING).cloned()
                {
                    let body =
                        build_voice_signaling_request(viewer_session, candidates, *completed);
                    std::thread::spawn(move || {
                        run_voice_signaling(&url, body);
                    });
                }
            }
            Command::RequestDisplayNames(agent_ids) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GET_DISPLAY_NAMES).cloned()
                {
                    let agent_uuids: Vec<Uuid> = agent_ids.iter().map(AgentKey::uuid).collect();
                    let url = format!("{base}{}", display_names_query(&agent_uuids));
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_DISPLAY_NAMES, &events_tx);
                    });
                }
            }
            Command::RequestRemoteParcelId {
                location,
                region_id,
                region_handle,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REMOTE_PARCEL_REQUEST).cloned()
                {
                    let body = build_remote_parcel_request(*location, *region_id, *region_handle);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_REMOTE_PARCEL_REQUEST, &events_tx);
                    });
                }
            }
            Command::RequestSimulatorFeatures => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_SIMULATOR_FEATURES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_SIMULATOR_FEATURES, &events_tx);
                    });
                }
            }
            Command::RequestAgentPreferences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_AGENT_PREFERENCES).cloned()
                {
                    let body = build_agent_preferences_request(&AgentPreferences::default());
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_AGENT_PREFERENCES, &events_tx);
                    });
                }
            }
            Command::SetAgentPreferences(prefs) => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_AGENT_PREFERENCES).cloned()
                {
                    let body = build_agent_preferences_request(prefs);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_AGENT_PREFERENCES, &events_tx);
                    });
                }
            }
            Command::RequestObjectCost { object_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_OBJECT_COST).cloned()
                {
                    let body = build_get_object_cost_request(object_ids);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_GET_OBJECT_COST, &events_tx);
                    });
                }
            }
            Command::RequestSelectedCost { object_ids, roots } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_RESOURCE_COST_SELECTED).cloned()
                {
                    let kind = if *roots {
                        SelectedCostKind::Roots
                    } else {
                        SelectedCostKind::Prims
                    };
                    let body = build_resource_cost_selected_request(kind, object_ids);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_RESOURCE_COST_SELECTED, &events_tx);
                    });
                }
            }
            Command::RequestObjectPhysicsData { object_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_OBJECT_PHYSICS_DATA).cloned()
                {
                    let body = build_get_object_physics_data_request(object_ids);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_GET_OBJECT_PHYSICS_DATA, &events_tx);
                    });
                }
            }
            Command::RequestAttachmentResources => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_ATTACHMENT_RESOURCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_ATTACHMENT_RESOURCES, &events_tx);
                    });
                }
            }
            Command::RequestLandResources { parcel_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_LAND_RESOURCES).cloned()
                {
                    let parcel_id = *parcel_id;
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_land_resources(&url, parcel_id, &events_tx);
                    });
                }
            }
            Command::RequestLandStat {
                report_type,
                request_flags,
                filter,
                parcel_local_id,
            } => {
                session
                    .request_land_stat(*report_type, *request_flags, filter, *parcel_local_id, now)
                    .ok();
            }
            Command::RequestExperienceInfo { experience_ids } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GET_EXPERIENCE_INFO).cloned()
                {
                    let url = format!("{base}{}", experience_info_query(experience_ids));
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_EXPERIENCE_INFO, &events_tx);
                    });
                }
            }
            Command::FindExperiences { query, page } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_FIND_EXPERIENCE_BY_NAME).cloned()
                {
                    let url = format!("{base}{}", find_experience_query(query, *page));
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_FIND_EXPERIENCE_BY_NAME, &events_tx);
                    });
                }
            }
            Command::RequestExperiencePermissions => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::SetExperiencePermission {
                experience_id,
                permission,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_EXPERIENCE_PREFERENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    if permission.is_forget() {
                        let url = format!("{base}{}", forget_experience_query(*experience_id));
                        std::thread::spawn(move || {
                            run_delete_caps_llsd(&url, CAP_EXPERIENCE_PREFERENCES, &events_tx);
                        });
                    } else {
                        let body =
                            build_set_experience_permission_request(*experience_id, *permission);
                        std::thread::spawn(move || {
                            run_put_caps_llsd(&base, body, CAP_EXPERIENCE_PREFERENCES, &events_tx);
                        });
                    }
                }
            }
            Command::RequestOwnedExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_AGENT_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_AGENT_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestAdminExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ADMIN_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_ADMIN_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestCreatorExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_CREATOR_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_CREATOR_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::RequestGroupExperiences { group_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GROUP_EXPERIENCES).cloned()
                {
                    let url = format!("{base}{}", group_experiences_query(group_id.uuid()));
                    let group_id = *group_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_group_experiences(&url, group_id, &asset_tx);
                    });
                }
            }
            Command::RequestExperienceAdmin { experience_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_IS_EXPERIENCE_ADMIN).cloned()
                {
                    let url = format!("{base}{}", experience_id_query(*experience_id));
                    let experience_id = *experience_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_experience_status(&url, experience_id, true, &asset_tx);
                    });
                }
            }
            Command::RequestExperienceContributor { experience_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_IS_EXPERIENCE_CONTRIBUTOR).cloned()
                {
                    let url = format!("{base}{}", experience_id_query(*experience_id));
                    let experience_id = *experience_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_experience_status(&url, experience_id, false, &asset_tx);
                    });
                }
            }
            Command::UpdateExperience { update } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_UPDATE_EXPERIENCE).cloned()
                {
                    let body = build_update_experience_request(update);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_UPDATE_EXPERIENCE, &events_tx);
                    });
                }
            }
            Command::RequestRegionExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REGION_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_REGION_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::SetRegionExperiences {
                allowed,
                blocked,
                trusted,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REGION_EXPERIENCES).cloned()
                {
                    let body = build_region_experiences_request(allowed, blocked, trusted);
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_REGION_EXPERIENCES, &events_tx);
                    });
                }
            }
            Command::OfferTeleport { targets, message } => {
                session.offer_teleport(targets, message, now).ok();
            }
            Command::AcceptTeleportLure { lure_id } => {
                session.accept_teleport_lure(*lure_id, now).ok();
            }
            Command::DeclineTeleportLure {
                from_agent_id,
                lure_id,
            } => {
                session
                    .decline_teleport_lure(*from_agent_id, *lure_id, now)
                    .ok();
            }
            Command::RequestTeleport {
                to_agent_id,
                message,
            } => {
                session.request_teleport(*to_agent_id, message, now).ok();
            }
            Command::GiveInventory {
                to_agent_id,
                item_id,
                asset_type,
                item_name,
                transaction_id,
            } => {
                session
                    .give_inventory(
                        *to_agent_id,
                        *item_id,
                        *asset_type,
                        item_name,
                        *transaction_id,
                        now,
                    )
                    .ok();
            }
            Command::GiveInventoryFolder {
                to_agent_id,
                folder_id,
                folder_name,
                transaction_id,
            } => {
                session
                    .give_inventory_folder(
                        *to_agent_id,
                        *folder_id,
                        folder_name,
                        *transaction_id,
                        now,
                    )
                    .ok();
            }
            Command::AcceptInventoryOffer { offer, folder_id } => {
                session.accept_inventory_offer(offer, *folder_id, now).ok();
            }
            Command::DeclineInventoryOffer {
                offer,
                trash_folder_id,
            } => {
                session
                    .decline_inventory_offer(offer, *trash_folder_id, now)
                    .ok();
            }
            Command::StartConference {
                session_id,
                invitees,
                message,
            } => {
                // The modern start is a `ChatSessionRequest` POST; the
                // deprecated `IM_SESSION_CONFERENCE_START` instant message is
                // the fallback for a grid without the cap (OpenSim). Either
                // way the session opens locally under the id we minted, and
                // the grid's `ChatterBoxSessionStartReply` moves it onto the
                // id the session really has.
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned()
                {
                    let body = chat_session_agents_body(
                        CHAT_SESSION_START_CONFERENCE,
                        session_id.get(),
                        invitees,
                    );
                    session.open_conference(*session_id, invitees, now);
                    std::thread::spawn(move || {
                        run_caps_oneway(&url, body);
                    });
                } else {
                    session
                        .start_conference(*session_id, invitees, message, now)
                        .ok();
                }
            }
            Command::InviteToChatSession {
                session_id,
                invitees,
            } => {
                // Adding to a session that already exists is the cap's own
                // `invite`; the legacy path has only the conference-start IM,
                // which a simulator without the cap treats as an add.
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned()
                {
                    let body =
                        chat_session_agents_body(CHAT_SESSION_INVITE, session_id.get(), invitees);
                    session.open_conference(*session_id, invitees, now);
                    std::thread::spawn(move || {
                        run_caps_oneway(&url, body);
                    });
                } else {
                    session
                        .start_conference(*session_id, invitees, "", now)
                        .ok();
                }
            }
            Command::SendConferenceMessage {
                session_id,
                message,
            } => {
                session
                    .send_conference_message(*session_id, message, now)
                    .ok();
                if let Some(own) = session.agent_id() {
                    let name = session.agent_legacy_name();
                    let roster: BTreeSet<_> = session
                        .participants(ChatSessionKind::Conference { id: *session_id })
                        .collect();
                    chat_log.log_conference(*session_id, &roster, own, &name, message);
                }
            }
            Command::LeaveConference { session_id } => {
                session.leave_conference(*session_id, now).ok();
            }
            Command::MarkSessionRead {
                session: chat_session,
            } => {
                session.mark_session_read(*chat_session);
            }
            Command::AcceptChatInvite {
                session_id,
                from_group,
            } => {
                // Promote the entry to joined locally, then drive the modern
                // accept over the cap when present (its reply roster seeds the
                // participants); without the cap the optimistic join suffices.
                session.accept_chat_invite(*session_id, *from_group, now);
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned()
                {
                    let body = chat_session_request_body(CHAT_SESSION_ACCEPT, session_id.get());
                    let events_tx = caps.events_tx.clone();
                    let (session_uuid, from_group) = (session_id.get(), *from_group);
                    std::thread::spawn(move || {
                        run_chat_session_request(&url, body, session_uuid, from_group, &events_tx);
                    });
                }
            }
            Command::DeclineChatInvite {
                session_id,
                from_group,
            } => {
                // Remove the entry, then refuse on the wire: the cap `decline
                // invitation` POST when present, else a UDP `SessionLeave`.
                session.decline_chat_invite(*session_id, *from_group, now);
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned()
                {
                    let body = chat_session_request_body(CHAT_SESSION_DECLINE, session_id.get());
                    let events_tx = caps.events_tx.clone();
                    let (session_uuid, from_group) = (session_id.get(), *from_group);
                    std::thread::spawn(move || {
                        run_chat_session_request(&url, body, session_uuid, from_group, &events_tx);
                    });
                } else if *from_group {
                    session
                        .leave_group_session(GroupKey::from(session_id.get()), now)
                        .ok();
                } else {
                    session.leave_conference(*session_id, now).ok();
                }
            }
            Command::JoinSessionVoice {
                session: chat_session,
            } => {
                // Optimistic local join, then drive the signalling: ensure a voice
                // account, then signal into the channel over `ChatSessionRequest`
                // (accept invitation). Signalling only — no audio.
                session.join_session_voice(*chat_session, now);
                if let (Some(own), Some(caps)) = (session.agent_id(), caps.as_ref()) {
                    let session_uuid = chat_session.canonical_session_id(own);
                    let from_group = matches!(chat_session, ChatSessionKind::Group { .. });
                    if let Some(url) = caps.map.get(CAP_PROVISION_VOICE_ACCOUNT).cloned() {
                        let body =
                            build_provision_voice_account_request(&VoiceProvisionRequest::vivox());
                        let events_tx = caps.events_tx.clone();
                        std::thread::spawn(move || {
                            run_voice_cap(&url, body, CAP_PROVISION_VOICE_ACCOUNT, &events_tx);
                        });
                    }
                    if let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                        let body = chat_session_request_body(CHAT_SESSION_ACCEPT, session_uuid);
                        let events_tx = caps.events_tx.clone();
                        std::thread::spawn(move || {
                            run_chat_session_request(
                                &url,
                                body,
                                session_uuid,
                                from_group,
                                &events_tx,
                            );
                        });
                    }
                }
            }
            Command::LeaveSessionVoice {
                session: chat_session,
            } => {
                // Optimistic local leave (keeps the text conversation), then signal
                // the voice decline on the wire: a 1:1 P2P call uses `decline p2p
                // voice`, a group / conference the multi-agent `decline invitation`.
                session.leave_session_voice(*chat_session);
                if let (Some(own), Some(caps)) = (session.agent_id(), caps.as_ref()) {
                    let session_uuid = chat_session.canonical_session_id(own);
                    let from_group = matches!(chat_session, ChatSessionKind::Group { .. });
                    let method = if matches!(chat_session, ChatSessionKind::Direct { .. }) {
                        CHAT_SESSION_DECLINE_P2P_VOICE
                    } else {
                        CHAT_SESSION_DECLINE
                    };
                    if let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                        let body = chat_session_request_body(method, session_uuid);
                        let events_tx = caps.events_tx.clone();
                        std::thread::spawn(move || {
                            run_chat_session_request(
                                &url,
                                body,
                                session_uuid,
                                from_group,
                                &events_tx,
                            );
                        });
                    }
                }
            }
            Command::FetchSessionHistory { kind } => {
                // Explicit server-backlog fetch, bypassing the auto-fetch gate.
                // Only group / conference sessions have a server backlog; on a
                // grid without the cap (stock OpenSim) there is nothing to POST
                // to, so the command silently degrades. Mirrors the tokio arm.
                if !matches!(kind, ChatSessionKind::Direct { .. })
                    && let (Some(own), Some(caps)) = (session.agent_id(), caps.as_ref())
                    && let Some(url) = caps.map.get(CAP_CHAT_SESSION_REQUEST).cloned()
                {
                    // Suppress a later duplicate auto-fetch of the same session.
                    session.note_server_history_requested(*kind);
                    let session_uuid = kind.canonical_session_id(own);
                    let body = chat_session_request_body(CHAT_SESSION_FETCH_HISTORY, session_uuid);
                    let from_group = matches!(kind, ChatSessionKind::Group { .. });
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_chat_session_fetch_history(
                            &url,
                            body,
                            session_uuid,
                            from_group,
                            &events_tx,
                        );
                    });
                }
            }
            Command::QueryChatSessions => {
                // Local query: build the light session list and surface it on the
                // event stream. (A bevy system may instead borrow the Session and
                // call `chat_sessions_info()` directly, skipping the round-trip.)
                outbound
                    .send(NetOutbound::Event(SessionEvent::ChatSessions(
                        session.chat_sessions_info().collect(),
                    )))
                    .ok();
            }
            Command::QueryChatHistoryPage {
                session: chat_session,
                before,
                limit,
            } => {
                // Newest-first paging across the unified memory→archive view: the
                // in-memory ring first, then older pages from the transcript (B9).
                let consumed = before.map_or(0, MessageCursor::consumed_count);
                let mem_len = session.history_len(*chat_session);
                let (messages, prev): (std::sync::Arc<[SessionMessage]>, _) = if consumed < mem_len
                {
                    let (page, mem_prev) = session.history_page(*chat_session, *before, *limit);
                    let collected: std::sync::Arc<[_]> = page.cloned().collect();
                    let next = consumed.saturating_add(collected.len());
                    let prev = mem_prev.or_else(|| {
                        chat_log
                            .read_older_page(*chat_session, mem_len, next, 1)
                            .filter(|(probe, _)| !probe.is_empty())
                            .map(|_more| MessageCursor::from_consumed(next))
                    });
                    (collected, prev)
                } else {
                    match chat_log.read_older_page(*chat_session, mem_len, consumed, *limit) {
                        Some((msgs, prev)) => (msgs.into(), prev),
                        None => (Vec::new().into(), None),
                    }
                };
                outbound
                    .send(NetOutbound::Event(SessionEvent::ChatHistoryPage {
                        session: *chat_session,
                        messages,
                        prev,
                    }))
                    .ok();
            }
            Command::QueryNearbyChatHistoryPage {
                already_shown,
                before,
                limit,
            } => {
                // Nearby chat has no in-memory ring: the whole page comes from the
                // on-disk transcript, skipping the newest `already_shown` lines the
                // caller already shows live (B9 paging discipline).
                let consumed = before.map_or(0, MessageCursor::consumed_count);
                let (lines, prev): (std::sync::Arc<[NearbyHistoryLine]>, _) =
                    match chat_log.read_nearby_older_page(*already_shown, consumed, *limit) {
                        Some((page, cursor)) => (page.into(), cursor),
                        None => (Vec::new().into(), None),
                    };
                outbound
                    .send(NetOutbound::Event(SessionEvent::NearbyChatHistoryPage {
                        lines,
                        prev,
                    }))
                    .ok();
            }
            Command::QueryInventoryFolder {
                folder,
                before,
                limit,
            } => {
                // Local query: page the held model into owning view types (one
                // bounded borrow→owned transform, `Arc<[…]>` payload). A bevy
                // system may instead borrow the Session and call
                // `inventory_folder_page` directly, skipping the round-trip.
                let (folders, items, prev) =
                    session.inventory_folder_page(*folder, *before, *limit);
                // On-demand: a query for an unfetched folder schedules its fetch
                // (works regardless of the background-crawl flag).
                if session.folder_fetch_state(*folder) == Some(FolderState::Unknown) {
                    fetch_folder_contents(&mut session, *folder, caps.as_ref(), now);
                }
                outbound
                    .send(NetOutbound::Event(SessionEvent::InventoryFolderPage {
                        folder: *folder,
                        folders: folders.into(),
                        items: items.into(),
                        prev,
                    }))
                    .ok();
            }
            Command::QueryInventoryRoots => {
                // Local query: surface the agent + library roots (both `Copy`).
                outbound
                    .send(NetOutbound::Event(SessionEvent::InventoryRoots {
                        agent_root: session.inventory_root(),
                        library_root: session.library_root(),
                    }))
                    .ok();
            }
            Command::QueryInventoryFolders => {
                // Local query: snapshot the agent tree's known folders (seeded
                // from the login skeleton, so present before any contents fetch).
                outbound
                    .send(NetOutbound::Event(SessionEvent::InventoryFolders(
                        session.inventory_folder_infos().into(),
                    )))
                    .ok();
            }
            Command::QueryFriends => {
                // Local query: build the buddy snapshot with online flags.
                outbound
                    .send(NetOutbound::Event(SessionEvent::FriendsSnapshot(
                        session.friends_presence().collect(),
                    )))
                    .ok();
            }
            Command::RetrieveInstantMessages => {
                session.retrieve_instant_messages(now).ok();
            }
            Command::RequestOfflineMessages => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_READ_OFFLINE_MSGS).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_READ_OFFLINE_MSGS, &events_tx);
                    });
                }
            }
            Command::TeleportViaLandmark { landmark } => {
                session.teleport_via_landmark(*landmark, now).ok();
            }
            Command::CancelTeleport => {
                session.cancel_teleport(now).ok();
            }
            Command::SetStartLocation {
                slot,
                position,
                look_at,
            } => {
                session
                    .set_start_location(*slot, *position, look_at.clone(), now)
                    .ok();
            }
            Command::RequestAgentDataUpdate => {
                session.request_agent_data_update(now).ok();
            }
            Command::QuitCopy => {
                session.quit_copy(now).ok();
            }
            Command::SetVelocityInterpolation { enabled } => {
                session.set_velocity_interpolation(*enabled, now).ok();
            }
            Command::RequestUserInfo => {
                // Cap-preferred (the modern `UserInfo` GET), falling back to
                // the legacy `UserInfoRequest` UDP message where the region
                // does not serve the capability (OpenSim).
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_USER_INFO).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_USER_INFO, &events_tx);
                    });
                } else {
                    session.request_user_info(now).ok();
                }
            }
            Command::UpdateUserInfo {
                im_via_email,
                directory_visibility,
            } => {
                // Cap-preferred (the modern `UserInfo` POST), falling back to
                // the legacy `UpdateUserInfo` UDP message where the region
                // does not serve the capability (OpenSim). `im_via_email` is
                // always included: OpenSim needs it and Second Life ignores
                // unknown keys (it manages the forwarding preference on the
                // account website).
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_USER_INFO).cloned()
                {
                    let body = build_user_info_update(&UserInfoUpdate {
                        im_via_email: Some(*im_via_email),
                        dir_visibility: directory_visibility.to_wire().to_owned(),
                    });
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_USER_INFO, &events_tx);
                    });
                } else {
                    session
                        .update_user_info(*im_via_email, *directory_visibility, now)
                        .ok();
                }
            }
            Command::SetChatLogConfig(config) => {
                chat_log.set_config((**config).clone());
            }
            Command::TriggerSound {
                sound,
                gain,
                region_handle,
                position,
            } => {
                session
                    .trigger_sound(*sound, *gain, *region_handle, *position, now)
                    .ok();
            }
            Command::RequestGodlikePowers { godlike } => {
                session.request_godlike_powers(*godlike, now).ok();
            }
            Command::EjectUser { target, action } => {
                session.eject_user(*target, *action, now).ok();
            }
            Command::FreezeUser { target, action } => {
                session.freeze_user(*target, *action, now).ok();
            }
            Command::SimWideDeletes { owner, flags } => {
                session.sim_wide_deletes(*owner, *flags, now).ok();
            }
            Command::GodUpdateRegionInfo { update } => {
                session.god_update_region_info(update, now).ok();
            }
            Command::ParcelGodForceOwner { parcel, owner } => {
                session.parcel_god_force_owner(*parcel, *owner, now).ok();
            }
            Command::ParcelGodMarkAsContent { parcel } => {
                session.parcel_god_mark_as_content(*parcel, now).ok();
            }
            Command::EventGodDelete {
                event,
                query_id,
                query_text,
                flags,
                query_start,
            } => {
                session
                    .event_god_delete(*event, *query_id, query_text, *flags, *query_start, now)
                    .ok();
            }
            Command::StateSave { filename } => {
                session.state_save(filename, now).ok();
            }
            Command::ViewerStartAuction { parcel, snapshot } => {
                session.viewer_start_auction(*parcel, *snapshot, now).ok();
            }
            Command::MarketplaceMerchantStatus => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::MerchantStatus,
                        Ok(merchant_status_request()),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceListings => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::GetListings,
                        Ok(listings_request()),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceListing(id) => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::GetListing(*id),
                        Ok(listing_request(*id)),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceCreateListing(payload) => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::CreateListing,
                        create_listing_request(payload),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceUpdateListing(payload) => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::UpdateListing(payload.id),
                        update_listing_request(payload),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceAssociateListing(payload) => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::AssociateInventory(payload.id),
                        associate_inventory_request(payload),
                        &caps.asset_tx,
                    );
                }
            }
            Command::MarketplaceDeleteListing(id) => {
                if let Some(caps) = caps.as_ref() {
                    dispatch_marketplace_request(
                        caps.map.get(CAP_DIRECT_DELIVERY).cloned(),
                        MarketplaceOperation::DeleteListing(*id),
                        Ok(delete_listing_request(*id)),
                        &caps.asset_tx,
                    );
                }
            }
            Command::Logout => session.initiate_logout(now),
        }
    }

    // Fire timers that are due.
    if session
        .poll_timeout()
        .is_some_and(|deadline| now >= deadline)
    {
        session.handle_timeout(now);
    }

    // Flush outgoing datagrams.
    while let Some(transmit) = session.poll_transmit() {
        socket.send_to(&transmit.payload, transmit.destination).ok();
    }

    // Surface protocol diagnostics the session collected this frame (decode
    // failures, unhandled messages, unknown CAPS events, missing replies). Only
    // populated while diagnostics are enabled.
    while let Some(diagnostic) = session.poll_diagnostic() {
        outbound.send(NetOutbound::Diagnostic(diagnostic)).ok();
    }

    // Surface events. A region change brings a new seed capability, so restart
    // the event-queue poller against the new region (dropping the old poller
    // signals its thread to stop).
    let mut done = false;
    let mut region_changed = false;
    while let Some(event) = session.poll_event() {
        match &event {
            SessionEvent::Disconnected(_) | SessionEvent::LoggedOut => done = true,
            SessionEvent::RegionChanged { .. } => region_changed = true,
            // POST a neighbour's seed capability so the simulator streams that
            // region's scene to the child circuit (its `SendInitialData` is gated
            // on the seed having been requested). One-shot, off the ECS thread.
            SessionEvent::NeighborSeed {
                seed_capability, ..
            } => post_neighbour_seed(seed_capability.clone()),
            // On the login inventory/library skeleton, load the disk cache (if
            // any) and reconcile it against the skeleton, so version-matching
            // folders skip the background refetch. A no-op when disabled.
            SessionEvent::InventorySkeleton(folders) => {
                inventory_cache.load_agent(&mut session, folders);
            }
            SessionEvent::LibraryInventory(folders) => {
                inventory_cache.load_library(&mut session, folders);
            }
            // On a `SimulatorFeatures` reply carrying a not-yet-resolved syntax
            // id, load the cached `LSLSyntax` document (forwarded over `events_tx`
            // for the uniform `handle_caps_event` decode) or fetch it from the
            // `LSLSyntax` cap on a worker thread. Unchanged / absent ids do
            // nothing.
            SessionEvent::SimulatorFeatures(features) => {
                if let Some(id) = features.lsl_syntax_id
                    && lsl_syntax.last_id != Some(id)
                    && let Some(caps) = caps.as_ref()
                {
                    lsl_syntax.last_id = Some(id);
                    if let Some(cached) = lsl_syntax.cache.load(id) {
                        caps.events_tx
                            .send((CAP_LSL_SYNTAX.to_owned(), cached))
                            .ok();
                    } else if let Some(url) = caps.map.get(CAP_LSL_SYNTAX).cloned() {
                        let events_tx = caps.events_tx.clone();
                        let cache = lsl_syntax.cache.clone();
                        std::thread::spawn(move || {
                            run_fetch_lsl_syntax(&url, id, &cache, &events_tx);
                        });
                    }
                }
            }
            // The decoded grid LSL library arrived (fresh fetch or cache hit):
            // log a one-line confirmation for live verification.
            SessionEvent::LslSyntax(syntax) => {
                tracing::info!(
                    symbols = syntax.len(),
                    functions = syntax.functions.len(),
                    constants = syntax.constants.len(),
                    events = syntax.events.len(),
                    "loaded grid LSL syntax definition",
                );
            }
            _ => {}
        }
        // Tap the event for the local chat log (no-op when disabled) before
        // forwarding it on.
        if chat_log.any_enabled() {
            chat_log.observe_event(&session, &event);
        }
        outbound.send(NetOutbound::Event(event)).ok();
    }
    if region_changed {
        // Re-target the single event-queue worker at the new root region rather
        // than dropping + respawning a poller (which, under rapid region changes,
        // left two pollers racing on one region's ack-sequenced queue and lost
        // events). If the worker was never started (no seed at login yet), start
        // it now.
        match caps.as_mut() {
            Some(existing) => existing.switch_to(&session),
            None => caps = start_caps(&session),
        }
    }

    if done || session.is_closed() {
        // Persist the inventory cache before exit (Firestorm's save-at-cleanup);
        // a no-op when the cache is disabled.
        inventory_cache.save(&mut session);
        return None;
    }
    // The optional dirty/idle inventory-cache save (crash-safety beyond
    // Firestorm's shutdown-only save); self-gating on the dirty flag and the
    // save interval, so a clean or disabled cache costs nothing.
    inventory_cache.maybe_save(&mut session, now);
    // Mirror the agent's parcel / fly / seat for the ECS side (e.g. the
    // viewer's take-off gate), sending only when something changed so a quiet
    // tick crosses the channel with nothing.
    let mut refreshed = agent_parcel.clone();
    refreshed.refresh_from(&session);
    if refreshed != agent_parcel {
        agent_parcel = refreshed;
        outbound
            .send(NetOutbound::AgentParcel(Box::new(agent_parcel.clone())))
            .ok();
    }
    Some(RunningSession {
        session,
        socket,
        recv_buf,
        caps,
        chat_log,
        inventory_cache,
        lsl_syntax,
        agent_parcel,
    })
}
