#![doc = include_str!("../README.md")]

use std::collections::HashMap;
use std::io::Error as IoError;
use std::time::{Duration, Instant};

use reqwest::Client as ReqwestClient;
use reqwest::Error as ReqwestError;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use sl_proto::{
    CAP_AGENT_EXPERIENCES, CAP_AGENT_PREFERENCES, CAP_ATTACHMENT_RESOURCES,
    CAP_CHAT_SESSION_REQUEST, CAP_CREATE_INVENTORY_CATEGORY, CAP_EXPERIENCE_PREFERENCES,
    CAP_EXT_ENVIRONMENT, CAP_FETCH_INVENTORY, CAP_FETCH_LIBRARY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_DISPLAY_NAMES,
    CAP_GET_EXPERIENCE_INFO, CAP_GET_EXPERIENCES, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_OBJECT_COST,
    CAP_GET_OBJECT_PHYSICS_DATA, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES, CAP_GROUP_MEMBER_DATA,
    CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN, CAP_IS_EXPERIENCE_CONTRIBUTOR,
    CAP_LAND_RESOURCES, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA,
    CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO, CAP_PROVISION_VOICE_ACCOUNT,
    CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES, CAP_REMOTE_PARCEL_REQUEST, CAP_RENDER_MATERIALS,
    CAP_RESOURCE_COST_SELECTED, CAP_SEND_USER_REPORT, CAP_SEND_USER_REPORT_WITH_SCREENSHOT,
    CAP_SIMULATOR_FEATURES, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE,
    CAP_UPDATE_SCRIPT_AGENT, CAP_UPDATE_SCRIPT_TASK, CAP_UPLOAD_BAKED_TEXTURE, CAP_VIEWER_ASSET,
    CAP_VOICE_SIGNALING, CHAT_SESSION_ACCEPT, CHAT_SESSION_DECLINE, CHAT_SESSION_DECLINE_P2P_VOICE,
    INVENTORY_FETCH_MAX_IN_FLIGHT, Llsd, RECV_BUFFER_SIZE, SelectedCostKind, Session,
    ais_category_children_fetch_url, ais_category_children_url, ais_category_url,
    ais_create_category_url, ais_item_url, build_agent_preferences_request,
    build_ais_create_category_body, build_ais_move_body, build_ais_rename_category_body,
    build_ais_update_item_body, build_create_inventory_category_request,
    build_get_object_cost_request, build_get_object_physics_data_request,
    build_modify_material_params_request, build_new_file_agent_inventory_request,
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

// Re-export the core types a consumer needs so they can depend on this crate
// alone.
pub use sl_proto::{
    ActiveGroup, AgentKey, AgentOrObjectKey, AgentPreferences, AnimatedObjects, AnimationKey,
    AnyMessage, Asset, AssetKey, AssetType, AttachmentMode, AttachmentPoint, AvatarClassified,
    AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties, Camera, CameraError,
    ChatAudible, ChatChannel, ChatLifecycleView, ChatLogConfig, ChatMessage, ChatSessionInfo,
    ChatSessionKind, ChatSource, ChatSourceType, ChatType, ChatTypeNotAVolume, Child, CircuitCode,
    CircuitId, ClassifiedCategory, ClassifiedInfo, ClassifiedKey, ClassifiedUpdate, ClickAction,
    ClientDirectories, ClockStyle, Color, ColorAlpha, Command, ControlFlags, ConversationKind,
    CreateGroupParams, DayCycle, DayCycleFrame, DeRezDestination, DetachOrder, Diagnostic,
    Direction, DiscardLevel, DisconnectReason, DisplayName, DisplayNameUpdate, Distance,
    EconomyData, EnvironmentSettings, EstateAccessDelta, EstateAccessKind, EstateCovenant,
    EstateInfo, Event, ExperienceInfo, ExperiencePermission, ExperienceProperties,
    ExperienceUpdate, ExtendedMesh, FlexibleData, FolderInfo, FolderState, FolderType, Friend,
    FriendRights, GlobalCoordinates, Glow, GltfMaterialOverride, GridCoordinates, GroupKey,
    GroupMember, GroupMembership, GroupNotice, GroupNoticeAttachment, GroupNoticeKey, GroupProfile,
    GroupRequestId, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleKey, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, HomeLocation, IceCandidate, ImDialog,
    ImSessionId, ImageCodec, InstantMessage, InterestsUpdate, InventoryCacheConfig,
    InventoryCallbackId, InventoryCursor, InventoryFolder, InventoryFolderKey, InventoryItem,
    InventoryItemOrFolderKey, InventoryKey, InventoryOffer, InventoryOwner, InventoryType,
    InviteChannel, ItemInfo, Key, Kilobits, LandArea, LandBrushAction, LandBrushSize, LandEdit,
    LandingType, LegacyMaterial, LightData, LightImage, LindenAmount, LindenBalance,
    LoadUrlRequest, LoggedChatType, LoginAccount, LoginParams, LoginRejectKind, LoginRequest,
    LoginResponse, LureId, MAX_FACES, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP,
    MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MapItem, MapItemType, MapRegionInfo, Material,
    MaterialOverrideUpdate, Maturity, MediaEntry, MeshKey, MessageCursor, MfaChallenge,
    MoneyBalance, MoneyTransaction, MoneyTransactionType, MovementMode, MuteEntry, MuteFlags,
    MuteType, NegativeBalanceError, NeighborInfo, NewInventoryItem, NewInventoryLink, Object,
    ObjectExtraParams, ObjectFlagSettings, ObjectKey, ObjectMediaResponse, ObjectMotion,
    ObjectPermMasks, ObjectProperties, ObjectPropertiesFamily, ObjectTransform, OpenRegionInfo,
    OpenSimExtras, OwnerKey, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope,
    ParcelCategory, ParcelDetails, ParcelFlags, ParcelInfo, ParcelKey, ParcelMediaCommand,
    ParcelMediaUpdateInfo, ParcelObjectOwner, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate, ParcelVoiceInfo, ParticleSystem, PermissionField,
    Permissions, Permissions5, PhysicsShapeTypes, PickInfo, PickKey, PickUpdate, PingId,
    PlayingAnimation, PrimShape, PrimShapeParams, ProductType, ProfileUpdate, ProposalCandidateId,
    ProposalVoteId, QueryId, ReflectionProbe, ReflectionProbeFlags, RegionChatSettings,
    RegionCombatSettings, RegionCoordinates, RegionFlags, RegionHandle, RegionIdentity,
    RegionInfoUpdate, RegionLimits, RegionLocalObjectId, RegionLocalParcelId, RegionName,
    RegionTerrainComposition, Reliability, RenderMaterialEntry, RenderMaterialRef, RestoreItem,
    RezAttachment, RezObjectParams, RezScriptParams, Rotation, SaleType, ScopedObjectId,
    ScopedParcelId, ScriptCompileError, ScriptControl, ScriptControlAction, ScriptDialog,
    ScriptLanguage, ScriptPermissionRequest, ScriptPermissions, ScriptTarget,
    ScriptTeleportRequest, ScriptUploadLocation, SculptData, SculptOrMeshKey, SequenceNumber,
    SessionMessage, SetDisplayNameReply, SimulatorFeatures, SkySettings, SoundFlags, SoundPreload,
    StartLocation, StartLocationParseError, TaskInventoryItem, TaskInventoryKey,
    TaskInventoryReply, TerraformArea, TerrainLayerType, TerrainPatch, Texture, TextureAnimation,
    TextureEntry, TextureFace, TextureKey, Throttle, ThrottleBuilder, ThrottleError,
    TimestampFormat, TransactionId, TransferId, TransferStatus, Transmit, UpdatableAssetType, Uuid,
    Vector, VoiceAccountInfo, VoiceProvisionRequest, WaterSettings, Wearable, WearableType, XferId,
    avatar_texture, decode_particle_system, decode_texture_anim, decode_texture_entry,
    encode_texture_entry, grid_to_handle, group_powers, handle_to_global, handle_to_grid, j2c,
    particle_pattern, pcode, sim_access, texture_anim_mode,
};
// `sl_texture::TextureEntry` (the store's LOD-aware texture object) and
// `TextureReadLease` are reachable as `sl_texture::…`; they are not re-exported
// flat because `TextureEntry` would collide with `sl_proto`'s prim-face type.
// `StoreStats` / `GateStats` (the pipeline-status snapshots) are the same shared
// `sl-asset-sched` types across the texture / mesh / asset stores, so they are
// re-exported once here (from `sl_texture`) rather than three times.
pub use sl_texture::{
    AssetFetcher, CacheLimits, DecodedImage as DecodedTexture, FetchChunk, GateStats,
    NotRemotelyFetchable, Priority, RemoteTextureSource, StoreStats, TextureError,
    TextureFetchType, TextureFetcher, TextureProgress, TextureRequest, TextureStore,
};
// The decoding, LOD-aware mesh store (the mesh counterpart of the texture
// store). `Priority` and `MeshKey` are already re-exported (from `sl_texture` /
// `sl_proto`); the mesh `CacheLimits` is aliased so it does not collide with the
// texture one.
pub use sl_mesh::{
    CacheLimits as MeshCacheLimits, DEFAULT_LOD_FACTOR, DecodedMesh, MeshEntry, MeshError,
    MeshFetcher, MeshLod, MeshPhysics, MeshProgress, MeshReadLease, MeshRequest, MeshSkin,
    MeshStore, Submesh,
};

// The generic-asset store (the opaque-blob counterpart of the texture/mesh
// stores), fetched whole over the `ViewerAsset` capability. Its `CacheLimits` is
// aliased so it does not collide with the texture/mesh ones; `Priority`,
// `AssetKey`, and `AssetType` are already re-exported.
pub use sl_asset::{
    AssetEntry, AssetError, AssetProgress, AssetRef, AssetStore, BlobFetcher,
    CacheLimits as AssetCacheLimits, FetchError as AssetFetchError,
};

// The GLTF (PBR) render-material asset decoder (`AT_MATERIAL`), the material
// counterpart of `sl_mesh` / `sl_texture`: a client fetches a material asset
// over the `ViewerAsset` capability (the generic `AssetStore` above) and decodes
// it into a renderer-agnostic `GltfMaterial` (P27.1), plus the per-face
// `MaterialOverride` delta (P27.2) layered on the base material. Kept at parity
// with the Bevy runtime.
pub use sl_material::{
    GltfAlphaMode, GltfMaterial, GltfTexture, GltfTextureTransform,
    MaterialError as GltfMaterialError, MaterialOverride, TextureOverride,
    TextureTransformOverride, parse_gltf_material_document, parse_material_asset,
    parse_material_override,
};

pub use crate::assets::ReqwestAssetFetcher;
pub use crate::meshes::ReqwestMeshFetcher;
pub use crate::textures::ReqwestTextureFetcher;

mod appearance;
pub mod assets;
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
mod retry;
pub mod textures;
mod upload;
mod voice;
use crate::appearance::request_server_appearance_update;
use crate::caps::{
    CAPS_FAILURE_PREFIX, abort_task, fetch_capabilities, make_sleep, spawn_event_queue,
    spawn_simulator_features,
};
use crate::chat_log::ChatLog;
use crate::experiences::{
    fetch_experience_admin, fetch_experience_contributor, fetch_group_experiences,
};
use crate::fetch::{fetch_asset_http, fetch_mesh_http, fetch_texture_http};
use crate::http::{
    delete_caps_llsd, fetch_land_resources, get_caps_llsd, patch_caps_llsd, post_caps_oneway,
    post_chat_session_request, put_caps_llsd,
};
use crate::inventory::{fetch_folder_contents, fetch_group_members, fetch_inventory};
use crate::inventory_cache::InventoryCache;
use crate::materials::{fetch_render_materials, post_modify_material_params};
use crate::media::{fetch_object_media, post_object_media};
use crate::upload::{run_caps_upload, run_report_screenshot_upload, run_script_upload};
use crate::voice::{post_voice_cap, post_voice_signaling};

/// How long to sleep when the session has no scheduled timeout.
const IDLE_SLEEP: Duration = Duration::from_secs(3600);

/// An error from the tokio client.
#[derive(Debug, Error)]
pub enum Error {
    /// A UDP socket I/O error.
    #[error("socket I/O error: {0}")]
    Io(#[from] IoError),
    /// An HTTP error while performing the XML-RPC login.
    #[error("login HTTP error: {0}")]
    Http(#[from] ReqwestError),
    /// The login response could not be parsed.
    #[error("login parse error: {0}")]
    Login(#[from] sl_wire::LoginParseError),
    /// A protocol state-machine error.
    #[error("protocol error: {0}")]
    Proto(#[from] sl_proto::Error),
    /// The grid rejected the login.
    #[error("login rejected: {reason} ({message})")]
    LoginRejected {
        /// A coarse classification of the rejection, so a caller can recognise
        /// the retryable "already logged in" case without matching on `reason`.
        kind: LoginRejectKind,
        /// The machine-readable reason code.
        reason: String,
        /// The human-readable message.
        message: String,
    },
    /// The grid requires a multi-factor one-time code. Retry [`Client::connect`]
    /// with a [`LoginRequest`] prepared via `LoginRequest::with_mfa`.
    #[error("multi-factor authentication required: {}", .0.message)]
    MfaChallenge(MfaChallenge),
    /// The session unexpectedly had no login request to perform.
    #[error("the session produced no login request")]
    NoLoginRequest,
    /// The region's capabilities could not be fetched from the seed URL, so login is
    /// aborted rather than proceeding into a capless session (the seed-caps request
    /// also advertises animesh support, and every cap-backed feature — asset fetch,
    /// the event queue, inventory — would be dead). The initial-login
    /// `CompleteAgentMovement` is deferred until caps arrive, so it is never sent and
    /// the agent never fully arrives.
    #[error("could not fetch region capabilities: {message}")]
    NoCapabilities {
        /// A readable description of why the capability fetch failed.
        message: String,
    },
}

/// A tokio-driven Second Life / OpenSim client wrapping a sans-I/O [`Session`].
#[derive(Debug)]
pub struct Client {
    /// The sans-I/O session being driven.
    session: Session,
    /// The bound UDP socket.
    socket: UdpSocket,
    /// A reusable receive buffer.
    recv_buf: Vec<u8>,
    /// An optional channel over which [`Client::run`] reports the region's
    /// capability map (name → URL) each time it is fetched (at startup and on
    /// every region change), for a driver that wants to resolve/symbolize
    /// `$cap:Name` placeholders.
    caps_reporter: Option<mpsc::Sender<HashMap<String, String>>>,
    /// The optional local chat-log configuration (default off). When any text-chat
    /// type is enabled, [`Client::run`] writes Firestorm-compatible transcripts and
    /// serves file-backed history pages.
    chat_log_config: ChatLogConfig,
    /// The per-account filesystem directories the runtime persists its optional
    /// features under (chat-log transcripts, the inventory disk-cache). Default
    /// all-`None`, disabling every disk feature; set via
    /// [`Client::set_directories`] before [`Client::run`].
    directories: ClientDirectories,
    /// The inventory disk-cache configuration. Off by default; once enabled (and
    /// paired with [`ClientDirectories::agent_cache_dir`]), [`Client::run`] loads
    /// the cache at login, reconciles it against the skeleton, and persists it on
    /// logout and the dirty/idle tick. Set via
    /// [`Client::set_inventory_cache_config`] before [`Client::run`].
    inventory_cache_config: InventoryCacheConfig,
}

impl Client {
    /// Logs in over XML-RPC, binds a UDP socket, and bootstraps the circuit.
    ///
    /// # Errors
    ///
    /// Returns an [`enum@Error`] if the login HTTP request, the response parse, the
    /// socket bind, or the circuit bootstrap fails.
    pub async fn connect(params: LoginParams) -> Result<Self, Error> {
        let mut session = Session::new(params);
        let request = session.login_http_request().ok_or(Error::NoLoginRequest)?;

        let http = ReqwestClient::new();
        let body = http
            .post(request.url)
            .header("Content-Type", "text/xml")
            .header("User-Agent", &request.user_agent)
            .body(request.body)
            .send()
            .await?
            .text()
            .await?;
        let success = match parse_login_response(&body)? {
            LoginResponse::Success(success) => *success,
            LoginResponse::MfaChallenge(challenge) => return Err(Error::MfaChallenge(challenge)),
            LoginResponse::Failure(failure) => {
                return Err(Error::LoginRejected {
                    kind: failure.kind(),
                    reason: failure.reason,
                    message: failure.message,
                });
            }
        };

        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        session.handle_login_response(LoginResponse::Success(Box::new(success)), Instant::now())?;

        Ok(Self {
            session,
            socket,
            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
            caps_reporter: None,
            chat_log_config: ChatLogConfig::default(),
            directories: ClientDirectories::default(),
            inventory_cache_config: InventoryCacheConfig::default(),
        })
    }

    /// The agent's own id, available once logged in. Useful for self-directed
    /// requests (e.g. reading one's own picks or classifieds) before
    /// [`Client::run`] consumes the client.
    #[must_use]
    pub fn agent_id(&self) -> Option<AgentKey> {
        self.session.agent_id()
    }

    /// The region handle of the region the agent logged in to, available once
    /// logged in. Seeded from the login response, so a driver can issue an
    /// intra-region [`Command::Teleport`] before
    /// [`Client::run`] consumes the client.
    #[must_use]
    pub fn region_handle(&self) -> Option<RegionHandle> {
        self.session.region_handle()
    }

    /// The identity of the current root circuit, available once logged in (the
    /// login response establishes it). A driver pairs it with a region-local id
    /// to build a [`ScopedParcelId`] / [`ScopedObjectId`] for the region the
    /// agent is in, before [`Client::run`] consumes the client.
    #[must_use]
    pub fn root_circuit_id(&self) -> Option<CircuitId> {
        self.session.root_circuit_id()
    }

    /// The session id, available once logged in. Useful for symbolizing the
    /// session in a REPL/diagnostic log before [`Client::run`] consumes the
    /// client.
    #[must_use]
    pub const fn session_id(&self) -> Option<Uuid> {
        self.session.session_id()
    }

    /// The circuit code, available once logged in. Useful for symbolizing the
    /// circuit in a REPL/diagnostic log before [`Client::run`] consumes the
    /// client.
    #[must_use]
    pub const fn circuit_code(&self) -> Option<CircuitCode> {
        self.session.circuit_code()
    }

    /// The region's seed capability URL, available once logged in. A REPL driver
    /// can seed its placeholder context with it before [`Client::run`] consumes
    /// the client.
    #[must_use]
    pub const fn seed_capability(&self) -> Option<&url::Url> {
        self.session.seed_capability()
    }

    /// Enables or disables protocol diagnostics for the session. Off by default;
    /// while enabled, the session records [`Diagnostic`]s for anomalies it would
    /// otherwise silently drop (decode failures, unhandled messages, unknown
    /// CAPS events, missing expected replies), and [`Client::run`] forwards them
    /// over its `diagnostics` channel. Call before [`Client::run`].
    pub fn set_diagnostics(&mut self, enabled: bool) {
        self.session.set_diagnostics(enabled);
    }

    /// Sets the channel over which [`Client::run`] reports the region's
    /// capability map (name → URL). The map is sent once after the seed
    /// capability is fetched at startup and again after every region change.
    /// Call before [`Client::run`]; a slow or dropped receiver never blocks the
    /// session (the send is best-effort). Useful for resolving/symbolizing
    /// `$cap:Name` placeholders in a REPL or diagnostic driver.
    pub fn set_caps_reporter(&mut self, reporter: mpsc::Sender<HashMap<String, String>>) {
        self.caps_reporter = Some(reporter);
    }

    /// Sets the local chat-log configuration. Off by default; once any text-chat
    /// type is enabled, [`Client::run`] writes Firestorm-compatible transcripts for
    /// nearby chat / IMs / group / conference sessions (per the enabled set) and
    /// serves the older, file-backed pages of
    /// [`Command::QueryChatHistoryPage`].
    /// Call before [`Client::run`].
    pub fn set_chat_log_config(&mut self, config: ChatLogConfig) {
        self.chat_log_config = config;
    }

    /// Sets the per-account filesystem directories the runtime persists its
    /// optional features under. Default all-`None` (every disk feature disabled):
    /// [`ClientDirectories::agent_chat_log_dir`] is where [`Client::run`] writes
    /// chat-log transcripts (paired with [`Client::set_chat_log_config`]), and
    /// [`ClientDirectories::agent_cache_dir`] is where it reads/writes the
    /// inventory disk-cache. A `None` field disables that feature. Call before
    /// [`Client::run`].
    pub fn set_directories(&mut self, directories: ClientDirectories) {
        self.directories = directories;
    }

    /// Sets the inventory disk-cache configuration. Off by default; once enabled
    /// (and paired with a [`ClientDirectories::agent_cache_dir`] via
    /// [`Client::set_directories`]), [`Client::run`] reads the per-account
    /// `<agent-uuid>.inv.llsd.gz` cache before the login skeleton, reconciles it
    /// against the skeleton so version-matching folders skip the background
    /// refetch, and writes the cache back on logout and on a dirty/idle tick.
    /// Call before [`Client::run`].
    pub const fn set_inventory_cache_config(&mut self, config: InventoryCacheConfig) {
        self.inventory_cache_config = config;
    }

    /// Enables or disables the automatic background inventory crawl (off by
    /// default). While enabled, [`Client::run`] breadth-first fetches the agent's
    /// inventory tree in the background (a bounded number of folder-contents
    /// requests in flight), so the held model fills in without explicit
    /// per-folder requests. While disabled, no folder fetches are issued unless
    /// the driver asks for one
    /// ([`Command::RequestFolderContents`]
    /// / [`Command::FetchInventoryFolders`]),
    /// so a consumer that ignores inventory pays nothing. Call before
    /// [`Client::run`].
    pub const fn set_background_inventory_fetch(&mut self, enabled: bool) {
        self.session.set_background_inventory_fetch(enabled);
    }

    /// Runs the session until it is disconnected or logged out, forwarding
    /// events to `events`, diagnostics to `diagnostics` (only when enabled via
    /// [`Client::set_diagnostics`]), and applying commands from `commands`.
    ///
    /// # Errors
    ///
    /// Returns an [`enum@Error`] on an unrecoverable socket or protocol error.
    pub async fn run(
        mut self,
        events: mpsc::Sender<Event>,
        diagnostics: mpsc::Sender<Diagnostic>,
        mut commands: mpsc::Receiver<Command>,
    ) -> Result<(), Error> {
        // The region's capability map is fetched once from the seed and cached
        // here: the event-queue long-poll runs off `EventQueueGet`, and inventory
        // fetches POST to `FetchInventoryDescendents2`. Both deliver their decoded
        // payloads back over `caps_rx` to `handle_caps_event`.
        let http = ReqwestClient::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let (caps_tx, mut caps_rx) = mpsc::channel::<(String, Llsd)>(64);
        // The region must serve capabilities: fail login (propagating the readable
        // error) rather than proceed into a capless session. Releasing the deferred
        // `CompleteAgentMovement` only now, on success, also guarantees the simulator
        // knows we render animesh (the seed-caps request advertises it) before it
        // streams the scene — its one-shot `ObjectAnimation` is gated on both.
        let mut caps = fetch_capabilities(self.session.seed_capability(), &http).await?;
        self.session.notify_capabilities_ready(Instant::now())?;
        if let Some(reporter) = &self.caps_reporter {
            reporter.send(caps.clone()).await.ok();
        }
        spawn_simulator_features(&caps, &http, &caps_tx);
        let mut caps_task = spawn_event_queue(&caps, &http, &caps_tx);

        // The optional local chat-log writer. Constructed even when disabled (its
        // methods short-circuit on the empty enabled-set) so the tap sites stay
        // unconditional; it owns the only file I/O in the runtime.
        let mut chat_log = ChatLog::new(
            self.chat_log_config.clone(),
            self.directories.agent_chat_log_dir.clone(),
            self.session.agent_legacy_name(),
            self.session.agent_id(),
        );

        // The optional inventory disk cache. Like the chat log it is constructed
        // even when disabled (its methods short-circuit), so the load / save taps
        // stay unconditional. It owns the gzip envelope and the crash-safe write.
        let mut inventory_cache = InventoryCache::new(
            self.inventory_cache_config,
            self.directories.agent_cache_dir.clone(),
            self.session.agent_id(),
            Instant::now(),
        );

        loop {
            while let Some(transmit) = self.session.poll_transmit() {
                self.socket
                    .send_to(&transmit.payload, transmit.destination)
                    .await?;
            }

            while let Some(diagnostic) = self.session.poll_diagnostic() {
                diagnostics.send(diagnostic).await.ok();
            }

            while let Some(event) = self.session.poll_event() {
                let terminal = matches!(event, Event::Disconnected(_) | Event::LoggedOut);
                // A region change brings a new seed capability, so re-fetch the
                // capability map and restart the event-queue poller.
                let region_changed = matches!(event, Event::RegionChanged { .. });
                // POST a neighbour's seed capability so the simulator starts
                // streaming that region's scene to the child circuit (its
                // `SendInitialData` is gated on the seed having been requested).
                // Detached: the POST must not block the main loop.
                if let Event::NeighborSeed {
                    seed_capability, ..
                } = &event
                {
                    let seed = seed_capability.clone();
                    let http = http.clone();
                    tokio::spawn(async move {
                        let _ignored = fetch_capabilities(Some(&seed), &http).await;
                    });
                }
                // Tap the event for the local chat log (best-effort, no-op when
                // disabled) before forwarding it on.
                if chat_log.any_enabled() {
                    chat_log.observe_event(&self.session, &event);
                }
                // On the login inventory/library skeleton, load the disk cache (if
                // any) and reconcile it against the skeleton, so version-matching
                // folders skip the background refetch. A no-op when the cache is
                // disabled.
                match &event {
                    Event::InventorySkeleton(folders) => {
                        inventory_cache.load_agent(&mut self.session, folders);
                    }
                    Event::LibraryInventory(folders) => {
                        inventory_cache.load_library(&mut self.session, folders);
                    }
                    _other => {}
                }
                events.send(event).await.ok();
                if region_changed {
                    abort_task(&mut caps_task);
                    // A region change's cap fetch is best-effort: the session is
                    // already established, so a failure degrades the new region
                    // rather than failing the session (unlike the initial login).
                    caps = fetch_capabilities(self.session.seed_capability(), &http)
                        .await
                        .unwrap_or_default();
                    if let Some(reporter) = &self.caps_reporter {
                        reporter.send(caps.clone()).await.ok();
                    }
                    spawn_simulator_features(&caps, &http, &caps_tx);
                    caps_task = spawn_event_queue(&caps, &http, &caps_tx);
                }
                if terminal {
                    // Persist the inventory cache before exit (Firestorm's
                    // save-at-cleanup); a no-op when the cache is disabled.
                    inventory_cache.save(&mut self.session);
                    abort_task(&mut caps_task);
                    return Ok(());
                }
            }
            if self.session.is_closed() {
                inventory_cache.save(&mut self.session);
                abort_task(&mut caps_task);
                return Ok(());
            }
            // The optional dirty/idle inventory-cache save (crash-safety beyond
            // Firestorm's shutdown-only save); self-gating on the dirty flag and
            // the save interval, so a clean or disabled cache costs nothing.
            inventory_cache.maybe_save(&mut self.session, Instant::now());

            // Background inventory crawl: when enabled, sweep the next bounded
            // batch of unfetched folders and POST a `FetchInventoryDescendents2`
            // for each. Self-gating — `next_inventory_fetch_batch` returns empty
            // when the crawl is off, so this costs nothing for a consumer that
            // ignores inventory. The replies fold in over `caps_rx` and the next
            // loop iteration continues the sweep a level deeper. Only swept while
            // the fetch capability and agent id are known, so folders are never
            // flipped to `Fetching` for a request that cannot be issued.
            if let (Some(url), Some(owner)) = (
                caps.get(CAP_FETCH_INVENTORY).cloned(),
                self.session.agent_id(),
            ) {
                let batch = self
                    .session
                    .next_inventory_fetch_batch(INVENTORY_FETCH_MAX_IN_FLIGHT);
                // The batch can span both trees (the scheduler walks from both
                // roots): the agent folders go to `FetchInventoryDescendents2` with
                // the agent owner, the Library folders to `FetchLibDescendents2`
                // with the Library owner (or, where the grid does not serve that cap
                // — e.g. OpenSim — over the UDP path instead, so they never stay
                // stuck `Fetching`).
                let (library_folders, agent_folders): (Vec<_>, Vec<_>) =
                    batch.into_iter().partition(|folder| {
                        self.session.inventory_owner(*folder) == Some(InventoryOwner::Library)
                    });
                if !agent_folders.is_empty() {
                    tokio::spawn(fetch_inventory(
                        url,
                        owner.uuid(),
                        agent_folders,
                        CAP_FETCH_INVENTORY,
                        http.clone(),
                        caps_tx.clone(),
                    ));
                }
                if !library_folders.is_empty() {
                    match (
                        caps.get(CAP_FETCH_LIBRARY).cloned(),
                        self.session.library_owner(),
                    ) {
                        (Some(lib_url), Some(lib_owner)) => {
                            tokio::spawn(fetch_inventory(
                                lib_url,
                                lib_owner.uuid(),
                                library_folders,
                                CAP_FETCH_LIBRARY,
                                http.clone(),
                                caps_tx.clone(),
                            ));
                        }
                        _ => {
                            for folder in library_folders {
                                self.session
                                    .request_folder_contents(folder, Instant::now())?;
                            }
                        }
                    }
                }
            }

            let sleep = make_sleep(self.session.poll_timeout());
            tokio::pin!(sleep);

            tokio::select! {
                result = self.socket.recv_from(&mut self.recv_buf) => {
                    let (len, from) = result?;
                    if let Some(datagram) = self.recv_buf.get(..len) {
                        self.session.handle_datagram(from, datagram, Instant::now())?;
                    }
                }
                caps_event = caps_rx.recv() => {
                    if let Some((message, body)) = caps_event {
                        // A CAPS helper reports a failed request by sending the
                        // failure sentinel rather than a decoded reply; surface
                        // it as a diagnostic instead of feeding the session.
                        if let Some(cap) = message.strip_prefix(CAPS_FAILURE_PREFIX) {
                            tracing::warn!(
                                capability = cap,
                                "CAPS request failed; no reply surfaced"
                            );
                            if self.session.diagnostics_enabled() {
                                diagnostics
                                    .send(Diagnostic::ExpectedReplyMissing {
                                        request: cap.to_owned(),
                                        sequence: None,
                                    })
                                    .await
                                    .ok();
                            }
                        } else {
                            self.session.handle_caps_event(&message, &body, Instant::now())?;
                        }
                    }
                }
                command = commands.recv() => {
                    match command {
                        Some(Command::Send { message, reliability }) => {
                            self.session.enqueue(*message, reliability, Instant::now())?;
                        }
                        Some(Command::Chat { message, chat_type, channel }) => {
                            self.session.say(&message, chat_type, channel, Instant::now())?;
                        }
                        Some(Command::Typing(typing)) => {
                            self.session.set_typing(typing, Instant::now())?;
                        }
                        Some(Command::InstantMessage { to_agent_id, message }) => {
                            self.session.send_instant_message(to_agent_id, &message, Instant::now())?;
                            chat_log.log_outbound_im(to_agent_id, &message);
                        }
                        Some(Command::ImTyping { to_agent_id, typing }) => {
                            self.session.send_im_typing(to_agent_id, typing, Instant::now())?;
                        }
                        Some(Command::SetControls(controls)) => {
                            self.session.set_controls(controls, Instant::now())?;
                        }
                        Some(Command::SetThrottle(throttle)) => {
                            self.session.set_throttle(throttle, Instant::now())?;
                        }
                        Some(Command::SetRotation { body, head }) => {
                            self.session.set_rotation(body, head, Instant::now())?;
                        }
                        Some(Command::SetCamera(camera)) => {
                            self.session.set_camera(camera, Instant::now())?;
                        }
                        Some(Command::Stand) => {
                            self.session.stand(Instant::now())?;
                        }
                        Some(Command::SitOnGround) => {
                            self.session.sit_on_ground(Instant::now())?;
                        }
                        Some(Command::Sit { target, offset }) => {
                            self.session.sit_on(target, offset, Instant::now())?;
                        }
                        Some(Command::Autopilot { global_x, global_y, z }) => {
                            self.session.autopilot_to(global_x, global_y, z, Instant::now())?;
                        }
                        Some(Command::RequestAvatarProperties(target)) => {
                            self.session.request_avatar_properties(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarPicks(target)) => {
                            self.session.request_avatar_picks(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarNotes(target)) => {
                            self.session.request_avatar_notes(target, Instant::now())?;
                        }
                        Some(Command::RequestAvatarClassifieds(target)) => {
                            self.session
                                .request_avatar_classifieds(target, Instant::now())?;
                        }
                        Some(Command::RequestPickInfo {
                            creator_id,
                            pick_id,
                        }) => {
                            self.session
                                .request_pick_info(creator_id, pick_id, Instant::now())?;
                        }
                        Some(Command::RequestClassifiedInfo(classified_id)) => {
                            self.session
                                .request_classified_info(classified_id, Instant::now())?;
                        }
                        Some(Command::UpdateProfile(update)) => {
                            self.session.update_profile(&update, Instant::now())?;
                        }
                        Some(Command::UpdateInterests(update)) => {
                            self.session.update_interests(&update, Instant::now())?;
                        }
                        Some(Command::UpdateAvatarNotes { target_id, notes }) => {
                            self.session
                                .update_avatar_notes(target_id, &notes, Instant::now())?;
                        }
                        Some(Command::UpdatePick(update)) => {
                            self.session.update_pick(&update, Instant::now())?;
                        }
                        Some(Command::DeletePick(pick_id)) => {
                            self.session.delete_pick(pick_id, Instant::now())?;
                        }
                        Some(Command::GodDeletePick { pick_id, query_id }) => {
                            self.session
                                .god_delete_pick(pick_id, query_id, Instant::now())?;
                        }
                        Some(Command::UpdateClassified(update)) => {
                            self.session.update_classified(&update, Instant::now())?;
                        }
                        Some(Command::DeleteClassified(classified_id)) => {
                            self.session
                                .delete_classified(classified_id, Instant::now())?;
                        }
                        Some(Command::GodDeleteClassified {
                            classified_id,
                            query_id,
                        }) => {
                            self.session.god_delete_classified(
                                classified_id,
                                query_id,
                                Instant::now(),
                            )?;
                        }
                        Some(Command::RequestFolderContents(folder_id)) => {
                            fetch_folder_contents(
                                &mut self.session,
                                folder_id,
                                &caps,
                                &http,
                                &caps_tx,
                                Instant::now(),
                            )?;
                        }
                        Some(Command::FetchInventoryFolders(folder_ids)) => {
                            if let (Some(url), Some(owner)) =
                                (caps.get(CAP_FETCH_INVENTORY).cloned(), self.session.agent_id())
                            {
                                tokio::spawn(fetch_inventory(
                                    url,
                                    owner.uuid(),
                                    folder_ids,
                                    CAP_FETCH_INVENTORY,
                                    http.clone(),
                                    caps_tx.clone(),
                                ));
                            }
                        }
                        Some(Command::CreateInventoryFolder { folder_id, parent_id, folder_type, name }) => {
                            self.session.create_inventory_folder(folder_id, parent_id, folder_type, &name, Instant::now())?;
                        }
                        Some(Command::UpdateInventoryFolder { folder_id, parent_id, folder_type, name }) => {
                            self.session.update_inventory_folder(folder_id, parent_id, folder_type, &name, Instant::now())?;
                        }
                        Some(Command::MoveInventoryFolder { folder_id, parent_id }) => {
                            self.session.move_inventory_folder(folder_id, parent_id, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryFolders(folder_ids)) => {
                            self.session.remove_inventory_folders(&folder_ids, Instant::now())?;
                        }
                        Some(Command::CreateInventoryItem(new)) => {
                            self.session.create_inventory_item(&new, Instant::now())?;
                        }
                        Some(Command::CreateScript { folder_id, name, description, next_owner_mask, language }) => {
                            self.session.create_script(folder_id, &name, &description, next_owner_mask, language, Instant::now())?;
                        }
                        Some(Command::LinkInventoryItem(new)) => {
                            self.session.link_inventory_item(&new, Instant::now())?;
                        }
                        Some(Command::UpdateInventoryItem { item, transaction_id }) => {
                            self.session.update_inventory_item(&item, transaction_id, Instant::now())?;
                        }
                        Some(Command::MoveInventoryItem { item_id, folder_id, new_name }) => {
                            self.session.move_inventory_item(item_id, folder_id, &new_name, Instant::now())?;
                        }
                        Some(Command::CopyInventoryItem { old_agent_id, old_item_id, new_folder_id, new_name }) => {
                            self.session.copy_inventory_item(old_agent_id, old_item_id, new_folder_id, &new_name, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryItems(item_ids)) => {
                            self.session.remove_inventory_items(&item_ids, Instant::now())?;
                        }
                        Some(Command::ChangeInventoryItemFlags { item_id, flags }) => {
                            self.session.change_inventory_item_flags(item_id, flags, Instant::now())?;
                        }
                        Some(Command::PurgeInventoryDescendents(folder_id)) => {
                            self.session.purge_inventory_descendents(folder_id, Instant::now())?;
                        }
                        Some(Command::RemoveInventoryObjects { folder_ids, item_ids }) => {
                            self.session.remove_inventory_objects(&folder_ids, &item_ids, Instant::now())?;
                        }
                        Some(Command::CreateInventoryCategory { parent_id, folder_type, name }) => {
                            if let Some(url) = caps.get(CAP_CREATE_INVENTORY_CATEGORY).cloned() {
                                let body = build_create_inventory_category_request(InventoryFolderKey::from(Uuid::new_v4()), parent_id, folder_type, &name);
                                tokio::spawn(post_voice_cap(url, body, CAP_CREATE_INVENTORY_CATEGORY, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3CreateFolder { parent_id, folder_type, name }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_create_category_url(parent_id, Uuid::new_v4()));
                                let body = build_ais_create_category_body(folder_type, &name);
                                tokio::spawn(post_voice_cap(url, body, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RenameFolder { folder_id, name }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_rename_category_body(&name), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3MoveFolder { folder_id, parent_id }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_move_body(parent_id), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RemoveFolder(folder_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_url(folder_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3PurgeFolder(folder_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_children_url(folder_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3FetchFolderChildren { folder_id, depth }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_category_children_fetch_url(folder_id, depth));
                                tokio::spawn(get_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3UpdateItem { item_id, name, description }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_update_item_body(&name, &description), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3MoveItem { item_id, parent_id }) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(patch_caps_llsd(url, build_ais_move_body(parent_id), CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3RemoveItem(item_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(delete_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::Ais3FetchItem(item_id)) => {
                            if let Some(base) = caps.get(CAP_INVENTORY_API_V3).cloned() {
                                let url = format!("{base}{}", ais_item_url(item_id));
                                tokio::spawn(get_caps_llsd(url, CAP_INVENTORY_API_V3, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::FetchGroupMembers(group_id)) => {
                            if let Some(url) = caps.get(CAP_GROUP_MEMBER_DATA).cloned() {
                                tokio::spawn(fetch_group_members(url, group_id, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::OfferFriendship { to_agent_id, message }) => {
                            self.session.send_friendship_offer(to_agent_id, &message, Instant::now())?;
                        }
                        Some(Command::GrantUserRights { target, rights }) => {
                            self.session.grant_user_rights(target, rights, Instant::now())?;
                        }
                        Some(Command::TerminateFriendship(other)) => {
                            self.session.terminate_friendship(other, Instant::now())?;
                        }
                        Some(Command::AcceptFriendship { transaction_id, friend_id, calling_card_folder }) => {
                            self.session.accept_friendship(transaction_id, friend_id, calling_card_folder, Instant::now())?;
                        }
                        Some(Command::DeclineFriendship(transaction_id)) => {
                            self.session.decline_friendship(transaction_id, Instant::now())?;
                        }
                        Some(Command::OfferCallingCard { to_agent_id, transaction_id }) => {
                            self.session.offer_calling_card(to_agent_id, transaction_id, Instant::now())?;
                        }
                        Some(Command::AcceptCallingCard { transaction_id, calling_card_folder }) => {
                            self.session.accept_calling_card(transaction_id, calling_card_folder, Instant::now())?;
                        }
                        Some(Command::DeclineCallingCard(transaction_id)) => {
                            self.session.decline_calling_card(transaction_id, Instant::now())?;
                        }
                        Some(Command::ActivateGroup(group_id)) => {
                            self.session.activate_group(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupMembers(group_id)) => {
                            self.session.request_group_members(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupRoles(group_id)) => {
                            self.session.request_group_roles(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupRoleMembers(group_id)) => {
                            self.session.request_group_role_members(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupTitles(group_id)) => {
                            self.session.request_group_titles(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupProfile(group_id)) => {
                            self.session.request_group_profile(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupNotices(group_id)) => {
                            self.session.request_group_notices(group_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupNotice(notice_id)) => {
                            self.session.request_group_notice(notice_id, Instant::now())?;
                        }
                        Some(Command::CreateGroup(params)) => {
                            self.session.create_group(&params, Instant::now())?;
                        }
                        Some(Command::UpdateGroupInfo(params)) => {
                            self.session.update_group_info(&params, Instant::now())?;
                        }
                        Some(Command::UpdateGroupTitle { group_id, title_role_id }) => {
                            self.session.update_group_title(group_id, title_role_id, Instant::now())?;
                        }
                        Some(Command::JoinGroup(group_id)) => {
                            self.session.join_group(group_id, Instant::now())?;
                        }
                        Some(Command::LeaveGroup(group_id)) => {
                            self.session.leave_group(group_id, Instant::now())?;
                        }
                        Some(Command::InviteToGroup { group_id, invitees }) => {
                            self.session.invite_to_group(group_id, &invitees, Instant::now())?;
                        }
                        Some(Command::SetGroupAcceptNotices { group_id, accept_notices, list_in_profile }) => {
                            self.session.set_group_accept_notices(group_id, accept_notices, list_in_profile, Instant::now())?;
                        }
                        Some(Command::SetGroupContribution { group_id, contribution }) => {
                            self.session.set_group_contribution(group_id, contribution, Instant::now())?;
                        }
                        Some(Command::StartGroupSession(group_id)) => {
                            self.session.start_group_session(group_id, Instant::now())?;
                        }
                        Some(Command::SendGroupMessage { group_id, message }) => {
                            self.session.send_group_message(group_id, &message, Instant::now())?;
                            if let Some(own) = self.session.agent_id() {
                                let name = self.session.agent_legacy_name();
                                chat_log.log_group(group_id, own, &name, &message);
                            }
                        }
                        Some(Command::LeaveGroupSession(group_id)) => {
                            self.session.leave_group_session(group_id, Instant::now())?;
                        }
                        Some(Command::UpdateGroupRoles { group_id, roles }) => {
                            self.session.update_group_roles(group_id, &roles, Instant::now())?;
                        }
                        Some(Command::ChangeGroupRoleMembers { group_id, changes }) => {
                            self.session.change_group_role_members(group_id, &changes, Instant::now())?;
                        }
                        Some(Command::EjectGroupMembers { group_id, member_ids }) => {
                            self.session.eject_group_members(group_id, &member_ids, Instant::now())?;
                        }
                        Some(Command::ActivateGestures { gestures }) => {
                            self.session.activate_gestures(&gestures, Instant::now())?;
                        }
                        Some(Command::DeactivateGestures { item_ids }) => {
                            self.session.deactivate_gestures(&item_ids, Instant::now())?;
                        }
                        Some(Command::SetAlwaysRun { mode }) => {
                            self.session.set_always_run(mode, Instant::now())?;
                        }
                        Some(Command::PauseAgent) => {
                            self.session.pause_agent(Instant::now())?;
                        }
                        Some(Command::ResumeAgent) => {
                            self.session.resume_agent(Instant::now())?;
                        }
                        Some(Command::SetAgentFov { vertical_angle }) => {
                            self.session.set_agent_fov(vertical_angle, Instant::now())?;
                        }
                        Some(Command::SetAgentSize { height, width }) => {
                            self.session.set_agent_size(height, width, Instant::now())?;
                        }
                        Some(Command::ReleaseScriptControls) => {
                            self.session.release_script_controls(Instant::now())?;
                        }
                        Some(Command::SendGroupNotice { group_id, subject, message, attachment }) => {
                            self.session.send_group_notice(group_id, &subject, &message, attachment, Instant::now())?;
                        }
                        Some(Command::RequestGroupAccountSummary { group_id, request_id, interval_days, current_interval }) => {
                            self.session.request_group_account_summary(group_id, request_id, interval_days, current_interval, Instant::now())?;
                        }
                        Some(Command::RequestGroupAccountDetails { group_id, request_id, interval_days, current_interval }) => {
                            self.session.request_group_account_details(group_id, request_id, interval_days, current_interval, Instant::now())?;
                        }
                        Some(Command::RequestGroupAccountTransactions { group_id, request_id, interval_days, current_interval }) => {
                            self.session.request_group_account_transactions(group_id, request_id, interval_days, current_interval, Instant::now())?;
                        }
                        Some(Command::RequestGroupActiveProposals { group_id, transaction_id }) => {
                            self.session.request_group_active_proposals(group_id, transaction_id, Instant::now())?;
                        }
                        Some(Command::RequestGroupVoteHistory { group_id, transaction_id }) => {
                            self.session.request_group_vote_history(group_id, transaction_id, Instant::now())?;
                        }
                        Some(Command::StartGroupProposal { group_id, quorum, majority, duration, proposal_text }) => {
                            self.session.start_group_proposal(group_id, quorum, majority, duration, &proposal_text, Instant::now())?;
                        }
                        Some(Command::GroupProposalBallot { proposal_id, group_id, vote_cast }) => {
                            self.session.cast_group_proposal_ballot(proposal_id, group_id, &vote_cast, Instant::now())?;
                        }
                        Some(Command::ReplyScriptDialog { object_id, chat_channel, button_index, button_label }) => {
                            self.session.reply_script_dialog(object_id, chat_channel, button_index, &button_label, Instant::now())?;
                        }
                        Some(Command::AnswerScriptPermissions { task_id, item_id, permissions, experience_id }) => {
                            self.session.answer_script_permissions(task_id, item_id, permissions, experience_id, Instant::now())?;
                        }
                        Some(Command::RequestMuteList) => {
                            self.session.request_mute_list(Instant::now())?;
                        }
                        Some(Command::Mute { id, name, mute_type, flags }) => {
                            self.session.mute(id, &name, mute_type, flags, Instant::now())?;
                        }
                        Some(Command::Unmute { id, name }) => {
                            self.session.unmute(id, &name, Instant::now())?;
                        }
                        Some(Command::Teleport { region_handle, position, look_at }) => {
                            self.session.teleport_to(region_handle, position, look_at, Instant::now())?;
                        }
                        Some(Command::RequestRegionInfo) => {
                            self.session.request_region_info(Instant::now())?;
                        }
                        Some(Command::RequestAvatarNames(ids)) => {
                            self.session.request_avatar_names(&ids, Instant::now())?;
                        }
                        Some(Command::RequestGroupNames(ids)) => {
                            self.session.request_group_names(&ids, Instant::now())?;
                        }
                        Some(Command::RequestEnvironment { parcel_id }) => {
                            if let Some(base) = caps.get(CAP_EXT_ENVIRONMENT).cloned() {
                                let url = format!("{base}?parcelid={}", parcel_id.unwrap_or(-1));
                                tokio::spawn(get_caps_llsd(
                                    url,
                                    CAP_EXT_ENVIRONMENT,
                                    http.clone(),
                                    caps_tx.clone(),
                                ));
                            }
                        }
                        Some(Command::RequestMoneyBalance) => {
                            self.session.request_money_balance(Instant::now())?;
                        }
                        Some(Command::RequestEconomyData) => {
                            self.session.request_economy_data(Instant::now())?;
                        }
                        Some(Command::SendMoneyTransfer { dest, amount, kind, description }) => {
                            self.session.send_money_transfer(
                                dest, amount, kind, &description, Instant::now(),
                            )?;
                        }
                        Some(Command::RequestParcelProperties { west, south, east, north, sequence_id }) => {
                            self.session.request_parcel_properties(
                                west, south, east, north, sequence_id, Instant::now(),
                            )?;
                        }
                        Some(Command::RequestParcelPropertiesById { local_id, sequence_id }) => {
                            self.session.request_parcel_properties_by_id(local_id, sequence_id, Instant::now())?;
                        }
                        Some(Command::SetParcelOtherCleanTime { local_id, clean_time }) => {
                            self.session.set_parcel_other_clean_time(local_id, clean_time, Instant::now())?;
                        }
                        Some(Command::ModifyLand(edit)) => {
                            self.session.modify_land(&edit, Instant::now())?;
                        }
                        Some(Command::UndoLand) => {
                            self.session.undo_land(Instant::now())?;
                        }
                        Some(Command::SetDrawDistance(far)) => {
                            self.session.set_draw_distance(far);
                        }
                        Some(Command::RequestMapBlocks { min_x, max_x, min_y, max_y }) => {
                            self.session.request_map_blocks(min_x, max_x, min_y, max_y, Instant::now())?;
                        }
                        Some(Command::RequestMapByName { name }) => {
                            self.session.request_map_by_name(&name, Instant::now())?;
                        }
                        Some(Command::RequestMapItems { item_type, region_handle }) => {
                            self.session.request_map_items(item_type, region_handle, Instant::now())?;
                        }
                        Some(Command::RequestMapLayer) => {
                            self.session.request_map_layer(Instant::now())?;
                        }
                        Some(Command::SendAbuseReport(report)) => {
                            self.session.send_abuse_report(&report, Instant::now())?;
                        }
                        Some(Command::SendAbuseReportViaCaps { mut report, screenshot }) => {
                            // With a snapshot and the screenshot cap available, upload
                            // the snapshot over the two-step uploader (filling
                            // `screenshot_id` with a fresh texture asset id) and POST
                            // the report referencing it; otherwise the plain path.
                            match caps
                                .get(CAP_SEND_USER_REPORT_WITH_SCREENSHOT)
                                .cloned()
                                .zip(screenshot.filter(|bytes| !bytes.is_empty()))
                            {
                                Some((url, bytes)) => {
                                    if report.screenshot_id.is_nil() {
                                        report.screenshot_id = Uuid::new_v4();
                                    }
                                    let body = build_send_user_report(&report);
                                    tokio::spawn(run_report_screenshot_upload(
                                        url,
                                        body,
                                        bytes,
                                        http.clone(),
                                    ));
                                }
                                None => {
                                    if let Some(url) = caps.get(CAP_SEND_USER_REPORT).cloned() {
                                        let body = build_send_user_report(&report);
                                        tokio::spawn(post_caps_oneway(url, body, http.clone()));
                                    }
                                }
                            }
                        }
                        Some(Command::SendPostcard(postcard)) => {
                            self.session.send_postcard(&postcard, Instant::now())?;
                        }
                        Some(Command::RequestObjects { local_ids }) => {
                            self.session.request_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::RequestObjectProperties { local_ids }) => {
                            self.session.request_object_properties(&local_ids, Instant::now())?;
                        }
                        Some(Command::DeselectObjects { local_ids }) => {
                            self.session.deselect_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::TouchObject { local_id }) => {
                            self.session.touch_object(local_id, Instant::now())?;
                        }
                        Some(Command::GrabObject { local_id, grab_offset }) => {
                            self.session.grab_object(local_id, grab_offset, Instant::now())?;
                        }
                        Some(Command::GrabObjectUpdate { object_id, grab_offset_initial, grab_position, time_since_last }) => {
                            self.session.grab_object_update(object_id, grab_offset_initial, grab_position, time_since_last, Instant::now())?;
                        }
                        Some(Command::DegrabObject { local_id }) => {
                            self.session.degrab_object(local_id, Instant::now())?;
                        }
                        Some(Command::RezObject { shape, group_id }) => {
                            self.session.rez_object(&shape, group_id, Instant::now())?;
                        }
                        Some(Command::DuplicateObjects { local_ids, offset, group_id }) => {
                            self.session.duplicate_objects(&local_ids, offset, group_id, Instant::now())?;
                        }
                        Some(Command::DeleteObjects { local_ids }) => {
                            self.session.delete_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::DerezObjects { local_ids, destination, transaction_id, group_id }) => {
                            self.session.derez_objects(&local_ids, destination, transaction_id, group_id, Instant::now())?;
                        }
                        Some(Command::UpdateObject { local_id, transform }) => {
                            self.session.update_object(local_id, &transform, Instant::now())?;
                        }
                        Some(Command::SetObjectName { local_id, name }) => {
                            self.session.set_object_name(local_id, &name, Instant::now())?;
                        }
                        Some(Command::SetObjectDescription { local_id, description }) => {
                            self.session.set_object_description(local_id, &description, Instant::now())?;
                        }
                        Some(Command::SetObjectClickAction { local_id, action }) => {
                            self.session.set_object_click_action(local_id, action, Instant::now())?;
                        }
                        Some(Command::SetObjectMaterial { local_id, material }) => {
                            self.session.set_object_material(local_id, material, Instant::now())?;
                        }
                        Some(Command::SetObjectFlags { local_id, flags }) => {
                            self.session.set_object_flags(local_id, &flags, Instant::now())?;
                        }
                        Some(Command::SetObjectShape { local_id, shape }) => {
                            self.session.set_object_shape(local_id, &shape, Instant::now())?;
                        }
                        Some(Command::SetObjectImage { local_id, media_url, texture_entry }) => {
                            self.session.set_object_image(local_id, media_url.as_deref(), &texture_entry, Instant::now())?;
                        }
                        Some(Command::SetObjectExtraParams { local_id, params }) => {
                            self.session.set_object_extra_params(local_id, &params, Instant::now())?;
                        }
                        Some(Command::SetObjectGroup { local_ids, group_id }) => {
                            self.session.set_object_group(&local_ids, group_id, Instant::now())?;
                        }
                        Some(Command::SetObjectPermissions { local_ids, field, set, mask }) => {
                            self.session.set_object_permissions(&local_ids, field, set, mask, Instant::now())?;
                        }
                        Some(Command::SetObjectForSale { local_id, sale_type, sale_price }) => {
                            self.session.set_object_for_sale(local_id, sale_type, sale_price, Instant::now())?;
                        }
                        Some(Command::SetObjectCategory { local_id, category }) => {
                            self.session.set_object_category(local_id, category, Instant::now())?;
                        }
                        Some(Command::SetObjectIncludeInSearch { local_id, include }) => {
                            self.session.set_object_include_in_search(local_id, include, Instant::now())?;
                        }
                        Some(Command::LinkObjects { local_ids }) => {
                            self.session.link_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::DelinkObjects { local_ids }) => {
                            self.session.delink_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::UpdateParcel(update)) => {
                            self.session.update_parcel(&update, Instant::now())?;
                        }
                        Some(Command::RequestParcelAccessList { local_id, scope }) => {
                            self.session.request_parcel_access_list(local_id, scope, Instant::now())?;
                        }
                        Some(Command::UpdateParcelAccessList { local_id, scope, entries }) => {
                            // A fresh transaction id per update, so the simulator
                            // clears the old entries before applying ours rather
                            // than appending (see `update_parcel_access_list`).
                            self.session.update_parcel_access_list(local_id, scope, &entries, Uuid::new_v4(), Instant::now())?;
                        }
                        Some(Command::RequestParcelDwell { local_id }) => {
                            self.session.request_parcel_dwell(local_id, Instant::now())?;
                        }
                        Some(Command::BuyParcel { local_id, price, area, group_id, is_group_owned }) => {
                            self.session.buy_parcel(local_id, price, area, group_id, is_group_owned, Instant::now())?;
                        }
                        Some(Command::ReturnParcelObjects { local_id, return_type, owner_ids, task_ids }) => {
                            self.session.return_parcel_objects(local_id, return_type, &owner_ids, &task_ids, Instant::now())?;
                        }
                        Some(Command::SelectParcelObjects { local_id, return_type, object_ids }) => {
                            self.session.select_parcel_objects(local_id, return_type, &object_ids, Instant::now())?;
                        }
                        Some(Command::DeedParcelToGroup { local_id, group_id }) => {
                            self.session.deed_parcel_to_group(local_id, group_id, Instant::now())?;
                        }
                        Some(Command::ReclaimParcel { local_id }) => {
                            self.session.reclaim_parcel(local_id, Instant::now())?;
                        }
                        Some(Command::ReleaseParcel { local_id }) => {
                            self.session.release_parcel(local_id, Instant::now())?;
                        }
                        Some(Command::JoinParcels { west, south, east, north }) => {
                            self.session.join_parcels(west, south, east, north, Instant::now())?;
                        }
                        Some(Command::DivideParcel { west, south, east, north }) => {
                            self.session.divide_parcel(west, south, east, north, Instant::now())?;
                        }
                        Some(Command::RequestParcelObjectOwners { local_id }) => {
                            self.session.request_parcel_object_owners(local_id, Instant::now())?;
                        }
                        Some(Command::BuyParcelPass { local_id }) => {
                            self.session.buy_parcel_pass(local_id, Instant::now())?;
                        }
                        Some(Command::DisableParcelObjects { local_id, return_type, owner_ids, task_ids }) => {
                            self.session.disable_parcel_objects(local_id, return_type, &owner_ids, &task_ids, Instant::now())?;
                        }
                        Some(Command::RequestParcelInfo { parcel_id }) => {
                            self.session.request_parcel_info(parcel_id, Instant::now())?;
                        }
                        Some(Command::RequestEstateInfo) => {
                            self.session.request_estate_info(Instant::now())?;
                        }
                        Some(Command::UpdateEstateAccess { delta, target }) => {
                            self.session.update_estate_access(delta, target, Instant::now())?;
                        }
                        Some(Command::KickEstateUser { target }) => {
                            self.session.kick_estate_user(target, Instant::now())?;
                        }
                        Some(Command::TeleportHomeUser { target }) => {
                            self.session.teleport_home_user(target, Instant::now())?;
                        }
                        Some(Command::TeleportHomeAllUsers) => {
                            self.session.teleport_home_all_users(Instant::now())?;
                        }
                        Some(Command::RestartRegion { seconds }) => {
                            self.session.restart_region(seconds, Instant::now())?;
                        }
                        Some(Command::SendEstateMessage { message }) => {
                            self.session.send_estate_message(&message, Instant::now())?;
                        }
                        Some(Command::SetRegionInfo(update)) => {
                            self.session.set_region_info(&update, Instant::now())?;
                        }
                        Some(Command::RequestEstateCovenant) => {
                            self.session.request_estate_covenant(Instant::now())?;
                        }
                        Some(Command::RequestTelehubInfo) => {
                            self.session.request_telehub_info(Instant::now())?;
                        }
                        Some(Command::ConnectTelehub { object_local_id }) => {
                            self.session.connect_telehub(object_local_id, Instant::now())?;
                        }
                        Some(Command::DisconnectTelehub) => {
                            self.session.disconnect_telehub(Instant::now())?;
                        }
                        Some(Command::AddTelehubSpawnPoint { object_local_id }) => {
                            self.session.add_telehub_spawn_point(object_local_id, Instant::now())?;
                        }
                        Some(Command::RemoveTelehubSpawnPoint { spawn_index }) => {
                            self.session.remove_telehub_spawn_point(spawn_index, Instant::now())?;
                        }
                        Some(Command::GodKickUser { target, reason }) => {
                            self.session.god_kick_user(target, &reason, Instant::now())?;
                        }
                        Some(Command::SendGodlikeMessage { method, params }) => {
                            let refs: Vec<&str> = params.iter().map(String::as_str).collect();
                            self.session.send_godlike_message(&method, &refs, Instant::now())?;
                        }
                        Some(Command::RequestTexture { texture_id, discard_level, priority }) => {
                            self.session.request_texture(texture_id, discard_level, priority, Instant::now())?;
                        }
                        Some(Command::FetchTexture { texture_id, discard_level }) => {
                            if let Some(url) = caps.get(CAP_GET_TEXTURE).cloned() {
                                tokio::spawn(fetch_texture_http(
                                    url, texture_id, discard_level, http.clone(), events.clone(),
                                ));
                            }
                        }
                        Some(Command::FetchMesh { mesh_id, byte_range }) => {
                            // GetMesh2 is preferred when offered; fall back to GetMesh.
                            if let Some(url) = caps.get(CAP_GET_MESH2).or_else(|| caps.get(CAP_GET_MESH)).cloned() {
                                tokio::spawn(fetch_mesh_http(url, mesh_id.uuid(), byte_range, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::FetchAsset { asset_id, asset_type, byte_range }) => {
                            if let Some(url) = caps.get(CAP_VIEWER_ASSET).cloned() {
                                tokio::spawn(fetch_asset_http(
                                    url, asset_id.uuid(), asset_type, byte_range, http.clone(), events.clone(),
                                ));
                            }
                        }
                        Some(Command::RequestWearables) => {
                            self.session.request_wearables(Instant::now())?;
                        }
                        Some(Command::SetWearing(wearables)) => {
                            self.session.set_wearing(&wearables, Instant::now())?;
                        }
                        Some(Command::SetAppearance { serial, size, texture_entry, visual_params, wearable_cache }) => {
                            self.session.set_appearance(serial, size, &texture_entry, &visual_params, &wearable_cache, Instant::now())?;
                        }
                        Some(Command::RequestCachedTextures { serial, slots }) => {
                            self.session.request_cached_textures(serial, &slots, Instant::now())?;
                        }
                        Some(Command::RequestServerAppearanceUpdate { cof_version }) => {
                            if let Some(url) = caps.get(CAP_UPDATE_AVATAR_APPEARANCE).cloned() {
                                tokio::spawn(request_server_appearance_update(
                                    url, cof_version, http.clone(), caps_tx.clone(),
                                ));
                            }
                        }
                        Some(Command::SetAnimations(animations)) => {
                            self.session.set_animations(&animations, Instant::now())?;
                        }
                        Some(Command::PlayAnimation(anim_id)) => {
                            self.session.play_animation(anim_id, Instant::now())?;
                        }
                        Some(Command::StopAnimation(anim_id)) => {
                            self.session.stop_animation(anim_id, Instant::now())?;
                        }
                        Some(Command::AttachObject { local_id, attachment_point, mode, rotation }) => {
                            self.session.attach_object(local_id, attachment_point, mode, &rotation, Instant::now())?;
                        }
                        Some(Command::DetachObjects { local_ids }) => {
                            self.session.detach_objects(&local_ids, Instant::now())?;
                        }
                        Some(Command::DropAttachments { local_ids }) => {
                            self.session.drop_attachments(&local_ids, Instant::now())?;
                        }
                        Some(Command::RemoveAttachment { attachment_point, item_id }) => {
                            self.session.remove_attachment(attachment_point, item_id, Instant::now())?;
                        }
                        Some(Command::RezAttachment(rez)) => {
                            self.session.rez_attachment(&rez, Instant::now())?;
                        }
                        Some(Command::RezAttachments { compound_id, detach, attachments }) => {
                            self.session.rez_attachments(compound_id, detach, &attachments, Instant::now())?;
                        }
                        Some(Command::ViewerEffect(effects)) => {
                            self.session.send_viewer_effect(&effects, Instant::now())?;
                        }
                        Some(Command::TrackAgent { prey_id }) => {
                            self.session.track_agent(prey_id, Instant::now())?;
                        }
                        Some(Command::FindAgent { hunter, prey }) => {
                            self.session.find_agent(hunter, prey, Instant::now())?;
                        }
                        Some(Command::DirFindQuery { query_id, query_text, flags, query_start }) => {
                            self.session.dir_find_query(query_id, &query_text, flags, query_start, Instant::now())?;
                        }
                        Some(Command::DirPlacesQuery { query_id, query_text, flags, category, sim_name, query_start }) => {
                            self.session.dir_places_query(query_id, &query_text, flags, category, &sim_name, query_start, Instant::now())?;
                        }
                        Some(Command::DirLandQuery { query_id, flags, search_type, price, area, query_start }) => {
                            self.session.dir_land_query(query_id, flags, search_type, price, area, query_start, Instant::now())?;
                        }
                        Some(Command::DirClassifiedQuery { query_id, query_text, flags, category, query_start }) => {
                            self.session.dir_classified_query(query_id, &query_text, flags, category, query_start, Instant::now())?;
                        }
                        Some(Command::AvatarPickerRequest { query_id, name }) => {
                            self.session.avatar_picker_request(query_id, &name, Instant::now())?;
                        }
                        Some(Command::PlacesQuery { query_id, transaction_id, query_text, flags, category, sim_name }) => {
                            self.session.places_query(query_id, transaction_id, &query_text, flags, category, &sim_name, Instant::now())?;
                        }
                        Some(Command::EventInfoRequest { event_id }) => {
                            self.session.event_info_request(event_id, Instant::now())?;
                        }
                        Some(Command::EventNotificationAddRequest { event_id }) => {
                            self.session.event_notification_add_request(event_id, Instant::now())?;
                        }
                        Some(Command::EventNotificationRemoveRequest { event_id }) => {
                            self.session.event_notification_remove_request(event_id, Instant::now())?;
                        }
                        Some(Command::BuyObject { group_id, category_id, objects }) => {
                            self.session.buy_object(group_id, category_id, &objects, Instant::now())?;
                        }
                        Some(Command::BuyObjectInventory { object_id, item_id, folder_id }) => {
                            self.session.buy_object_inventory(object_id, item_id, folder_id, Instant::now())?;
                        }
                        Some(Command::RequestPayPrice { object_id }) => {
                            self.session.request_pay_price(object_id, Instant::now())?;
                        }
                        Some(Command::RequestObjectPropertiesFamily { request_flags, object_id }) => {
                            self.session.request_object_properties_family(request_flags, object_id, Instant::now())?;
                        }
                        Some(Command::SpinObjectStart { object_id }) => {
                            self.session.spin_object_start(object_id, Instant::now())?;
                        }
                        Some(Command::SpinObjectUpdate { object_id, rotation }) => {
                            self.session.spin_object_update(object_id, rotation, Instant::now())?;
                        }
                        Some(Command::SpinObjectStop { object_id }) => {
                            self.session.spin_object_stop(object_id, Instant::now())?;
                        }
                        Some(Command::DuplicateObjectsOnRay {
                            local_ids, group_id, ray_start, ray_end, bypass_raycast,
                            ray_end_is_intersection, copy_centers, copy_rotates, ray_target_id,
                            duplicate_flags,
                        }) => {
                            self.session.duplicate_objects_on_ray(
                                &local_ids, group_id, ray_start, ray_end, bypass_raycast,
                                ray_end_is_intersection, copy_centers, copy_rotates, ray_target_id,
                                duplicate_flags, Instant::now(),
                            )?;
                        }
                        Some(Command::RezRestoreToWorld { item }) => {
                            self.session.rez_restore_to_world(&item, Instant::now())?;
                        }
                        Some(Command::RezObjectFromNotecard { rez }) => {
                            self.session.rez_object_from_notecard(&rez, Instant::now())?;
                        }
                        Some(Command::RezObjectFromInventory { params }) => {
                            self.session.rez_object_from_inventory(&params, Instant::now())?;
                        }
                        Some(Command::RezScript { target, params }) => {
                            self.session.rez_script(target, &params, Instant::now())?;
                        }
                        Some(Command::RevokeScriptPermissions { object_id, permissions }) => {
                            self.session.revoke_script_permissions(object_id, permissions, Instant::now())?;
                        }
                        Some(Command::QueryScriptPermissions) => {
                            // Local query: synthesize the snapshot from the session
                            // and surface it on the event stream (no wire send).
                            events.send(Event::ScriptPermissionState(
                                self.session.script_permission_state(),
                            )).await.ok();
                        }
                        Some(Command::DetachAttachmentIntoInventory { item_id }) => {
                            self.session.detach_attachment_into_inventory(item_id, Instant::now())?;
                        }
                        Some(Command::RequestTaskInventory { target }) => {
                            self.session.request_task_inventory(target, Instant::now())?;
                        }
                        Some(Command::FetchTaskInventory { target }) => {
                            self.session.fetch_task_inventory(target, Instant::now())?;
                        }
                        Some(Command::RequestXfer { filename }) => {
                            self.session.request_xfer(&filename, Instant::now())?;
                        }
                        Some(Command::UpdateTaskInventory { target, key, item }) => {
                            self.session.update_task_inventory(target, key, &item, Instant::now())?;
                        }
                        Some(Command::MoveTaskInventory { target, folder_id, item_id }) => {
                            self.session.move_task_inventory(target, folder_id, item_id, Instant::now())?;
                        }
                        Some(Command::RemoveTaskInventory { target, item_id }) => {
                            self.session.remove_task_inventory(target, item_id, Instant::now())?;
                        }
                        Some(Command::RequestScriptRunning { object_id, item_id }) => {
                            self.session.request_script_running(object_id, item_id, Instant::now())?;
                        }
                        Some(Command::SetScriptRunning { object_id, item_id, running }) => {
                            self.session.set_script_running(object_id, item_id, running, Instant::now())?;
                        }
                        Some(Command::ResetScript { object_id, item_id }) => {
                            self.session.reset_script(object_id, item_id, Instant::now())?;
                        }
                        Some(Command::UploadAsset { asset_type, .. }) if asset_type.is_script() => {
                            // Scripts must go through `UploadScript` so the
                            // simulator's compile result is surfaced; the generic
                            // create-with-body path would discard it.
                            events.send(Event::AssetUploadFailed {
                                reason: "scripts must be uploaded with UploadScript (create the item \
                                    with create_inventory_item first)".to_owned(),
                            }).await.ok();
                        }
                        Some(Command::UploadAsset {
                            folder_id, asset_type, inventory_type, name, description,
                            next_owner_mask, group_mask, everyone_mask, expected_upload_cost, data,
                        }) => {
                            // The modern CAPS uploader (the only upload path — the
                            // legacy UDP asset-upload fallback was dropped): needs
                            // both the region capability and a CAPS name for the
                            // asset and inventory classes.
                            let caps_upload = match (asset_type.caps_asset_name(), inventory_type.caps_name()) {
                                (Some(asset_name), Some(inv_name)) => caps
                                    .get(CAP_NEW_FILE_AGENT_INVENTORY)
                                    .cloned()
                                    .map(|url| (url, asset_name, inv_name)),
                                _ => None,
                            };
                            if let Some((url, asset_name, inv_name)) = caps_upload {
                                let body = build_new_file_agent_inventory_request(
                                    folder_id, asset_name, inv_name, &name, &description,
                                    next_owner_mask, group_mask, everyone_mask, expected_upload_cost,
                                );
                                tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                            } else {
                                events.send(Event::AssetUploadFailed {
                                    reason: "NewFileAgentInventory capability not available".to_owned(),
                                }).await.ok();
                            }
                        }
                        Some(Command::UploadBakedTexture { data }) => {
                            if let Some(url) = caps.get(CAP_UPLOAD_BAKED_TEXTURE).cloned() {
                                let body = build_upload_baked_texture_request();
                                tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                            } else {
                                events.send(Event::AssetUploadFailed {
                                    reason: "UploadBakedTexture capability not available".to_owned(),
                                }).await.ok();
                            }
                        }
                        Some(Command::UpdateInventoryAsset { item_id, asset_type, data }) => {
                            // `UpdatableAssetType::cap` is total — scripts (which
                            // need the compile-aware `UploadScript`) are excluded
                            // from this type by construction.
                            let cap = asset_type.cap();
                            if let Some(url) = caps.get(cap).cloned() {
                                let body = build_update_item_asset_request(item_id);
                                tokio::spawn(run_caps_upload(url, body, data, http.clone(), events.clone()));
                            } else {
                                events.send(Event::AssetUploadFailed {
                                    reason: format!("{cap} capability not available"),
                                }).await.ok();
                            }
                        }
                        Some(Command::UploadScript { location, target, source }) => {
                            // Choose the capability + request body by location; the
                            // completion carries the simulator's compile result.
                            let target_wire = target.to_wire();
                            let (cap, body, running) = match location {
                                ScriptUploadLocation::AgentInventory { item_id } => (
                                    CAP_UPDATE_SCRIPT_AGENT,
                                    build_update_script_agent_request(item_id, target_wire),
                                    None,
                                ),
                                ScriptUploadLocation::TaskInventory {
                                    task_id, item_id, running, experience,
                                } => (
                                    CAP_UPDATE_SCRIPT_TASK,
                                    build_update_script_task_request(
                                        task_id, item_id, running, target_wire, experience,
                                    ),
                                    Some(running),
                                ),
                            };
                            if let Some(url) = caps.get(cap).cloned() {
                                tokio::spawn(run_script_upload(
                                    url, body, source, running, http.clone(), events.clone(),
                                ));
                            } else {
                                events.send(Event::AssetUploadFailed {
                                    reason: format!("{cap} capability not available"),
                                }).await.ok();
                            }
                        }
                        Some(Command::RequestObjectMedia { object_id }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA).cloned() {
                                tokio::spawn(fetch_object_media(url, object_id, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetObjectMedia { object_id, faces }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA).cloned() {
                                let body = build_object_media_update_request(object_id, &faces);
                                tokio::spawn(post_object_media(url, body, http.clone()));
                            }
                        }
                        Some(Command::NavigateObjectMedia { object_id, face, url: media_url }) => {
                            if let Some(url) = caps.get(CAP_OBJECT_MEDIA_NAVIGATE).cloned() {
                                let body = build_object_media_navigate_request(object_id, face, &media_url);
                                tokio::spawn(post_object_media(url, body, http.clone()));
                            }
                        }
                        Some(Command::RequestRenderMaterials { material_ids }) => {
                            if let Some(url) = caps.get(CAP_RENDER_MATERIALS).cloned() {
                                tokio::spawn(fetch_render_materials(url, material_ids, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::ModifyMaterialParams { updates }) => {
                            if let Some(url) = caps.get(CAP_MODIFY_MATERIAL_PARAMS).cloned() {
                                let body = build_modify_material_params_request(&updates);
                                tokio::spawn(post_modify_material_params(url, body, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestVoiceAccount { request }) => {
                            if let Some(url) = caps.get(CAP_PROVISION_VOICE_ACCOUNT).cloned() {
                                let body = build_provision_voice_account_request(&request);
                                tokio::spawn(post_voice_cap(url, body, CAP_PROVISION_VOICE_ACCOUNT, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestParcelVoiceInfo) => {
                            if let Some(url) = caps.get(CAP_PARCEL_VOICE_INFO).cloned() {
                                let body = build_parcel_voice_info_request();
                                tokio::spawn(post_voice_cap(url, body, CAP_PARCEL_VOICE_INFO, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SendVoiceSignaling { viewer_session, candidates, completed }) => {
                            if let Some(url) = caps.get(CAP_VOICE_SIGNALING).cloned() {
                                let body = build_voice_signaling_request(&viewer_session, &candidates, completed);
                                tokio::spawn(post_voice_signaling(url, body, http.clone()));
                            }
                        }
                        Some(Command::RequestDisplayNames(agent_ids)) => {
                            if let Some(base) = caps.get(CAP_GET_DISPLAY_NAMES).cloned() {
                                let agent_uuids: Vec<Uuid> =
                                    agent_ids.iter().map(AgentKey::uuid).collect();
                                let url = format!("{base}{}", display_names_query(&agent_uuids));
                                tokio::spawn(get_caps_llsd(url, CAP_GET_DISPLAY_NAMES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestRemoteParcelId { location, region_id, region_handle }) => {
                            if let Some(url) = caps.get(CAP_REMOTE_PARCEL_REQUEST).cloned() {
                                let body =
                                    build_remote_parcel_request(location, region_id, region_handle);
                                tokio::spawn(post_voice_cap(url, body, CAP_REMOTE_PARCEL_REQUEST, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestSimulatorFeatures) => {
                            if let Some(url) = caps.get(CAP_SIMULATOR_FEATURES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_SIMULATOR_FEATURES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestAgentPreferences) => {
                            if let Some(url) = caps.get(CAP_AGENT_PREFERENCES).cloned() {
                                let body = build_agent_preferences_request(&AgentPreferences::default());
                                tokio::spawn(post_voice_cap(url, body, CAP_AGENT_PREFERENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetAgentPreferences(prefs)) => {
                            if let Some(url) = caps.get(CAP_AGENT_PREFERENCES).cloned() {
                                let body = build_agent_preferences_request(&prefs);
                                tokio::spawn(post_voice_cap(url, body, CAP_AGENT_PREFERENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestObjectCost { object_ids }) => {
                            if let Some(url) = caps.get(CAP_GET_OBJECT_COST).cloned() {
                                let body = build_get_object_cost_request(&object_ids);
                                tokio::spawn(post_voice_cap(url, body, CAP_GET_OBJECT_COST, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestSelectedCost { object_ids, roots }) => {
                            if let Some(url) = caps.get(CAP_RESOURCE_COST_SELECTED).cloned() {
                                let kind = if roots { SelectedCostKind::Roots } else { SelectedCostKind::Prims };
                                let body = build_resource_cost_selected_request(kind, &object_ids);
                                tokio::spawn(post_voice_cap(url, body, CAP_RESOURCE_COST_SELECTED, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestObjectPhysicsData { object_ids }) => {
                            if let Some(url) = caps.get(CAP_GET_OBJECT_PHYSICS_DATA).cloned() {
                                let body = build_get_object_physics_data_request(&object_ids);
                                tokio::spawn(post_voice_cap(url, body, CAP_GET_OBJECT_PHYSICS_DATA, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestAttachmentResources) => {
                            if let Some(url) = caps.get(CAP_ATTACHMENT_RESOURCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_ATTACHMENT_RESOURCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestLandResources { parcel_id }) => {
                            if let Some(url) = caps.get(CAP_LAND_RESOURCES).cloned() {
                                tokio::spawn(fetch_land_resources(url, parcel_id, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestLandStat { report_type, request_flags, filter, parcel_local_id }) => {
                            self.session.request_land_stat(report_type, request_flags, &filter, parcel_local_id, Instant::now())?;
                        }
                        Some(Command::RequestExperienceInfo { experience_ids }) => {
                            if let Some(base) = caps.get(CAP_GET_EXPERIENCE_INFO).cloned() {
                                let url = format!("{base}{}", experience_info_query(&experience_ids));
                                tokio::spawn(get_caps_llsd(url, CAP_GET_EXPERIENCE_INFO, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::FindExperiences { query, page }) => {
                            if let Some(base) = caps.get(CAP_FIND_EXPERIENCE_BY_NAME).cloned() {
                                let url = format!("{base}{}", find_experience_query(&query, page));
                                tokio::spawn(get_caps_llsd(url, CAP_FIND_EXPERIENCE_BY_NAME, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestExperiencePermissions) => {
                            if let Some(url) = caps.get(CAP_GET_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetExperiencePermission { experience_id, permission }) => {
                            if let Some(base) = caps.get(CAP_EXPERIENCE_PREFERENCES).cloned() {
                                if permission.is_forget() {
                                    let url = format!("{base}{}", forget_experience_query(experience_id));
                                    tokio::spawn(delete_caps_llsd(url, CAP_EXPERIENCE_PREFERENCES, http.clone(), caps_tx.clone()));
                                } else {
                                    let body = build_set_experience_permission_request(experience_id, permission);
                                    tokio::spawn(put_caps_llsd(base, body, CAP_EXPERIENCE_PREFERENCES, http.clone(), caps_tx.clone()));
                                }
                            }
                        }
                        Some(Command::RequestOwnedExperiences) => {
                            if let Some(url) = caps.get(CAP_AGENT_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_AGENT_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestAdminExperiences) => {
                            if let Some(url) = caps.get(CAP_GET_ADMIN_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_ADMIN_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestCreatorExperiences) => {
                            if let Some(url) = caps.get(CAP_GET_CREATOR_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_GET_CREATOR_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestGroupExperiences { group_id }) => {
                            if let Some(base) = caps.get(CAP_GROUP_EXPERIENCES).cloned() {
                                let url = format!("{base}{}", group_experiences_query(group_id.uuid()));
                                tokio::spawn(fetch_group_experiences(url, group_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::RequestExperienceAdmin { experience_id }) => {
                            if let Some(base) = caps.get(CAP_IS_EXPERIENCE_ADMIN).cloned() {
                                let url = format!("{base}{}", experience_id_query(experience_id));
                                tokio::spawn(fetch_experience_admin(url, experience_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::RequestExperienceContributor { experience_id }) => {
                            if let Some(base) = caps.get(CAP_IS_EXPERIENCE_CONTRIBUTOR).cloned() {
                                let url = format!("{base}{}", experience_id_query(experience_id));
                                tokio::spawn(fetch_experience_contributor(url, experience_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::UpdateExperience { update }) => {
                            if let Some(url) = caps.get(CAP_UPDATE_EXPERIENCE).cloned() {
                                let body = build_update_experience_request(&update);
                                tokio::spawn(post_voice_cap(url, body, CAP_UPDATE_EXPERIENCE, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::RequestRegionExperiences) => {
                            if let Some(url) = caps.get(CAP_REGION_EXPERIENCES).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_REGION_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::SetRegionExperiences { allowed, blocked, trusted }) => {
                            if let Some(url) = caps.get(CAP_REGION_EXPERIENCES).cloned() {
                                let body = build_region_experiences_request(&allowed, &blocked, &trusted);
                                tokio::spawn(post_voice_cap(url, body, CAP_REGION_EXPERIENCES, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::OfferTeleport { targets, message }) => {
                            self.session.offer_teleport(&targets, &message, Instant::now())?;
                        }
                        Some(Command::AcceptTeleportLure { lure_id }) => {
                            self.session.accept_teleport_lure(lure_id, Instant::now())?;
                        }
                        Some(Command::DeclineTeleportLure { from_agent_id, lure_id }) => {
                            self.session.decline_teleport_lure(from_agent_id, lure_id, Instant::now())?;
                        }
                        Some(Command::RequestTeleport { to_agent_id, message }) => {
                            self.session.request_teleport(to_agent_id, &message, Instant::now())?;
                        }
                        Some(Command::GiveInventory { to_agent_id, item_id, asset_type, item_name, transaction_id }) => {
                            self.session.give_inventory(to_agent_id, item_id, asset_type, &item_name, transaction_id, Instant::now())?;
                        }
                        Some(Command::GiveInventoryFolder { to_agent_id, folder_id, folder_name, transaction_id }) => {
                            self.session.give_inventory_folder(to_agent_id, folder_id, &folder_name, transaction_id, Instant::now())?;
                        }
                        Some(Command::AcceptInventoryOffer { offer, folder_id }) => {
                            self.session.accept_inventory_offer(&offer, folder_id, Instant::now())?;
                        }
                        Some(Command::DeclineInventoryOffer { offer, trash_folder_id }) => {
                            self.session.decline_inventory_offer(&offer, trash_folder_id, Instant::now())?;
                        }
                        Some(Command::StartConference { session_id, invitees, message }) => {
                            self.session.start_conference(session_id, &invitees, &message, Instant::now())?;
                        }
                        Some(Command::SendConferenceMessage { session_id, message }) => {
                            self.session.send_conference_message(session_id, &message, Instant::now())?;
                            if let Some(own) = self.session.agent_id() {
                                let name = self.session.agent_legacy_name();
                                let roster: std::collections::BTreeSet<_> = self
                                    .session
                                    .participants(ChatSessionKind::Conference { id: session_id })
                                    .collect();
                                chat_log.log_conference(session_id, &roster, own, &name, &message);
                            }
                        }
                        Some(Command::LeaveConference { session_id }) => {
                            self.session.leave_conference(session_id, Instant::now())?;
                        }
                        Some(Command::MarkSessionRead { session }) => {
                            self.session.mark_session_read(session);
                        }
                        Some(Command::AcceptChatInvite { session_id, from_group }) => {
                            // Promote the registry entry to joined, then drive the
                            // modern accept over the cap when present (its reply
                            // roster seeds the participants); on a grid without the
                            // cap the optimistic local join is the whole accept.
                            self.session.accept_chat_invite(session_id, from_group, Instant::now());
                            if let Some(url) = caps.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                                let body = chat_session_request_body(CHAT_SESSION_ACCEPT, session_id.get());
                                tokio::spawn(post_chat_session_request(url, body, session_id.get(), from_group, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::DeclineChatInvite { session_id, from_group }) => {
                            // Remove the registry entry, then refuse on the wire:
                            // the cap `decline invitation` POST when present, else
                            // a UDP `SessionLeave` as the OpenSim fallback.
                            self.session.decline_chat_invite(session_id, from_group, Instant::now());
                            if let Some(url) = caps.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                                let body = chat_session_request_body(CHAT_SESSION_DECLINE, session_id.get());
                                tokio::spawn(post_chat_session_request(url, body, session_id.get(), from_group, http.clone(), caps_tx.clone()));
                            } else if from_group {
                                self.session.leave_group_session(GroupKey::from(session_id.get()), Instant::now())?;
                            } else {
                                self.session.leave_conference(session_id, Instant::now())?;
                            }
                        }
                        Some(Command::JoinSessionVoice { session }) => {
                            // Optimistic local join, then drive the signalling: ensure
                            // a voice account, then signal into the session's channel
                            // over `ChatSessionRequest` (accept invitation). Signalling
                            // only — the audio session is the caller's concern.
                            self.session.join_session_voice(session, Instant::now());
                            if let Some(own) = self.session.agent_id() {
                                let session_uuid = session.canonical_session_id(own);
                                let from_group = matches!(session, ChatSessionKind::Group { .. });
                                if let Some(url) = caps.get(CAP_PROVISION_VOICE_ACCOUNT).cloned() {
                                    let body = build_provision_voice_account_request(&VoiceProvisionRequest::vivox());
                                    tokio::spawn(post_voice_cap(url, body, CAP_PROVISION_VOICE_ACCOUNT, http.clone(), caps_tx.clone()));
                                }
                                if let Some(url) = caps.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                                    let body = chat_session_request_body(CHAT_SESSION_ACCEPT, session_uuid);
                                    tokio::spawn(post_chat_session_request(url, body, session_uuid, from_group, http.clone(), caps_tx.clone()));
                                }
                            }
                        }
                        Some(Command::LeaveSessionVoice { session }) => {
                            // Optimistic local leave (keeps the text conversation),
                            // then signal the voice decline on the wire: a 1:1 P2P
                            // call uses `decline p2p voice`, a group / conference the
                            // multi-agent `decline invitation`.
                            self.session.leave_session_voice(session);
                            if let Some(own) = self.session.agent_id() {
                                let session_uuid = session.canonical_session_id(own);
                                let from_group = matches!(session, ChatSessionKind::Group { .. });
                                let method = if matches!(session, ChatSessionKind::Direct { .. }) {
                                    CHAT_SESSION_DECLINE_P2P_VOICE
                                } else {
                                    CHAT_SESSION_DECLINE
                                };
                                if let Some(url) = caps.get(CAP_CHAT_SESSION_REQUEST).cloned() {
                                    let body = chat_session_request_body(method, session_uuid);
                                    tokio::spawn(post_chat_session_request(url, body, session_uuid, from_group, http.clone(), caps_tx.clone()));
                                }
                            }
                        }
                        Some(Command::QueryChatSessions) => {
                            // Local query: build the light session list and surface
                            // it on the event stream (no wire send).
                            events.send(Event::ChatSessions(
                                self.session.chat_sessions_info().collect(),
                            )).await.ok();
                        }
                        Some(Command::QueryChatHistoryPage { session, before, limit }) => {
                            // Newest-first paging across the unified memory→archive
                            // view: serve the in-memory ring first, then continue
                            // older pages from the on-disk transcript (B9).
                            let consumed = before.map_or(0, MessageCursor::consumed_count);
                            let mem_len = self.session.history_len(session);
                            let (messages, prev): (std::sync::Arc<[SessionMessage]>, _) =
                                if consumed < mem_len {
                                    let (page, mem_prev) =
                                        self.session.history_page(session, before, limit);
                                    let collected: std::sync::Arc<[_]> = page.cloned().collect();
                                    let next = consumed.saturating_add(collected.len());
                                    // When the ring is exhausted, hand a file cursor
                                    // if the transcript holds anything older.
                                    let prev = mem_prev.or_else(|| {
                                        chat_log
                                            .read_older_page(session, mem_len, next, 1)
                                            .filter(|(probe, _)| !probe.is_empty())
                                            .map(|_more| MessageCursor::from_consumed(next))
                                    });
                                    (collected, prev)
                                } else {
                                    match chat_log.read_older_page(session, mem_len, consumed, limit) {
                                        Some((msgs, prev)) => (msgs.into(), prev),
                                        None => (Vec::new().into(), None),
                                    }
                                };
                            events.send(Event::ChatHistoryPage { session, messages, prev }).await.ok();
                        }
                        Some(Command::QueryInventoryFolder { folder, before, limit }) => {
                            // Local query: page the held model into owning view
                            // types (one bounded borrow→owned transform, the
                            // payload `Arc<[…]>` so re-handoff is a refcount
                            // bump). A bevy reader may instead borrow the Session
                            // and call `inventory_folder_page` directly.
                            let (folders, items, prev) =
                                self.session.inventory_folder_page(folder, before, limit);
                            // On-demand: a query for an unfetched folder schedules
                            // its fetch so a later query sees the contents (works
                            // regardless of the background-crawl flag).
                            if self.session.folder_fetch_state(folder)
                                == Some(FolderState::Unknown)
                            {
                                fetch_folder_contents(
                                    &mut self.session,
                                    folder,
                                    &caps,
                                    &http,
                                    &caps_tx,
                                    Instant::now(),
                                )
                                .ok();
                            }
                            events.send(Event::InventoryFolderPage {
                                folder,
                                folders: folders.into(),
                                items: items.into(),
                                prev,
                            }).await.ok();
                        }
                        Some(Command::QueryInventoryRoots) => {
                            // Local query: surface the agent + library roots (both
                            // `Copy` keys, no `Arc` needed).
                            events.send(Event::InventoryRoots {
                                agent_root: self.session.inventory_root(),
                                library_root: self.session.library_root(),
                            }).await.ok();
                        }
                        Some(Command::QueryInventoryFolders) => {
                            // Local query: snapshot the agent tree's known folders
                            // (seeded from the login skeleton, so present before any
                            // contents fetch). `Arc` so the clone across the channel
                            // is cheap regardless of tree size.
                            let folders: std::sync::Arc<[FolderInfo]> =
                                self.session.inventory_folder_infos().into();
                            events.send(Event::InventoryFolders(folders)).await.ok();
                        }
                        Some(Command::QueryFriends) => {
                            // Local query: build the buddy snapshot with online flags.
                            events.send(Event::FriendsSnapshot(
                                self.session.friends_presence().collect(),
                            )).await.ok();
                        }
                        Some(Command::RetrieveInstantMessages) => {
                            self.session.retrieve_instant_messages(Instant::now())?;
                        }
                        Some(Command::RequestOfflineMessages) => {
                            if let Some(url) = caps.get(CAP_READ_OFFLINE_MSGS).cloned() {
                                tokio::spawn(get_caps_llsd(url, CAP_READ_OFFLINE_MSGS, http.clone(), caps_tx.clone()));
                            }
                        }
                        Some(Command::TeleportViaLandmark { landmark }) => {
                            self.session.teleport_via_landmark(landmark, Instant::now())?;
                        }
                        Some(Command::CancelTeleport) => {
                            self.session.cancel_teleport(Instant::now())?;
                        }
                        Some(Command::SetStartLocation { slot, position, look_at }) => {
                            self.session.set_start_location(slot, position, look_at, Instant::now())?;
                        }
                        Some(Command::RequestAgentDataUpdate) => {
                            self.session.request_agent_data_update(Instant::now())?;
                        }
                        Some(Command::QuitCopy) => {
                            self.session.quit_copy(Instant::now())?;
                        }
                        Some(Command::SetVelocityInterpolation { enabled }) => {
                            self.session.set_velocity_interpolation(enabled, Instant::now())?;
                        }
                        Some(Command::RequestUserInfo) => {
                            self.session.request_user_info(Instant::now())?;
                        }
                        Some(Command::UpdateUserInfo { im_via_email, directory_visibility }) => {
                            self.session.update_user_info(im_via_email, directory_visibility, Instant::now())?;
                        }
                        Some(Command::TriggerSound { sound, gain, region_handle, position }) => {
                            self.session.trigger_sound(sound, gain, region_handle, position, Instant::now())?;
                        }
                        Some(Command::RequestGodlikePowers { godlike }) => {
                            self.session.request_godlike_powers(godlike, Instant::now())?;
                        }
                        Some(Command::EjectUser { target, action }) => {
                            self.session.eject_user(target, action, Instant::now())?;
                        }
                        Some(Command::FreezeUser { target, action }) => {
                            self.session.freeze_user(target, action, Instant::now())?;
                        }
                        Some(Command::SimWideDeletes { owner, flags }) => {
                            self.session.sim_wide_deletes(owner, flags, Instant::now())?;
                        }
                        Some(Command::GodUpdateRegionInfo { update }) => {
                            self.session.god_update_region_info(&update, Instant::now())?;
                        }
                        Some(Command::ParcelGodForceOwner { parcel, owner }) => {
                            self.session.parcel_god_force_owner(parcel, owner, Instant::now())?;
                        }
                        Some(Command::ParcelGodMarkAsContent { parcel }) => {
                            self.session.parcel_god_mark_as_content(parcel, Instant::now())?;
                        }
                        Some(Command::EventGodDelete { event, query_id, query_text, flags, query_start }) => {
                            self.session.event_god_delete(event, query_id, &query_text, flags, query_start, Instant::now())?;
                        }
                        Some(Command::StateSave { filename }) => {
                            self.session.state_save(&filename, Instant::now())?;
                        }
                        Some(Command::ViewerStartAuction { parcel, snapshot }) => {
                            self.session.viewer_start_auction(parcel, snapshot, Instant::now())?;
                        }
                        Some(Command::Logout) | None => {
                            self.session.initiate_logout(Instant::now());
                        }
                    }
                }
                () = &mut sleep => {
                    self.session.handle_timeout(Instant::now());
                }
            }
        }
    }
}
