#![doc = include_str!("../README.md")]

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use bevy::prelude::*;
use reqwest::blocking::Client as ReqwestBlockingClient;

use std::collections::{BTreeSet, HashMap};

use sl_proto::{
    CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES, CAP_ATTACHMENT_RESOURCES,
    CAP_CHAT_SESSION_REQUEST, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_OBJECT_COST, CAP_GET_OBJECT_PHYSICS_DATA,
    CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_LAND_RESOURCES, CAP_MODIFY_MATERIAL_PARAMS,
    CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS,
    CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_RESOURCE_COST_SELECTED, CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    CAP_SIMULATOR_FEATURES, CAP_UPDATE_EXPERIENCE, CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK,
    CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE,
    CHAT_SESSION_DECLINE_P2P_VOICE, ChatSessionKind, Event as SessionEvent, GroupKey,
    INVENTORY_FETCH_MAX_IN_FLIGHT, Llsd, LoginResponse, MessageCursor, RECV_BUFFER_SIZE,
    SelectedCostKind, Session, SessionMessage, ais_category_children_fetch_url,
    ais_category_children_url, ais_category_url, ais_create_category_url, ais_item_url,
    build_agent_preferences_request, build_ais_create_category_body, build_ais_move_body,
    build_ais_rename_category_body, build_ais_update_item_body,
    build_create_inventory_category_request, build_get_object_cost_request,
    build_get_object_physics_data_request, build_modify_material_params_request,
    build_object_media_navigate_request, build_object_media_update_request,
    build_parcel_voice_info_request, build_provision_voice_account_request,
    build_region_experiences_request, build_remote_parcel_request,
    build_resource_cost_selected_request, build_send_user_report,
    build_set_experience_permission_request, build_update_experience_request,
    build_update_item_asset_request, build_update_script_agent_request,
    build_update_script_task_request, build_upload_baked_texture_request,
    build_voice_signaling_request, chat_session_request_body, display_names_query,
    experience_id_query, experience_info_query, find_experience_query, forget_experience_query,
    group_experiences_query, parse_login_response,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    ActiveGroup, AgentKey, AgentOrObjectKey, AgentPreferences, AnimatedObjects, AnimationKey,
    AnyMessage, AssetKey, AttachmentMode, AttachmentPoint, AvatarAppearance, AvatarClassified,
    AvatarGroupMembership, AvatarInterests, AvatarName, AvatarPick, AvatarProperties, Camera,
    CameraError, ChatAudible, ChatChannel, ChatLogConfig, ChatMessage, ChatSource, ChatSourceType,
    ChatType, ChatTypeNotAVolume, Child, CircuitCode, CircuitId, ClassifiedCategory,
    ClassifiedInfo, ClassifiedKey, ClassifiedUpdate, ClickAction, ClientDirectories, ClockStyle,
    CoarseLocation, Command, ControlFlags, ConversationKind, CreateGroupParams, DeRezDestination,
    DetachOrder, Diagnostic, Direction, DisconnectReason, DisplayName, DisplayNameUpdate, Distance,
    EconomyData, EnvironmentSettings, EstateAccessDelta, EstateAccessKind, EstateCovenant,
    EstateInfo, ExperienceInfo, ExperiencePermission, ExperienceProperties, ExperienceUpdate,
    ExtendedMesh, FlexibleData, FolderInfo, FolderState, FolderType, Friend, FriendRights,
    GlobalCoordinates, GltfMaterialOverride, GridCoordinates, GroupMember, GroupMembership,
    GroupNotice, GroupNoticeAttachment, GroupNoticeKey, GroupProfile, GroupRequestId, GroupRole,
    GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember, GroupRoleMemberChange,
    GroupRoleUpdateType, GroupTitle, HomeLocation, IceCandidate, ImDialog, ImSessionId,
    InstantMessage, InterestsUpdate, InventoryCacheConfig, InventoryCallbackId, InventoryCursor,
    InventoryFolder, InventoryFolderKey, InventoryItem, InventoryItemOrFolderKey, InventoryKey,
    InventoryOffer, InventoryOwner, InventoryType, ItemInfo, Key, Kilobits, LandArea, LandingType,
    LegacyMaterial, LightData, LightImage, LindenAmount, LindenBalance, LoadUrlRequest,
    LoggedChatType, LoginAccount, LoginFailure, LoginParams, LoginRejectKind, LoginRequest, LureId,
    MAX_FACES, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP, MEDIA_PERM_NONE,
    MEDIA_PERM_OWNER, MapItem, MapItemType, MapRegionInfo, Material, MaterialOverrideUpdate,
    Maturity, MediaEntry, MeshKey, MfaChallenge, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MovementMode, MuteEntry, MuteFlags, MuteType, NegativeBalanceError,
    NeighborInfo, NewInventoryItem, NewInventoryLink, Object, ObjectExtraParams,
    ObjectFlagSettings, ObjectMediaResponse, ObjectMotion, ObjectPermMasks, ObjectProperties,
    ObjectPropertiesFamily, ObjectTransform, OpenRegionInfo, OpenSimExtras, OwnerKey,
    ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory, ParcelDetails,
    ParcelFlags, ParcelInfo, ParcelKey, ParcelMediaCommand, ParcelMediaUpdateInfo,
    ParcelObjectOwner, ParcelOverlayInfo, ParcelRequestResult, ParcelReturnType, ParcelStatus,
    ParcelUpdate, ParcelVoiceInfo, ParticleSystem, PermissionField, Permissions, Permissions5,
    PhysicsShapeTypes, PickInfo, PickKey, PickUpdate, PingId, PlayingAnimation, PrimShape,
    PrimShapeParams, ProductType, ProfileUpdate, ProposalCandidateId, ProposalVoteId, QueryId,
    ReflectionProbe, ReflectionProbeFlags, RegionChatSettings, RegionCombatSettings,
    RegionCoordinates, RegionFlags, RegionHandle, RegionIdentity, RegionInfoUpdate, RegionLimits,
    RegionLocalObjectId, RegionLocalParcelId, RegionName, RegionTerrainComposition, Reliability,
    RenderMaterialEntry, RenderMaterialRef, RestoreItem, RezAttachment, RezObjectParams, Rotation,
    SaleType, ScopedObjectId, ScopedParcelId, ScriptCompileError, ScriptControl,
    ScriptControlAction, ScriptDialog, ScriptLanguage, ScriptPermissionRequest, ScriptPermissions,
    ScriptTarget, ScriptTeleportRequest, ScriptUploadLocation, SculptData, SculptOrMeshKey,
    SequenceNumber, SetDisplayNameReply, SimulatorFeatures, SoundFlags, SoundPreload,
    StartLocation, StartLocationParseError, TaskInventoryItem, TaskInventoryKey,
    TaskInventoryReply, TerrainLayerType, TerrainPatch, TextureAnimation, TextureEntry,
    TextureFace, TextureKey, Throttle, ThrottleBuilder, ThrottleError, TimestampFormat,
    TransactionId, TransferId, Transmit, UpdatableAssetType, Uuid, Vector, VoiceAccountInfo,
    VoiceProvisionRequest, Wearable, WearableType, XferId, avatar_texture, decode_particle_system,
    decode_texture_anim, decode_texture_entry, encode_texture_entry, grid_to_handle, group_powers,
    handle_to_global, handle_to_grid, particle_pattern, pcode, sim_access, texture_anim_mode,
};
#[doc(no_inline)]
pub use sl_proto::{Asset, AssetType, ImageCodec, Texture, TransferStatus};
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
pub use sl_proto::CAP_VIEWER_ASSET;
pub use sl_proto::{DisconnectReason as SessionDisconnectReason, Event as SlSessionEvent};
// The decoding, LOD-aware texture store, re-exported so a Bevy app can build and
// drive one (`sl_texture::TextureEntry`/`TextureReadLease` stay accessible as
// `sl_texture::…` to avoid colliding with `sl_proto`'s prim-face `TextureEntry`).
pub use sl_proto::DiscardLevel;
pub use sl_texture::{
    AssetFetcher, CacheLimits, DecodedImage as DecodedTexture, FetchChunk, Priority, TextureError,
    TextureFetcher, TextureProgress, TextureRequest, TextureStore,
};
// The decoding, LOD-aware mesh store (the mesh counterpart of the texture
// store). `Priority` and `MeshKey` are already re-exported (from `sl_texture` /
// `sl_proto`); the mesh `CacheLimits` is aliased so it does not collide with the
// texture one.
pub use sl_mesh::{
    CacheLimits as MeshCacheLimits, DecodedMesh, MeshEntry, MeshError, MeshFetcher, MeshLod,
    MeshPhysics, MeshProgress, MeshReadLease, MeshRequest, MeshSkin, MeshStore, Submesh,
};

// The generic-asset store (the opaque-blob counterpart of the texture/mesh
// stores), fetched whole over the `ViewerAsset` capability. Its `CacheLimits` is
// aliased so it does not collide with the texture/mesh ones; `Priority`,
// `AssetKey`, and `AssetType` are already re-exported.
pub use sl_asset::{
    AssetEntry, AssetError, AssetProgress, AssetRef, AssetStore, BlobFetcher,
    CacheLimits as AssetCacheLimits,
};

// The pure prim-tessellation geometry (no store/fetcher — a prim is tessellated
// on the CPU from its shape parameters, not fetched). Re-exported so the viewer
// can dequantize a `PrimShapeParams` into a float shape, tessellate it at a
// `PrimLod`, and feed the resulting faces through `to_bevy_prim_mesh`. The
// dequantized float shape is aliased `PrimShapeFloat` so it does not collide
// with `sl_proto`'s quantized rez-params `PrimShape`.
pub use sl_prim::{
    HoleType, PathCurve, PrimFace, PrimFaceId, PrimLod, PrimMesh, PrimShape as PrimShapeFloat,
    ProfileCurve, tessellate,
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

// The pure system-avatar decoders (skeleton / base body / visual params), the
// avatar counterpart of `sl_mesh` / `sl_texture`. Re-exported so the viewer can
// parse the standard Linden `character/` assets and drive them through
// [`to_bevy_base_mesh`] / [`BevySkeleton`]. `AttachmentPoint` is already
// re-exported (from `sl_proto`, which `sl_avatar` re-exports too).
pub use sl_avatar::{
    AppearanceValues, AttachmentPointDef, AttachmentPoints, BaseMesh, BaseMeshError, BoneDeform,
    CollisionVolume, ColorOp, ColorRamp, Joint, MaskTexture, MorphMask, MorphMasks, MorphWeights,
    MorphedMesh, ParamError, PartMorphMask, ResolvedParams, SkeletalDeformations, Skeleton,
    SkeletonError, VisualParam, VisualParams, WearableAsset, WearableError, combine_layer_color,
    global_color, global_color_params,
};

// The client-side avatar baker (`sl-bake`, the OpenSim / legacy path): compose a
// bake region from ordered wearable layers, and the per-region layer plan (P15).
pub use sl_bake::{
    BakeRegion, BakedImage, Layer, LayerKind, LayerTint, PlannedLayer, TexGen, composite_region,
    region_layers, region_plan,
};

pub use crate::assets::BevyAssetFetcher;
pub use crate::avatars::{BaseMeshSkin, BevySkeleton, to_bevy_base_mesh, to_bevy_morphed_mesh};
pub use crate::meshes::{BevyMeshFetcher, to_bevy_mesh, to_bevy_meshes};
pub use crate::prims::{to_bevy_prim_mesh, to_bevy_prim_meshes};
#[cfg(feature = "bevy_pbr")]
pub use crate::terrain::{ATTRIBUTE_TERRAIN_WEIGHTS, TerrainMaterial, TerrainMaterialPlugin};
pub use crate::textures::{BevyTextureFetcher, to_bevy_image};

pub mod assets;
pub mod avatars;
mod caps;
mod chat_log;
mod experiences;
mod fetch;
mod http;
mod inventory;
mod inventory_cache;
mod materials;
mod media;
pub mod meshes;
pub mod prims;
#[cfg(feature = "bevy_pbr")]
pub mod terrain;
pub mod textures;
mod upload;
mod voice;
mod world;
use crate::caps::{CAPS_FAILURE_PREFIX, post_neighbour_seed, start_caps};
use crate::chat_log::ChatLog;
use crate::experiences::{run_experience_status, run_group_experiences};
use crate::fetch::{emit_disconnect, run_asset_fetch, run_generic_asset_fetch, run_texture_fetch};
use crate::http::{
    run_caps_oneway, run_chat_session_request, run_delete_caps_llsd, run_get_caps_llsd,
    run_land_resources, run_patch_caps_llsd, run_put_caps_llsd,
};
use crate::inventory::{
    fetch_folder_contents, run_group_members_fetch, run_inventory_fetch,
    run_server_appearance_update,
};
use crate::inventory_cache::InventoryCache;
use crate::materials::{run_modify_material_params, run_render_materials_fetch};
use crate::media::{run_object_media_fetch, run_object_media_post};
use crate::upload::{
    emit_upload_failure, emit_upload_unavailable, run_caps_upload, run_report_screenshot_upload,
    run_script_upload, spawn_new_file_upload,
};
use crate::voice::{run_voice_cap, run_voice_signaling};
use crate::world::{SlRegionIndex, maintain_world};

pub use crate::world::{
    SlCurrentRegion, SlIdentity, SlNeighbor, SlParcel, SlRegion, SlRegionIdentity, SlRegionLimits,
};

/// How long to wait for a single CAPS event-queue long-poll before retrying.
const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// The Bevy plugin that drives a sans-I/O [`Session`] from ECS systems.
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
                inventory_cache_config: self.inventory_cache_config,
                background_inventory_fetch: self.background_inventory_fetch,
            })
            .init_resource::<SlIdentity>()
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
    /// The inventory disk-cache configuration (default off).
    inventory_cache_config: InventoryCacheConfig,
    /// Whether the automatic background inventory crawl is enabled (default off).
    background_inventory_fetch: bool,
}

/// The driver's runtime state resource.
#[derive(Resource)]
struct SlState {
    /// The current phase of the driver.
    inner: SlInner,
}

/// The driver phases.
enum SlInner {
    /// Awaiting the result of the (threaded, blocking) XML-RPC login.
    LoggingIn {
        /// The session whose circuit will be bootstrapped on success.
        session: Box<Session>,
        /// Receives the login response body (or an error string).
        rx: Receiver<Result<String, String>>,
    },
    /// The circuit is up; pumping the socket each frame.
    Running {
        /// The driven session.
        session: Box<Session>,
        /// The non-blocking UDP socket.
        socket: UdpSocket,
        /// A reusable receive buffer.
        recv_buf: Vec<u8>,
        /// The CAPS subsystem for the current region, if a seed capability is
        /// known. Restarted on each region change.
        caps: Option<Caps>,
        /// The local chat-log writer/reader (a no-op when disabled).
        chat_log: Box<ChatLog>,
        /// The inventory disk-cache reader/writer (a no-op when disabled).
        inventory_cache: Box<InventoryCache>,
    },
    /// The session is finished.
    Done,
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
    /// Receives the region's capability map once the poller has fetched it.
    pub(crate) map_rx: Receiver<HashMap<String, String>>,
    /// The cached capability map (cap name → URL), empty until discovered.
    pub(crate) map: HashMap<String, String>,
    /// Set on drop to ask the poller thread to stop at its next loop iteration.
    pub(crate) stop: Arc<AtomicBool>,
}

impl Drop for Caps {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Startup system: builds the session and spawns the blocking login thread.
fn start_login(mut commands: Commands, config: Res<SlConfig>) {
    let mut session = Session::new(config.params.clone());
    session.set_diagnostics(config.diagnostics);
    session.set_background_inventory_fetch(config.background_inventory_fetch);
    let inner = match session.login_http_request() {
        Some(request) => {
            let (tx, rx) = unbounded();
            std::thread::spawn(move || {
                tx.send(perform_login(
                    request.url.as_str(),
                    &request.user_agent,
                    request.body,
                ))
                .ok();
            });
            SlInner::LoggingIn {
                session: Box::new(session),
                rx,
            }
        }
        None => SlInner::Done,
    };
    commands.insert_resource(SlState { inner });
}

/// Performs the blocking XML-RPC login POST, returning the response body.
fn perform_login(url: &str, user_agent: &str, body: String) -> Result<String, String> {
    ReqwestBlockingClient::new()
        .post(url)
        .header("Content-Type", "text/xml")
        .header("User-Agent", user_agent)
        .body(body)
        .send()
        .and_then(reqwest::blocking::Response::text)
        .map_err(|error| error.to_string())
}

/// Update system: advances the session each frame.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected ECS resources and event \
              writers/readers; the chat-log config is one more such resource"
)]
fn drive(
    mut state: ResMut<SlState>,
    config: Res<SlConfig>,
    mut events: MessageWriter<SlEvent>,
    mut diagnostics: MessageWriter<SlDiagnostic>,
    mut capabilities: MessageWriter<SlCapabilities>,
    mut identity: ResMut<SlIdentity>,
    mut mfa: MessageWriter<SlMfaChallenge>,
    mut rejected: MessageWriter<SlLoginRejected>,
    mut commands: MessageReader<SlCommand>,
) {
    let now = Instant::now();
    let inner = std::mem::replace(&mut state.inner, SlInner::Done);
    state.inner = match inner {
        SlInner::LoggingIn { session, rx } => advance_login(
            session,
            rx,
            &config.chat_log_config,
            &config.directories,
            &config.inventory_cache_config,
            now,
            &mut events,
            &mut identity,
            &mut mfa,
            &mut rejected,
        ),
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
            chat_log,
            inventory_cache,
        } => advance_running(
            session,
            socket,
            recv_buf,
            caps,
            chat_log,
            inventory_cache,
            now,
            &mut events,
            &mut diagnostics,
            &mut capabilities,
            &mut commands,
        ),
        SlInner::Done => SlInner::Done,
    };
}

/// Handles the logging-in phase, transitioning to `Running` once the login
/// response arrives.
#[expect(
    clippy::too_many_arguments,
    reason = "the login step threads the session, its channel, the chat-log and \
              inventory-cache configs and directories, and several Bevy writers it \
              emits to on success"
)]
fn advance_login(
    mut session: Box<Session>,
    rx: Receiver<Result<String, String>>,
    chat_log_config: &ChatLogConfig,
    directories: &ClientDirectories,
    inventory_cache_config: &InventoryCacheConfig,
    now: Instant,
    events: &mut MessageWriter<SlEvent>,
    identity: &mut SlIdentity,
    mfa: &mut MessageWriter<SlMfaChallenge>,
    rejected: &mut MessageWriter<SlLoginRejected>,
) -> SlInner {
    match rx.try_recv() {
        Ok(Ok(body)) => match parse_login_response(&body) {
            Ok(LoginResponse::Success(success)) => {
                if session
                    .handle_login_response(LoginResponse::Success(success), now)
                    .is_err()
                {
                    emit_disconnect(events, DisconnectReason::ProtocolError);
                    return SlInner::Done;
                }
                match bind_socket() {
                    Ok(socket) => {
                        *identity = SlIdentity {
                            agent_id: session.agent_id(),
                            session_id: session.session_id(),
                            circuit_code: session.circuit_code(),
                            seed_capability: session.seed_capability().cloned(),
                            region_handle: session.region_handle(),
                            circuit_id: session.root_circuit_id(),
                        };
                        let caps = start_caps(&session);
                        let chat_log = Box::new(ChatLog::new(
                            chat_log_config.clone(),
                            directories.agent_chat_log_dir.clone(),
                            session.agent_legacy_name(),
                            session.agent_id(),
                        ));
                        let inventory_cache = Box::new(InventoryCache::new(
                            *inventory_cache_config,
                            directories.agent_cache_dir.clone(),
                            session.agent_id(),
                            now,
                        ));
                        SlInner::Running {
                            session,
                            socket,
                            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
                            caps,
                            chat_log,
                            inventory_cache,
                        }
                    }
                    Err(()) => {
                        emit_disconnect(events, DisconnectReason::ProtocolError);
                        SlInner::Done
                    }
                }
            }
            Ok(LoginResponse::MfaChallenge(challenge)) => {
                mfa.write(SlMfaChallenge(challenge));
                SlInner::Done
            }
            Ok(LoginResponse::Failure(failure)) => {
                // A retryable "already logged in" rejection is surfaced like an
                // MFA challenge — its own event the driver can act on (consult
                // the user, re-add the plugin) — rather than a fatal disconnect.
                if failure.kind() == LoginRejectKind::AlreadyLoggedIn {
                    rejected.write(SlLoginRejected(failure));
                } else {
                    emit_disconnect(
                        events,
                        DisconnectReason::LoginFailed {
                            reason: failure.reason,
                            message: failure.message,
                        },
                    );
                }
                SlInner::Done
            }
            Err(_parse) => {
                emit_disconnect(events, DisconnectReason::ProtocolError);
                SlInner::Done
            }
        },
        Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
            emit_disconnect(events, DisconnectReason::ProtocolError);
            SlInner::Done
        }
        Err(TryRecvError::Empty) => SlInner::LoggingIn { session, rx },
    }
}

/// Binds a non-blocking UDP socket on an ephemeral port.
fn bind_socket() -> Result<UdpSocket, ()> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_ignored| ())?;
    socket.set_nonblocking(true).map_err(|_ignored| ())?;
    Ok(socket)
}

/// Handles the running phase: receive UDP and CAPS events, apply commands, time
/// out, transmit, and surface events and diagnostics.
#[expect(
    clippy::too_many_arguments,
    reason = "the ECS driver fans the session's output to several Bevy writers \
              (events, diagnostics) alongside its state"
)]
fn advance_running(
    mut session: Box<Session>,
    socket: UdpSocket,
    mut recv_buf: Vec<u8>,
    mut caps: Option<Caps>,
    mut chat_log: Box<ChatLog>,
    mut inventory_cache: Box<InventoryCache>,
    now: Instant,
    events: &mut MessageWriter<SlEvent>,
    diagnostics: &mut MessageWriter<SlDiagnostic>,
    capabilities: &mut MessageWriter<SlCapabilities>,
    commands: &mut MessageReader<SlCommand>,
) -> SlInner {
    // Drain all available inbound datagrams.
    loop {
        match socket.recv_from(&mut recv_buf) {
            Ok((len, from)) => {
                if let Some(datagram) = recv_buf.get(..len) {
                    session.handle_datagram(from, datagram, now).ok();
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(_other) => break,
        }
    }

    // Cache the capability map once the poller discovers it, then drain any CAPS
    // payloads (event-queue events plus inventory responses).
    if let Some(caps) = caps.as_mut() {
        while let Ok(map) = caps.map_rx.try_recv() {
            // The viewer fetches `SimulatorFeatures` on arriving in a region, so
            // GET it once the capability map is known (at login and on each region
            // change), surfacing the flags as `Event::SimulatorFeatures`.
            if let Some(url) = map.get(CAP_SIMULATOR_FEATURES).cloned() {
                let events_tx = caps.events_tx.clone();
                std::thread::spawn(move || {
                    run_get_caps_llsd(&url, CAP_SIMULATOR_FEATURES, &events_tx);
                });
            }
            capabilities.write(SlCapabilities(map.clone()));
            caps.map = map;
        }
        while let Ok((message, body)) = caps.events_rx.try_recv() {
            // A CAPS helper reports a failed request by sending the failure
            // sentinel rather than a decoded reply; surface it as a diagnostic
            // instead of feeding the session.
            if let Some(cap) = message.strip_prefix(CAPS_FAILURE_PREFIX) {
                tracing::warn!(capability = cap, "CAPS request failed; no reply surfaced");
                if session.diagnostics_enabled() {
                    diagnostics.write(SlDiagnostic(Diagnostic::ExpectedReplyMissing {
                        request: cap.to_owned(),
                        sequence: None,
                    }));
                }
            } else {
                session.handle_caps_event(&message, &body, now).ok();
            }
        }
        // Binary asset fetches return fully-formed session events; surface them.
        while let Ok(event) = caps.asset_rx.try_recv() {
            events.write(SlEvent(event));
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
    }

    // Apply queued commands.
    for command in commands.read() {
        match &command.0 {
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
                session
                    .move_inventory_folder(*folder_id, *parent_id, now)
                    .ok();
            }
            Command::RemoveInventoryFolders(folder_ids) => {
                session.remove_inventory_folders(folder_ids, now).ok();
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
                session.link_inventory_item(new, now).ok();
            }
            Command::UpdateInventoryItem {
                item,
                transaction_id,
            } => {
                session
                    .update_inventory_item(item, *transaction_id, now)
                    .ok();
            }
            Command::MoveInventoryItem {
                item_id,
                folder_id,
                new_name,
            } => {
                session
                    .move_inventory_item(*item_id, *folder_id, new_name, now)
                    .ok();
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
                session.remove_inventory_items(item_ids, now).ok();
            }
            Command::ChangeInventoryItemFlags { item_id, flags } => {
                session
                    .change_inventory_item_flags(*item_id, *flags, now)
                    .ok();
            }
            Command::PurgeInventoryDescendents(folder_id) => {
                session.purge_inventory_descendents(*folder_id, now).ok();
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
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_EXT_ENVIRONMENT).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let url = format!("{base}?parcelid={}", parcel_id.unwrap_or(-1));
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_EXT_ENVIRONMENT, &events_tx);
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
            Command::RequestObjectProperties { local_ids } => {
                session.request_object_properties(local_ids, now).ok();
            }
            Command::DeselectObjects { local_ids } => {
                session.deselect_objects(local_ids, now).ok();
            }
            Command::TouchObject { local_id } => {
                session.touch_object(*local_id, now).ok();
            }
            Command::GrabObject {
                local_id,
                grab_offset,
            } => {
                session
                    .grab_object(*local_id, grab_offset.clone(), now)
                    .ok();
            }
            Command::GrabObjectUpdate {
                object_id,
                grab_offset_initial,
                grab_position,
                time_since_last,
            } => {
                session
                    .grab_object_update(
                        *object_id,
                        grab_offset_initial.clone(),
                        grab_position.clone(),
                        *time_since_last,
                        now,
                    )
                    .ok();
            }
            Command::DegrabObject { local_id } => {
                session.degrab_object(*local_id, now).ok();
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
                session.avatar_picker_request(*query_id, name, now).ok();
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
                events.write(SlEvent(SessionEvent::ScriptPermissionState(
                    session.script_permission_state(),
                )));
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
                item_id,
                asset_type,
                data,
            } => {
                // `UpdatableAssetType::cap` is total — scripts (which need the
                // compile-aware `UploadScript`) are excluded from this type by
                // construction.
                let cap = asset_type.cap();
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(cap).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let body = build_update_item_asset_request(*item_id);
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
                        run_object_media_post(&url, body);
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
                        run_object_media_post(&url, body);
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
                session
                    .start_conference(*session_id, invitees, message, now)
                    .ok();
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
            Command::QueryChatSessions => {
                // Local query: build the light session list and surface it on the
                // event stream. (A bevy system may instead borrow the Session and
                // call `chat_sessions_info()` directly, skipping the round-trip.)
                events.write(SlEvent(SessionEvent::ChatSessions(
                    session.chat_sessions_info().collect(),
                )));
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
                events.write(SlEvent(SessionEvent::ChatHistoryPage {
                    session: *chat_session,
                    messages,
                    prev,
                }));
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
                events.write(SlEvent(SessionEvent::InventoryFolderPage {
                    folder: *folder,
                    folders: folders.into(),
                    items: items.into(),
                    prev,
                }));
            }
            Command::QueryInventoryRoots => {
                // Local query: surface the agent + library roots (both `Copy`).
                events.write(SlEvent(SessionEvent::InventoryRoots {
                    agent_root: session.inventory_root(),
                    library_root: session.library_root(),
                }));
            }
            Command::QueryInventoryFolders => {
                // Local query: snapshot the agent tree's known folders (seeded
                // from the login skeleton, so present before any contents fetch).
                events.write(SlEvent(SessionEvent::InventoryFolders(
                    session.inventory_folder_infos().into(),
                )));
            }
            Command::QueryFriends => {
                // Local query: build the buddy snapshot with online flags.
                events.write(SlEvent(SessionEvent::FriendsSnapshot(
                    session.friends_presence().collect(),
                )));
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
                session.request_user_info(now).ok();
            }
            Command::UpdateUserInfo {
                im_via_email,
                directory_visibility,
            } => {
                session
                    .update_user_info(*im_via_email, *directory_visibility, now)
                    .ok();
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
        diagnostics.write(SlDiagnostic(diagnostic));
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
            _ => {}
        }
        // Tap the event for the local chat log (no-op when disabled) before
        // forwarding it on.
        if chat_log.any_enabled() {
            chat_log.observe_event(&session, &event);
        }
        events.write(SlEvent(event));
    }
    if region_changed {
        caps = start_caps(&session);
    }

    if done || session.is_closed() {
        // Persist the inventory cache before exit (Firestorm's save-at-cleanup);
        // a no-op when the cache is disabled.
        inventory_cache.save(&mut session);
        SlInner::Done
    } else {
        // The optional dirty/idle inventory-cache save (crash-safety beyond
        // Firestorm's shutdown-only save); self-gating on the dirty flag and the
        // save interval, so a clean or disabled cache costs nothing.
        inventory_cache.maybe_save(&mut session, now);
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
            chat_log,
            inventory_cache,
        }
    }
}
