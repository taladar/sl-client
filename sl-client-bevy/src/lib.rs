#![doc = include_str!("../README.md")]

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

use bevy::prelude::*;
use reqwest::blocking::Client as ReqwestBlockingClient;

use std::collections::HashMap;

use sl_proto::{
    AssetUploadResponse, CAP_FETCH_INVENTORY, CAP_GET_ASSET, CAP_GET_MESH, CAP_GET_MESH2,
    CAP_GET_TEXTURE, CAP_GROUP_MEMBER_DATA, CAP_MODIFY_MATERIAL_PARAMS,
    CAP_NEW_FILE_AGENT_INVENTORY, CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE,
    CAP_RENDER_MATERIALS, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPLOAD_BAKED_TEXTURE,
    Event as SessionEvent, Llsd, LoginResponse, REQUESTED_CAPABILITIES, Session,
    build_event_queue_request, build_fetch_inventory_request, build_group_member_data_request,
    build_modify_material_params_request, build_new_file_agent_inventory_request,
    build_object_media_get_request, build_object_media_navigate_request,
    build_object_media_update_request, build_render_materials_request, build_seed_request,
    build_update_avatar_appearance_request, build_update_item_asset_request,
    build_upload_baked_texture_request, j2c, parse_asset_upload_response,
    parse_event_queue_response, parse_llsd_xml, parse_login_response,
    parse_render_materials_response, parse_seed_response,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    ActiveGroup, AnyMessage, AvatarGroupMembership, AvatarInterests, AvatarPick, AvatarProperties,
    ChatAudible, ChatMessage, ChatSourceType, ChatType, ClickAction, ControlFlags,
    CreateGroupParams, DeRezDestination, DisconnectReason, EconomyData, EstateAccessDelta,
    EstateAccessKind, EstateInfo, ExtendedMesh, FlexibleData, Friend, FriendRights,
    GltfMaterialOverride, GroupMember, GroupMembership, GroupNotice, GroupProfile, GroupRole,
    GroupRoleMember, GroupTitle, ImDialog, InstantMessage, InventoryFolder, InventoryItem,
    InventoryType, LegacyMaterial, LightData, LightImage, LindenAmount, LoadUrlRequest,
    LoginParams, LoginRequest, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP,
    MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MapItem, MapItemType, MapRegionInfo, Material,
    MaterialOverrideUpdate, Maturity, MediaEntry, MfaChallenge, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NeighborInfo, Object, ObjectExtraParams,
    ObjectFlagSettings, ObjectMediaResponse, ObjectMotion, ObjectProperties, ObjectTransform,
    ParcelAccessEntry, ParcelAccessScope, ParcelCategory, ParcelFlags, ParcelInfo,
    ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelOverlayInfo, ParcelReturnType, ParcelUpdate,
    PermissionField, PlayingAnimation, PrimShape, ProductType, ReflectionProbe, RegionFlags,
    RegionIdentity, RegionInfoUpdate, RegionLimits, Reliability, RenderMaterialEntry,
    RenderMaterialRef, Rotation, SaleType, ScriptDialog, ScriptPermissionRequest,
    ScriptPermissions, ScriptTeleportRequest, SculptData, SoundFlags, SoundPreload,
    TerrainLayerType, TerrainPatch, TextureEntry, TextureFace, Throttle, Transmit, Uuid, Vector,
    Wearable, WearableType, avatar_texture, decode_texture_entry, grid_to_handle, handle_to_global,
    handle_to_grid, pcode, sim_access,
};
#[doc(no_inline)]
pub use sl_proto::{Asset, AssetType, ImageCodec, Texture, TransferStatus};
pub use sl_proto::{DisconnectReason as SessionDisconnectReason, Event as SlSessionEvent};

/// The maximum UDP datagram size we are prepared to receive.
const RECV_BUFFER_SIZE: usize = 0x1_0000;

/// How long to wait for a single CAPS event-queue long-poll before retrying.
const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// The Bevy plugin that drives a sans-I/O [`Session`] from ECS systems.
#[derive(Debug, Clone)]
pub struct SlClientPlugin {
    /// The login parameters used to start the session.
    pub params: LoginParams,
}

impl Plugin for SlClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SlEvent>()
            .add_event::<SlMfaChallenge>()
            .add_event::<SlCommand>()
            .insert_resource(SlConfig {
                params: self.params.clone(),
            })
            .add_systems(Startup, start_login)
            .add_systems(Update, drive);
    }
}

/// A high-level session event, emitted as a Bevy event.
#[derive(Event, Debug, Clone)]
pub struct SlEvent(pub SessionEvent);

/// Emitted when the grid requires a multi-factor one-time code. To answer it,
/// re-add the plugin with login parameters prepared via
/// `LoginRequest::with_mfa`.
#[derive(Event, Debug, Clone)]
pub struct SlMfaChallenge(pub MfaChallenge);

/// A command to a running session, sent as a Bevy event.
#[derive(Event, Debug)]
pub enum SlCommand {
    /// Send an application message.
    Send {
        /// The message to send.
        message: Box<AnyMessage>,
        /// How to deliver it.
        reliability: Reliability,
    },
    /// Send local chat via `ChatFromViewer`. Incoming chat arrives as an
    /// [`SlSessionEvent::ChatReceived`].
    Chat {
        /// The message text.
        message: String,
        /// The chat type (whisper / normal / shout / …).
        chat_type: ChatType,
        /// The chat channel (`0` for ordinary local chat).
        channel: i32,
    },
    /// Broadcast a local-chat typing indicator (`true` = start, `false` = stop).
    /// Other clients see it as an [`SlSessionEvent::ChatTyping`].
    Typing(bool),
    /// Send a direct (1:1) instant message. Incoming IMs arrive as an
    /// [`SlSessionEvent::InstantMessageReceived`].
    InstantMessage {
        /// The recipient's agent id.
        to_agent_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Send an instant-message typing indicator to `to_agent_id` (`true` = start,
    /// `false` = stop). Other clients see it as an [`SlSessionEvent::ImTyping`].
    ImTyping {
        /// The correspondent's agent id.
        to_agent_id: Uuid,
        /// Whether typing started (`true`) or stopped (`false`).
        typing: bool,
    },
    /// Set the agent control flags (movement); the simulator moves the agent
    /// accordingly. Pass [`ControlFlags::empty`] to stop.
    SetControls(ControlFlags),
    /// Set the per-category bandwidth throttle (`AgentThrottle`); the simulator
    /// allocates its UDP send budget accordingly. Re-sent on every region change.
    SetThrottle(Throttle),
    /// Set the agent's body and head rotation (facing/steering).
    SetRotation {
        /// The body rotation.
        body: Rotation,
        /// The head rotation.
        head: Rotation,
    },
    /// Stand the agent up (from sitting).
    Stand,
    /// Sit the agent on the ground where it stands.
    SitOnGround,
    /// Sit the agent on the object `target` at the region-local `offset`. The
    /// result arrives as an [`SlSessionEvent::SitResult`].
    Sit {
        /// The object to sit on.
        target: Uuid,
        /// The seat offset, in region-local metres.
        offset: Vector,
    },
    /// Walk the agent to the global coordinates `(global_x, global_y, z)` using
    /// the simulator's server-side autopilot.
    Autopilot {
        /// The global X coordinate, in metres.
        global_x: f64,
        /// The global Y coordinate, in metres.
        global_y: f64,
        /// The region-local height, in metres.
        z: f64,
    },
    /// Request an avatar's profile. Replies arrive as
    /// [`SlSessionEvent::AvatarProperties`], [`SlSessionEvent::AvatarInterests`],
    /// and [`SlSessionEvent::AvatarGroups`].
    RequestAvatarProperties(Uuid),
    /// Request an avatar's picks. The reply arrives as
    /// [`SlSessionEvent::AvatarPicks`].
    RequestAvatarPicks(Uuid),
    /// Request the agent's private notes about an avatar. The reply arrives as
    /// [`SlSessionEvent::AvatarNotes`].
    RequestAvatarNotes(Uuid),
    /// Request the contents (sub-folders and items) of an inventory folder over
    /// **UDP** (`FetchInventoryDescendents`). The reply arrives as
    /// [`SlSessionEvent::InventoryDescendents`]. The full folder skeleton arrives
    /// once at login as [`SlSessionEvent::InventorySkeleton`].
    RequestFolderContents(Uuid),
    /// Fetch the contents of one or more inventory folders over the **HTTP CAPS**
    /// path (`FetchInventoryDescendents2`) — the modern path used on Second Life.
    /// Each folder's contents arrive as an [`SlSessionEvent::InventoryDescendents`].
    FetchInventoryFolders(Vec<Uuid>),
    /// Set the friendship rights granted to a friend (`GrantUserRights`). The
    /// `rights` bitfield combines the [`FriendRights`] `CAN_*` flags. The change
    /// is echoed back as an [`SlSessionEvent::FriendRightsChanged`].
    GrantUserRights {
        /// The friend whose granted rights to set.
        target: Uuid,
        /// The new rights bitfield (combine `FriendRights::CAN_*`).
        rights: FriendRights,
    },
    /// Offer friendship to an agent (`ImprovedInstantMessage`,
    /// `IM_FRIENDSHIP_OFFERED`). The offer arrives at the recipient as an
    /// [`SlSessionEvent::InstantMessageReceived`] with
    /// [`ImDialog::FriendshipOffered`].
    OfferFriendship {
        /// The agent to offer friendship to.
        to_agent_id: Uuid,
        /// The offer message text.
        message: String,
    },
    /// End the friendship with an agent (`TerminateFriendship`).
    TerminateFriendship(Uuid),
    /// Accept a friendship offer (`AcceptFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`] of the incoming friendship-offer IM; the
    /// calling card goes into `calling_card_folder`.
    AcceptFriendship {
        /// The offer's transaction id (the friendship-offer IM's `id`).
        transaction_id: Uuid,
        /// The inventory folder to place the new calling card in.
        calling_card_folder: Uuid,
    },
    /// Decline a friendship offer (`DeclineFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`] of the incoming friendship-offer IM.
    DeclineFriendship(Uuid),
    /// Make a group the active group (`ActivateGroup`); nil clears it. Confirmed
    /// by [`SlSessionEvent::ActiveGroupChanged`].
    ActivateGroup(Uuid),
    /// Request a group's member roster over **UDP** (`GroupMembersRequest`).
    /// Replies arrive as [`SlSessionEvent::GroupMembers`].
    RequestGroupMembers(Uuid),
    /// Fetch a group's member roster over the **HTTP CAPS** path
    /// (`GroupMemberData`) — the modern path used on Second Life. The roster
    /// arrives as an [`SlSessionEvent::GroupMembers`].
    FetchGroupMembers(Uuid),
    /// Request a group's roles. The reply arrives as
    /// [`SlSessionEvent::GroupRoleData`].
    RequestGroupRoles(Uuid),
    /// Request a group's role↔member pairings. The reply arrives as
    /// [`SlSessionEvent::GroupRoleMembers`].
    RequestGroupRoleMembers(Uuid),
    /// Request the agent's selectable titles in a group. The reply arrives as
    /// [`SlSessionEvent::GroupTitles`].
    RequestGroupTitles(Uuid),
    /// Request a group's profile. The reply arrives as
    /// [`SlSessionEvent::GroupProfileReceived`].
    RequestGroupProfile(Uuid),
    /// Request a group's notice list. The reply arrives as
    /// [`SlSessionEvent::GroupNotices`].
    RequestGroupNotices(Uuid),
    /// Request a single group notice's full body (by notice id).
    RequestGroupNotice(Uuid),
    /// Create a new group. The result arrives as
    /// [`SlSessionEvent::CreateGroupResult`].
    CreateGroup(CreateGroupParams),
    /// Join an open-enrollment group. The result arrives as
    /// [`SlSessionEvent::JoinGroupResult`].
    JoinGroup(Uuid),
    /// Leave a group. The result arrives as [`SlSessionEvent::LeaveGroupResult`].
    LeaveGroup(Uuid),
    /// Invite agents to a group, each an `(invitee_id, role_id)` pair (nil role
    /// = the default Everyone role).
    InviteToGroup {
        /// The group to invite into.
        group_id: Uuid,
        /// The `(invitee_id, role_id)` pairs.
        invitees: Vec<(Uuid, Uuid)>,
    },
    /// Set whether the agent accepts notices from a group / lists it in profile.
    SetGroupAcceptNotices {
        /// The group.
        group_id: Uuid,
        /// Whether to accept notices.
        accept_notices: bool,
        /// Whether to list the group in the agent's profile.
        list_in_profile: bool,
    },
    /// Set the agent's L$ contribution to a group.
    SetGroupContribution {
        /// The group.
        group_id: Uuid,
        /// The new contribution amount.
        contribution: i32,
    },
    /// Start (join) a group's IM session (`IM_SESSION_GROUP_START`). Group
    /// messages then arrive as [`SlSessionEvent::GroupSessionMessage`].
    StartGroupSession(Uuid),
    /// Send a message into a group's IM session. Other members receive it as
    /// [`SlSessionEvent::GroupSessionMessage`].
    SendGroupMessage {
        /// The group (and IM session) to post to.
        group_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Leave a group's IM session (stop receiving its chat) without leaving the
    /// group itself.
    LeaveGroupSession(Uuid),
    /// Reply to a scripted-object dialog (`ScriptDialogReply`) from an
    /// [`SlSessionEvent::ScriptDialog`] — the chosen button on its hidden
    /// `chat_channel`.
    ReplyScriptDialog {
        /// The object that raised the dialog.
        object_id: Uuid,
        /// The dialog's hidden chat channel.
        chat_channel: i32,
        /// The chosen button index.
        button_index: i32,
        /// The chosen button label (or the typed text for an `llTextBox`).
        button_label: String,
    },
    /// Answer a scripted-object permission request (`ScriptAnswerYes`) from an
    /// [`SlSessionEvent::ScriptPermissionRequest`] — grants `permissions`
    /// ([`ScriptPermissions::default`] denies everything).
    AnswerScriptPermissions {
        /// The task (object) id holding the script.
        task_id: Uuid,
        /// The script item id.
        item_id: Uuid,
        /// The permissions to grant.
        permissions: ScriptPermissions,
    },
    /// Request the agent's mute (block) list (`MuteListRequest`). The list
    /// arrives as [`SlSessionEvent::MuteList`] (or
    /// [`SlSessionEvent::MuteListUnchanged`]).
    RequestMuteList,
    /// Mute (block) an entity (`UpdateMuteListEntry`).
    Mute {
        /// The muted entity's id (nil for a [`MuteType::ByName`] mute).
        id: Uuid,
        /// The muted entity's name.
        name: String,
        /// What kind of entity is muted.
        mute_type: MuteType,
        /// The per-aspect exception flags ([`MuteFlags::default`] mutes all).
        flags: MuteFlags,
    },
    /// Remove a mute (`RemoveMuteListEntry`); `id`/`name` must match the entry.
    Unmute {
        /// The muted entity's id.
        id: Uuid,
        /// The muted entity's name.
        name: String,
    },
    /// Teleport to `position` (region-local) in the region `region_handle`.
    Teleport {
        /// The destination region handle.
        region_handle: u64,
        /// The destination position within the region.
        position: Vector,
        /// The look-at direction on arrival.
        look_at: Vector,
    },
    /// Request the current region's info (agent/object limits).
    RequestRegionInfo,
    /// Request `ParcelProperties` for a metre rectangle (region-local).
    RequestParcelProperties {
        /// The western edge (metres).
        west: f32,
        /// The southern edge (metres).
        south: f32,
        /// The eastern edge (metres).
        east: f32,
        /// The northern edge (metres).
        north: f32,
        /// A sequence id echoed back in the reply for matching.
        sequence_id: i32,
    },
    /// Edit a parcel's settings (`ParcelPropertiesUpdate`).
    UpdateParcel(ParcelUpdate),
    /// Request a parcel's allow or ban list (`ParcelAccessListRequest`); the
    /// reply arrives as [`SlSessionEvent::ParcelAccessList`].
    RequestParcelAccessList {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which list to fetch (allow or ban).
        scope: ParcelAccessScope,
    },
    /// Replace a parcel's allow or ban list (`ParcelAccessListUpdate`); empty
    /// `entries` clears it.
    UpdateParcelAccessList {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which list to set (allow or ban).
        scope: ParcelAccessScope,
        /// The new entries.
        entries: Vec<ParcelAccessEntry>,
    },
    /// Request a parcel's dwell/traffic value (`ParcelDwellRequest`); the reply
    /// arrives as [`SlSessionEvent::ParcelDwell`].
    RequestParcelDwell {
        /// The parcel's region-local id.
        local_id: i32,
    },
    /// Buy a parcel (`ParcelBuy`).
    BuyParcel {
        /// The parcel's region-local id.
        local_id: i32,
        /// The agreed price in L$.
        price: i32,
        /// The parcel area in m².
        area: i32,
        /// The group to buy for (nil for a personal purchase).
        group_id: Uuid,
        /// Whether the purchase is group-owned.
        is_group_owned: bool,
    },
    /// Return objects on a parcel (`ParcelReturnObjects`).
    ReturnParcelObjects {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which objects to return (combine `ParcelReturnType` constants).
        return_type: ParcelReturnType,
        /// Optional owner-id scope.
        owner_ids: Vec<Uuid>,
        /// Optional explicit object/task-id scope.
        task_ids: Vec<Uuid>,
    },
    /// Select (highlight) objects on a parcel (`ParcelSelectObjects`).
    SelectParcelObjects {
        /// The parcel's region-local id.
        local_id: i32,
        /// Which objects to select (combine `ParcelReturnType` constants).
        return_type: ParcelReturnType,
        /// Explicit object ids (used with `ParcelReturnType::LIST`).
        object_ids: Vec<Uuid>,
    },
    /// Deed a parcel to a group (`ParcelDeedToGroup`).
    DeedParcelToGroup {
        /// The parcel's region-local id.
        local_id: i32,
        /// The group to deed the parcel to.
        group_id: Uuid,
    },
    /// Reclaim a parcel to the estate (`ParcelReclaim`).
    ReclaimParcel {
        /// The parcel's region-local id.
        local_id: i32,
    },
    /// Release (abandon) a parcel back to the estate (`ParcelRelease`).
    ReleaseParcel {
        /// The parcel's region-local id.
        local_id: i32,
    },
    /// Request the region's estate config + access lists (`getinfo`); replies
    /// arrive as [`SlSessionEvent::EstateInfo`] and [`SlSessionEvent::EstateAccessList`].
    RequestEstateInfo,
    /// Add/remove an agent or group on an estate access list (`estateaccessdelta`).
    UpdateEstateAccess {
        /// Which list change to apply.
        delta: EstateAccessDelta,
        /// The target agent or group id.
        target: Uuid,
    },
    /// Kick (eject) an agent from the region (`kickestate`).
    KickEstateUser {
        /// The agent to kick.
        target: Uuid,
    },
    /// Teleport an agent home (`teleporthomeuser`).
    TeleportHomeUser {
        /// The agent to send home.
        target: Uuid,
    },
    /// Teleport every agent in the region home (`teleporthomeallusers`).
    TeleportHomeAllUsers,
    /// Schedule a region restart in `seconds` (`restart`); `-1` delays a pending
    /// restart by an hour.
    RestartRegion {
        /// Seconds until restart (`-1` to delay).
        seconds: i32,
    },
    /// Send an estate-wide blue-box notice (`simulatormessage`).
    SendEstateMessage {
        /// The message body.
        message: String,
    },
    /// Update the region's settings (`setregioninfo`).
    SetRegionInfo(RegionInfoUpdate),
    /// God-level eject of an agent (`GodKickUser`; needs grid-god rights).
    GodKickUser {
        /// The agent to kick.
        target: Uuid,
        /// The kick reason.
        reason: String,
    },
    /// Send a generic god-level command (`GodlikeMessage`; needs grid-god rights).
    SendGodlikeMessage {
        /// The god method name.
        method: String,
        /// The string parameters.
        params: Vec<String>,
    },
    /// Request the agent's L$ balance (`MoneyBalanceRequest`); the reply arrives
    /// as [`SlSessionEvent::MoneyBalance`].
    RequestMoneyBalance,
    /// Request the grid's economy data (`EconomyDataRequest`); the reply arrives
    /// as [`SlSessionEvent::EconomyData`].
    RequestEconomyData,
    /// Pay L$ to an avatar or object (`MoneyTransferRequest`).
    SendMoneyTransfer {
        /// The payee (avatar or object id).
        dest: Uuid,
        /// The L$ amount to pay.
        amount: LindenAmount,
        /// The kind of transaction (e.g. gift, pay-object).
        kind: MoneyTransactionType,
        /// A description annotating the transaction.
        description: String,
    },
    /// Set the draw distance advertised in keep-alive `AgentUpdate`s.
    SetDrawDistance(f32),
    /// Request world-map blocks for a grid-coordinate rectangle (region
    /// indices); each region arrives as an [`SlSessionEvent::MapBlock`].
    RequestMapBlocks {
        /// Minimum grid x (inclusive).
        min_x: u32,
        /// Maximum grid x (inclusive).
        max_x: u32,
        /// Minimum grid y (inclusive).
        min_y: u32,
        /// Maximum grid y (inclusive).
        max_y: u32,
    },
    /// Search the world map for regions by name (`MapNameRequest`); matches
    /// arrive as [`SlSessionEvent::MapBlock`].
    RequestMapByName {
        /// The region name (or prefix) to search for.
        name: String,
    },
    /// Request world-map overlay items of a given type (`MapItemRequest`); the
    /// reply arrives as [`SlSessionEvent::MapItems`].
    RequestMapItems {
        /// The kind of item to request (avatars, telehubs, land for sale, …).
        item_type: MapItemType,
        /// The target region handle (0 = the current region).
        region_handle: u64,
    },
    /// Request the full `ObjectUpdate` for the given region-local ids
    /// (`RequestMultipleObjects`); updates arrive as [`SlSessionEvent::ObjectAdded`]
    /// / [`SlSessionEvent::ObjectUpdated`].
    RequestObjects {
        /// The region-local ids to (re)fetch.
        local_ids: Vec<u32>,
    },
    /// Request objects' extended properties by selecting them (`ObjectSelect`);
    /// the reply arrives as [`SlSessionEvent::ObjectProperties`].
    RequestObjectProperties {
        /// The region-local ids to select.
        local_ids: Vec<u32>,
    },
    /// Deselect previously selected objects (`ObjectDeselect`).
    DeselectObjects {
        /// The region-local ids to deselect.
        local_ids: Vec<u32>,
    },
    /// Touch (left-click) an object (`ObjectGrab` + `ObjectDeGrab`).
    TouchObject {
        /// The object's region-local id.
        local_id: u32,
    },
    /// Begin grabbing an object (`ObjectGrab`).
    GrabObject {
        /// The object's region-local id.
        local_id: u32,
        /// The grab offset from the object's centre.
        grab_offset: Vector,
    },
    /// Update an in-progress grab as the object is dragged (`ObjectGrabUpdate`).
    GrabObjectUpdate {
        /// The object's persistent global id.
        object_id: Uuid,
        /// The initial grab offset.
        grab_offset_initial: Vector,
        /// The current region-local grab position.
        grab_position: Vector,
        /// Milliseconds since the previous update.
        time_since_last: u32,
    },
    /// Release a grab on an object (`ObjectDeGrab`).
    DegrabObject {
        /// The object's region-local id.
        local_id: u32,
    },
    /// Rez (create) a new primitive (`ObjectAdd`).
    RezObject {
        /// The shape of the prim to rez.
        shape: PrimShape,
        /// The group the new object is set to ([`Uuid::nil`] for none).
        group_id: Uuid,
    },
    /// Duplicate objects with an offset (`ObjectDuplicate`).
    DuplicateObjects {
        /// The region-local ids to duplicate.
        local_ids: Vec<u32>,
        /// The offset to apply to the copies.
        offset: Vector,
        /// The group the copies are set to.
        group_id: Uuid,
    },
    /// Delete objects to the trash (`ObjectDelete`).
    DeleteObjects {
        /// The region-local ids to delete.
        local_ids: Vec<u32>,
    },
    /// Derez objects (take/return/trash; `DeRezObject`).
    DerezObjects {
        /// The region-local ids to derez.
        local_ids: Vec<u32>,
        /// Where the objects should go.
        destination: DeRezDestination,
        /// The destination folder/task id (meaning depends on `destination`).
        destination_id: Uuid,
        /// A caller-chosen id correlating the resulting inventory update.
        transaction_id: Uuid,
        /// The active group ([`Uuid::nil`] for none).
        group_id: Uuid,
    },
    /// Move/rotate/scale an object (`MultipleObjectUpdate`).
    UpdateObject {
        /// The object's region-local id.
        local_id: u32,
        /// The transform to apply (only set components change).
        transform: ObjectTransform,
    },
    /// Rename an object (`ObjectName`).
    SetObjectName {
        /// The object's region-local id.
        local_id: u32,
        /// The new name.
        name: String,
    },
    /// Re-describe an object (`ObjectDescription`).
    SetObjectDescription {
        /// The object's region-local id.
        local_id: u32,
        /// The new description.
        description: String,
    },
    /// Set an object's left-click behaviour (`ObjectClickAction`).
    SetObjectClickAction {
        /// The object's region-local id.
        local_id: u32,
        /// The new click action.
        action: ClickAction,
    },
    /// Set an object's physical material (`ObjectMaterial`).
    SetObjectMaterial {
        /// The object's region-local id.
        local_id: u32,
        /// The new material.
        material: Material,
    },
    /// Set an object's physics/temporary/phantom flags (`ObjectFlagUpdate`).
    SetObjectFlags {
        /// The object's region-local id.
        local_id: u32,
        /// The flag settings to apply.
        flags: ObjectFlagSettings,
    },
    /// Set the group objects are set to (`ObjectGroup`).
    SetObjectGroup {
        /// The region-local ids.
        local_ids: Vec<u32>,
        /// The group id.
        group_id: Uuid,
    },
    /// Set or clear permission bits on objects (`ObjectPermissions`).
    SetObjectPermissions {
        /// The region-local ids.
        local_ids: Vec<u32>,
        /// Which mask to change.
        field: PermissionField,
        /// Whether to set (true) or clear (false) the bits.
        set: bool,
        /// The `PERM_*` bits to set or clear.
        mask: u32,
    },
    /// Set an object's sale type and price (`ObjectSaleInfo`).
    SetObjectForSale {
        /// The object's region-local id.
        local_id: u32,
        /// The sale type.
        sale_type: SaleType,
        /// The sale price in L$.
        sale_price: i32,
    },
    /// Set an object's category code (`ObjectCategory`).
    SetObjectCategory {
        /// The object's region-local id.
        local_id: u32,
        /// The category code.
        category: u32,
    },
    /// Toggle whether an object is listed in search (`ObjectIncludeInSearch`).
    SetObjectIncludeInSearch {
        /// The object's region-local id.
        local_id: u32,
        /// Whether to include the object in search.
        include: bool,
    },
    /// Link objects into one linkset (`ObjectLink`); the first id is the root.
    LinkObjects {
        /// The region-local ids to link (first = root).
        local_ids: Vec<u32>,
    },
    /// Unlink objects from their linksets (`ObjectDelink`).
    DelinkObjects {
        /// The region-local ids to unlink.
        local_ids: Vec<u32>,
    },
    /// Request a texture over the legacy UDP image path (`RequestImage`); the
    /// reassembled image arrives as [`SlSessionEvent::TextureReceived`] (or
    /// [`SlSessionEvent::TextureNotFound`]).
    RequestTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: i8,
        /// The download priority (larger is fetched sooner).
        priority: f32,
    },
    /// Request a generic asset over the UDP transfer path (`TransferRequest`);
    /// the reassembled asset arrives as [`SlSessionEvent::AssetReceived`] (or
    /// [`SlSessionEvent::AssetTransferFailed`]).
    RequestAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class.
        asset_type: AssetType,
        /// The transfer priority.
        priority: f32,
    },
    /// Fetch a texture over the HTTP `GetTexture` capability; the image arrives
    /// as [`SlSessionEvent::TextureReceived`] (or
    /// [`SlSessionEvent::TextureNotFound`]). When `discard_level` is non-zero the
    /// codestream is truncated to that level-of-detail prefix via [`j2c`].
    FetchTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: u8,
    },
    /// Fetch a mesh asset over the HTTP `GetMesh2`/`GetMesh` capability; the data
    /// arrives as [`SlSessionEvent::AssetReceived`].
    FetchMesh {
        /// The mesh asset's id.
        mesh_id: Uuid,
    },
    /// Fetch a generic asset over the HTTP `GetAsset` capability; the data
    /// arrives as [`SlSessionEvent::AssetReceived`] (or
    /// [`SlSessionEvent::AssetTransferFailed`]).
    FetchAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class (selects the cap query parameter).
        asset_type: AssetType,
    },
    /// Ask the simulator to (re-)send the agent's own wearables
    /// (`AgentWearablesRequest`); the reply arrives as
    /// [`SlSessionEvent::AgentWearables`].
    RequestWearables,
    /// Set the agent's outfit (`AgentIsNowWearing`): the complete set of
    /// wearables to wear. The simulator acknowledges with a fresh
    /// [`SlSessionEvent::AgentWearables`].
    SetWearing(Vec<Wearable>),
    /// Advertise the agent's own appearance (`AgentSetAppearance`): the legacy
    /// client-side bake path (used by OpenSim and pre-server-baking regions).
    SetAppearance {
        /// The appearance serial (strictly increasing; 0 resets).
        serial: u32,
        /// The agent's bounding-box size, in metres.
        size: Vector,
        /// The packed `TextureEntry` blob carrying the baked-texture ids.
        texture_entry: Vec<u8>,
        /// The visual parameter bytes (one per parameter, in viewer order).
        visual_params: Vec<u8>,
        /// The per-baked-slot cache hashes (`(cache id, texture slot index)`).
        wearable_cache: Vec<(Uuid, u8)>,
    },
    /// Query the simulator's baked-texture cache (`AgentCachedTexture`): the
    /// reply arrives as [`SlSessionEvent::CachedTextureResponse`].
    RequestCachedTextures {
        /// The serial echoed back in the reply.
        serial: i32,
        /// The queried slots, as `(cache id, texture slot index)` pairs.
        slots: Vec<(Uuid, u8)>,
    },
    /// Trigger a modern server-side appearance bake over the HTTP
    /// `UpdateAvatarAppearance` capability (Second Life "central baking"): the
    /// grid composites the Current Outfit Folder and broadcasts the result as
    /// [`SlSessionEvent::AvatarAppearance`]; the POST reply arrives as
    /// [`SlSessionEvent::ServerAppearanceUpdate`].
    RequestServerAppearanceUpdate {
        /// The Current Outfit Folder version the grid should bake.
        cof_version: i32,
    },
    /// Start and/or stop several of the agent's own animations (`AgentAnimation`):
    /// each `(anim_id, start)` pair starts (`true`) or stops (`false`) one
    /// animation. Other avatars observe the result as a
    /// [`SlSessionEvent::AvatarAnimation`].
    SetAnimations(Vec<(Uuid, bool)>),
    /// Start one of the agent's own animations (`AgentAnimation`); convenience
    /// for a single-element [`SlCommand::SetAnimations`].
    PlayAnimation(Uuid),
    /// Stop one of the agent's own animations (`AgentAnimation`); convenience for
    /// a single-element [`SlCommand::SetAnimations`].
    StopAnimation(Uuid),
    /// Upload a new asset over the legacy UDP path (`AssetUploadRequest`): stores
    /// the asset bytes (small assets inline, larger ones over `Xfer`) without
    /// creating an inventory item. Completion arrives as
    /// [`SlSessionEvent::AssetUploadComplete`]. For an upload that also creates an
    /// inventory item, use [`SlCommand::UploadAsset`].
    UploadAssetUdp {
        /// The asset class to store the bytes as.
        asset_type: AssetType,
        /// The raw asset bytes.
        data: Vec<u8>,
        /// Mark the asset temporary.
        temp_file: bool,
        /// Keep the asset on the simulator only (do not store it grid-wide).
        store_local: bool,
    },
    /// Upload a new asset and create an inventory item for it over the modern
    /// `NewFileAgentInventory` capability (the two-step CAPS uploader). The result
    /// arrives as [`SlSessionEvent::AssetUploaded`] (or
    /// [`SlSessionEvent::AssetUploadFailed`]).
    ///
    /// For a mesh, `data` must be the **fully-formed mesh asset bytes** —
    /// uploading does not run the viewer's model-import pipeline (LOD / physics /
    /// cost generation).
    UploadAsset {
        /// The destination inventory folder.
        folder_id: Uuid,
        /// The asset class (e.g. [`AssetType::Texture`], [`AssetType::Animation`]).
        asset_type: AssetType,
        /// The inventory-item class (e.g. [`InventoryType::Texture`],
        /// [`InventoryType::Wearable`]).
        inventory_type: InventoryType,
        /// The new item's name.
        name: String,
        /// The new item's description.
        description: String,
        /// The permission bits granted to the next owner.
        next_owner_mask: u32,
        /// The permission bits granted to the group.
        group_mask: u32,
        /// The permission bits granted to everyone.
        everyone_mask: u32,
        /// The L$ price the client expects to be charged (0 on free grids such
        /// as OpenSim).
        expected_upload_cost: i32,
        /// The raw asset bytes.
        data: Vec<u8>,
    },
    /// Upload a client-computed baked avatar texture over the
    /// `UploadBakedTexture` capability (the legacy appearance path): stores a
    /// *temporary* asset with no inventory item. The result arrives as
    /// [`SlSessionEvent::AssetUploaded`] (with `new_inventory_item` = `None`) or
    /// [`SlSessionEvent::AssetUploadFailed`].
    UploadBakedTexture {
        /// The raw baked-texture bytes (a JPEG-2000 codestream).
        data: Vec<u8>,
    },
    /// Replace the asset of an existing inventory item over the matching
    /// `Update*AgentInventory` capability (gesture / notecard / script /
    /// settings, selected by `asset_type`). The result arrives as
    /// [`SlSessionEvent::AssetUploaded`] or [`SlSessionEvent::AssetUploadFailed`].
    UpdateInventoryAsset {
        /// The inventory item whose asset is being replaced.
        item_id: Uuid,
        /// The item's asset class (selects the capability; see
        /// [`AssetType::update_item_cap`]).
        asset_type: AssetType,
        /// The new raw asset bytes.
        data: Vec<u8>,
    },
    /// Fetch an object's per-face **media-on-a-prim** settings over the
    /// `ObjectMedia` capability (a GET). The result arrives as
    /// [`SlSessionEvent::ObjectMedia`].
    RequestObjectMedia {
        /// The object whose media to fetch.
        object_id: Uuid,
    },
    /// Set an object's per-face media over the `ObjectMedia` capability (an
    /// UPDATE). `faces` is one entry per prim face in order; a face with no media
    /// is `None`. The simulator advances the object's media version (visible on a
    /// subsequent [`SlCommand::RequestObjectMedia`]) rather than replying.
    SetObjectMedia {
        /// The object whose media to set.
        object_id: Uuid,
        /// Per-face media, one slot per prim face in order (`None` = no media).
        faces: Vec<Option<MediaEntry>>,
    },
    /// Navigate the media on a single prim face to a new URL over the
    /// `ObjectMediaNavigate` capability. The simulator advances the object's
    /// media version (visible on a subsequent [`SlCommand::RequestObjectMedia`]).
    NavigateObjectMedia {
        /// The object whose media to navigate.
        object_id: Uuid,
        /// The prim face (texture index) to navigate.
        face: u8,
        /// The URL to navigate that face's media to.
        url: String,
    },
    /// Fetch the legacy (normal/specular) materials for `material_ids` over the
    /// `RenderMaterials` capability (the OpenSim-supported path). The result
    /// arrives as [`SlSessionEvent::RenderMaterials`].
    RequestRenderMaterials {
        /// The material ids to fetch (per-face `TextureEntry` material ids).
        material_ids: Vec<Uuid>,
    },
    /// Set GLTF (PBR) materials on object faces over the `ModifyMaterialParams`
    /// capability. Each update applies an opaque `gltf_json` override and/or a
    /// stored material `asset_id` to one face (`side`, or `-1` for all). The
    /// `{ success, message }` reply arrives as
    /// [`SlSessionEvent::MaterialParamsResult`].
    ModifyMaterialParams {
        /// The per-face material assignments to apply.
        updates: Vec<MaterialOverrideUpdate>,
    },
    /// Begin a clean logout.
    Logout,
}

/// The plugin configuration resource.
#[derive(Resource, Debug)]
struct SlConfig {
    /// The login parameters.
    params: LoginParams,
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
    },
    /// The session is finished.
    Done,
}

/// The CAPS subsystem for one region: a background thread fetches the capability
/// map (reported over `map_rx`) then long-polls `EventQueueGet`, forwarding each
/// decoded event over `events_rx`. One-shot CAPS fetches (inventory) run on their
/// own threads and report back over the same `events_tx`. Dropping it signals the
/// poller thread to stop after its in-flight request returns.
struct Caps {
    /// Receives decoded event-queue events and CAPS responses (e.g. inventory).
    events_rx: Receiver<(String, Llsd)>,
    /// A sender clone for spawning one-shot CAPS fetches.
    events_tx: Sender<(String, Llsd)>,
    /// Receives fully-formed session events from one-shot binary asset fetches
    /// (the HTTP texture/mesh/asset caps, which return raw bytes rather than
    /// LLSD), to be surfaced directly as [`SlEvent`]s.
    asset_rx: Receiver<SessionEvent>,
    /// A sender clone for spawning one-shot binary asset fetches.
    asset_tx: Sender<SessionEvent>,
    /// Receives the region's capability map once the poller has fetched it.
    map_rx: Receiver<HashMap<String, String>>,
    /// The cached capability map (cap name → URL), empty until discovered.
    map: HashMap<String, String>,
    /// Set on drop to ask the poller thread to stop at its next loop iteration.
    stop: Arc<AtomicBool>,
}

impl Drop for Caps {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Startup system: builds the session and spawns the blocking login thread.
fn start_login(mut commands: Commands, config: Res<SlConfig>) {
    let session = Session::new(config.params.clone());
    let inner = match session.login_http_request() {
        Some(request) => {
            let (tx, rx) = unbounded();
            std::thread::spawn(move || {
                tx.send(perform_login(
                    &request.url,
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
fn drive(
    mut state: ResMut<SlState>,
    mut events: EventWriter<SlEvent>,
    mut mfa: EventWriter<SlMfaChallenge>,
    mut commands: EventReader<SlCommand>,
) {
    let now = Instant::now();
    let inner = std::mem::replace(&mut state.inner, SlInner::Done);
    state.inner = match inner {
        SlInner::LoggingIn { session, rx } => {
            advance_login(session, rx, now, &mut events, &mut mfa)
        }
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        } => advance_running(
            session,
            socket,
            recv_buf,
            caps,
            now,
            &mut events,
            &mut commands,
        ),
        SlInner::Done => SlInner::Done,
    };
}

/// Handles the logging-in phase, transitioning to `Running` once the login
/// response arrives.
fn advance_login(
    mut session: Box<Session>,
    rx: Receiver<Result<String, String>>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    mfa: &mut EventWriter<SlMfaChallenge>,
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
                        let caps = start_caps(&session);
                        SlInner::Running {
                            session,
                            socket,
                            recv_buf: vec![0u8; RECV_BUFFER_SIZE],
                            caps,
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
                emit_disconnect(
                    events,
                    DisconnectReason::LoginFailed {
                        reason: failure.reason,
                        message: failure.message,
                    },
                );
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
/// out, transmit, and surface events.
fn advance_running(
    mut session: Box<Session>,
    socket: UdpSocket,
    mut recv_buf: Vec<u8>,
    mut caps: Option<Caps>,
    now: Instant,
    events: &mut EventWriter<SlEvent>,
    commands: &mut EventReader<SlCommand>,
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
            caps.map = map;
        }
        while let Ok((message, body)) = caps.events_rx.try_recv() {
            session.handle_caps_event(&message, &body, now).ok();
        }
        // Binary asset fetches return fully-formed session events; surface them.
        while let Ok(event) = caps.asset_rx.try_recv() {
            events.write(SlEvent(event));
        }
    }

    // Apply queued commands.
    for command in commands.read() {
        match command {
            SlCommand::Send {
                message,
                reliability,
            } => {
                session.enqueue((**message).clone(), *reliability, now).ok();
            }
            SlCommand::Chat {
                message,
                chat_type,
                channel,
            } => {
                session.say(message, *chat_type, *channel, now).ok();
            }
            SlCommand::Typing(typing) => {
                session.set_typing(*typing, now).ok();
            }
            SlCommand::InstantMessage {
                to_agent_id,
                message,
            } => {
                session
                    .send_instant_message(*to_agent_id, message, now)
                    .ok();
            }
            SlCommand::ImTyping {
                to_agent_id,
                typing,
            } => {
                session.send_im_typing(*to_agent_id, *typing, now).ok();
            }
            SlCommand::SetControls(controls) => {
                session.set_controls(*controls, now).ok();
            }
            SlCommand::SetThrottle(throttle) => {
                session.set_throttle(*throttle, now).ok();
            }
            SlCommand::SetRotation { body, head } => {
                session.set_rotation(body.clone(), head.clone(), now).ok();
            }
            SlCommand::Stand => {
                session.stand(now).ok();
            }
            SlCommand::SitOnGround => {
                session.sit_on_ground(now).ok();
            }
            SlCommand::Sit { target, offset } => {
                session.sit_on(*target, offset.clone(), now).ok();
            }
            SlCommand::Autopilot {
                global_x,
                global_y,
                z,
            } => {
                session.autopilot_to(*global_x, *global_y, *z, now).ok();
            }
            SlCommand::RequestAvatarProperties(target) => {
                session.request_avatar_properties(*target, now).ok();
            }
            SlCommand::RequestAvatarPicks(target) => {
                session.request_avatar_picks(*target, now).ok();
            }
            SlCommand::RequestAvatarNotes(target) => {
                session.request_avatar_notes(*target, now).ok();
            }
            SlCommand::RequestFolderContents(folder_id) => {
                session.request_folder_contents(*folder_id, now).ok();
            }
            SlCommand::FetchInventoryFolders(folder_ids) => {
                if let Some(caps) = caps.as_ref()
                    && let (Some(url), Some(owner)) = (
                        caps.map.get(CAP_FETCH_INVENTORY).cloned(),
                        session.agent_id(),
                    )
                {
                    let events_tx = caps.events_tx.clone();
                    let folders = folder_ids.clone();
                    std::thread::spawn(move || {
                        run_inventory_fetch(&url, owner, &folders, &events_tx);
                    });
                }
            }
            SlCommand::FetchGroupMembers(group_id) => {
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
            SlCommand::OfferFriendship {
                to_agent_id,
                message,
            } => {
                session
                    .send_friendship_offer(*to_agent_id, message, now)
                    .ok();
            }
            SlCommand::GrantUserRights { target, rights } => {
                session.grant_user_rights(*target, *rights, now).ok();
            }
            SlCommand::TerminateFriendship(other) => {
                session.terminate_friendship(*other, now).ok();
            }
            SlCommand::AcceptFriendship {
                transaction_id,
                calling_card_folder,
            } => {
                session
                    .accept_friendship(*transaction_id, *calling_card_folder, now)
                    .ok();
            }
            SlCommand::DeclineFriendship(transaction_id) => {
                session.decline_friendship(*transaction_id, now).ok();
            }
            SlCommand::ActivateGroup(group_id) => {
                session.activate_group(*group_id, now).ok();
            }
            SlCommand::RequestGroupMembers(group_id) => {
                session.request_group_members(*group_id, now).ok();
            }
            SlCommand::RequestGroupRoles(group_id) => {
                session.request_group_roles(*group_id, now).ok();
            }
            SlCommand::RequestGroupRoleMembers(group_id) => {
                session.request_group_role_members(*group_id, now).ok();
            }
            SlCommand::RequestGroupTitles(group_id) => {
                session.request_group_titles(*group_id, now).ok();
            }
            SlCommand::RequestGroupProfile(group_id) => {
                session.request_group_profile(*group_id, now).ok();
            }
            SlCommand::RequestGroupNotices(group_id) => {
                session.request_group_notices(*group_id, now).ok();
            }
            SlCommand::RequestGroupNotice(notice_id) => {
                session.request_group_notice(*notice_id, now).ok();
            }
            SlCommand::CreateGroup(params) => {
                session.create_group(params, now).ok();
            }
            SlCommand::JoinGroup(group_id) => {
                session.join_group(*group_id, now).ok();
            }
            SlCommand::LeaveGroup(group_id) => {
                session.leave_group(*group_id, now).ok();
            }
            SlCommand::InviteToGroup { group_id, invitees } => {
                session.invite_to_group(*group_id, invitees, now).ok();
            }
            SlCommand::SetGroupAcceptNotices {
                group_id,
                accept_notices,
                list_in_profile,
            } => {
                session
                    .set_group_accept_notices(*group_id, *accept_notices, *list_in_profile, now)
                    .ok();
            }
            SlCommand::SetGroupContribution {
                group_id,
                contribution,
            } => {
                session
                    .set_group_contribution(*group_id, *contribution, now)
                    .ok();
            }
            SlCommand::StartGroupSession(group_id) => {
                session.start_group_session(*group_id, now).ok();
            }
            SlCommand::SendGroupMessage { group_id, message } => {
                session.send_group_message(*group_id, message, now).ok();
            }
            SlCommand::LeaveGroupSession(group_id) => {
                session.leave_group_session(*group_id, now).ok();
            }
            SlCommand::ReplyScriptDialog {
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
            SlCommand::AnswerScriptPermissions {
                task_id,
                item_id,
                permissions,
            } => {
                session
                    .answer_script_permissions(*task_id, *item_id, *permissions, now)
                    .ok();
            }
            SlCommand::RequestMuteList => {
                session.request_mute_list(now).ok();
            }
            SlCommand::Mute {
                id,
                name,
                mute_type,
                flags,
            } => {
                session.mute(*id, name, *mute_type, *flags, now).ok();
            }
            SlCommand::Unmute { id, name } => {
                session.unmute(*id, name, now).ok();
            }
            SlCommand::Teleport {
                region_handle,
                position,
                look_at,
            } => {
                session
                    .teleport_to(*region_handle, position.clone(), look_at.clone(), now)
                    .ok();
            }
            SlCommand::RequestRegionInfo => {
                session.request_region_info(now).ok();
            }
            SlCommand::RequestMoneyBalance => {
                session.request_money_balance(now).ok();
            }
            SlCommand::RequestEconomyData => {
                session.request_economy_data(now).ok();
            }
            SlCommand::SendMoneyTransfer {
                dest,
                amount,
                kind,
                description,
            } => {
                session
                    .send_money_transfer(*dest, amount.clone(), *kind, description, now)
                    .ok();
            }
            SlCommand::RequestParcelProperties {
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
            SlCommand::SetDrawDistance(far) => session.set_draw_distance(*far),
            SlCommand::RequestMapBlocks {
                min_x,
                max_x,
                min_y,
                max_y,
            } => {
                session
                    .request_map_blocks(*min_x, *max_x, *min_y, *max_y, now)
                    .ok();
            }
            SlCommand::RequestMapByName { name } => {
                session.request_map_by_name(name, now).ok();
            }
            SlCommand::RequestMapItems {
                item_type,
                region_handle,
            } => {
                session
                    .request_map_items(*item_type, *region_handle, now)
                    .ok();
            }
            SlCommand::RequestObjects { local_ids } => {
                session.request_objects(local_ids, now).ok();
            }
            SlCommand::RequestObjectProperties { local_ids } => {
                session.request_object_properties(local_ids, now).ok();
            }
            SlCommand::DeselectObjects { local_ids } => {
                session.deselect_objects(local_ids, now).ok();
            }
            SlCommand::TouchObject { local_id } => {
                session.touch_object(*local_id, now).ok();
            }
            SlCommand::GrabObject {
                local_id,
                grab_offset,
            } => {
                session
                    .grab_object(*local_id, grab_offset.clone(), now)
                    .ok();
            }
            SlCommand::GrabObjectUpdate {
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
            SlCommand::DegrabObject { local_id } => {
                session.degrab_object(*local_id, now).ok();
            }
            SlCommand::RezObject { shape, group_id } => {
                session.rez_object(shape, *group_id, now).ok();
            }
            SlCommand::DuplicateObjects {
                local_ids,
                offset,
                group_id,
            } => {
                session
                    .duplicate_objects(local_ids, offset.clone(), *group_id, now)
                    .ok();
            }
            SlCommand::DeleteObjects { local_ids } => {
                session.delete_objects(local_ids, now).ok();
            }
            SlCommand::DerezObjects {
                local_ids,
                destination,
                destination_id,
                transaction_id,
                group_id,
            } => {
                session
                    .derez_objects(
                        local_ids,
                        *destination,
                        *destination_id,
                        *transaction_id,
                        *group_id,
                        now,
                    )
                    .ok();
            }
            SlCommand::UpdateObject {
                local_id,
                transform,
            } => {
                session.update_object(*local_id, transform, now).ok();
            }
            SlCommand::SetObjectName { local_id, name } => {
                session.set_object_name(*local_id, name, now).ok();
            }
            SlCommand::SetObjectDescription {
                local_id,
                description,
            } => {
                session
                    .set_object_description(*local_id, description, now)
                    .ok();
            }
            SlCommand::SetObjectClickAction { local_id, action } => {
                session
                    .set_object_click_action(*local_id, *action, now)
                    .ok();
            }
            SlCommand::SetObjectMaterial { local_id, material } => {
                session.set_object_material(*local_id, *material, now).ok();
            }
            SlCommand::SetObjectFlags { local_id, flags } => {
                session.set_object_flags(*local_id, flags, now).ok();
            }
            SlCommand::SetObjectGroup {
                local_ids,
                group_id,
            } => {
                session.set_object_group(local_ids, *group_id, now).ok();
            }
            SlCommand::SetObjectPermissions {
                local_ids,
                field,
                set,
                mask,
            } => {
                session
                    .set_object_permissions(local_ids, *field, *set, *mask, now)
                    .ok();
            }
            SlCommand::SetObjectForSale {
                local_id,
                sale_type,
                sale_price,
            } => {
                session
                    .set_object_for_sale(*local_id, *sale_type, *sale_price, now)
                    .ok();
            }
            SlCommand::SetObjectCategory { local_id, category } => {
                session.set_object_category(*local_id, *category, now).ok();
            }
            SlCommand::SetObjectIncludeInSearch { local_id, include } => {
                session
                    .set_object_include_in_search(*local_id, *include, now)
                    .ok();
            }
            SlCommand::LinkObjects { local_ids } => {
                session.link_objects(local_ids, now).ok();
            }
            SlCommand::DelinkObjects { local_ids } => {
                session.delink_objects(local_ids, now).ok();
            }
            SlCommand::UpdateParcel(update) => {
                session.update_parcel(update, now).ok();
            }
            SlCommand::RequestParcelAccessList { local_id, scope } => {
                session
                    .request_parcel_access_list(*local_id, *scope, now)
                    .ok();
            }
            SlCommand::UpdateParcelAccessList {
                local_id,
                scope,
                entries,
            } => {
                session
                    .update_parcel_access_list(*local_id, *scope, entries, now)
                    .ok();
            }
            SlCommand::RequestParcelDwell { local_id } => {
                session.request_parcel_dwell(*local_id, now).ok();
            }
            SlCommand::BuyParcel {
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
            SlCommand::ReturnParcelObjects {
                local_id,
                return_type,
                owner_ids,
                task_ids,
            } => {
                session
                    .return_parcel_objects(*local_id, *return_type, owner_ids, task_ids, now)
                    .ok();
            }
            SlCommand::SelectParcelObjects {
                local_id,
                return_type,
                object_ids,
            } => {
                session
                    .select_parcel_objects(*local_id, *return_type, object_ids, now)
                    .ok();
            }
            SlCommand::DeedParcelToGroup { local_id, group_id } => {
                session.deed_parcel_to_group(*local_id, *group_id, now).ok();
            }
            SlCommand::ReclaimParcel { local_id } => {
                session.reclaim_parcel(*local_id, now).ok();
            }
            SlCommand::ReleaseParcel { local_id } => {
                session.release_parcel(*local_id, now).ok();
            }
            SlCommand::RequestEstateInfo => {
                session.request_estate_info(now).ok();
            }
            SlCommand::UpdateEstateAccess { delta, target } => {
                session.update_estate_access(*delta, *target, now).ok();
            }
            SlCommand::KickEstateUser { target } => {
                session.kick_estate_user(*target, now).ok();
            }
            SlCommand::TeleportHomeUser { target } => {
                session.teleport_home_user(*target, now).ok();
            }
            SlCommand::TeleportHomeAllUsers => {
                session.teleport_home_all_users(now).ok();
            }
            SlCommand::RestartRegion { seconds } => {
                session.restart_region(*seconds, now).ok();
            }
            SlCommand::SendEstateMessage { message } => {
                session.send_estate_message(message, now).ok();
            }
            SlCommand::SetRegionInfo(update) => {
                session.set_region_info(update, now).ok();
            }
            SlCommand::GodKickUser { target, reason } => {
                session.god_kick_user(*target, reason, now).ok();
            }
            SlCommand::SendGodlikeMessage { method, params } => {
                let refs: Vec<&str> = params.iter().map(String::as_str).collect();
                session.send_godlike_message(method, &refs, now).ok();
            }
            SlCommand::RequestTexture {
                texture_id,
                discard_level,
                priority,
            } => {
                session
                    .request_texture(*texture_id, *discard_level, *priority, now)
                    .ok();
            }
            SlCommand::RequestAsset {
                asset_id,
                asset_type,
                priority,
            } => {
                session
                    .request_asset(*asset_id, *asset_type, *priority, now)
                    .ok();
            }
            SlCommand::FetchTexture {
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
            SlCommand::FetchMesh { mesh_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps
                        .map
                        .get(CAP_GET_MESH2)
                        .or_else(|| caps.map.get(CAP_GET_MESH))
                        .cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let id = *mesh_id;
                    std::thread::spawn(move || {
                        run_asset_fetch(
                            &url,
                            &format!("?mesh_id={id}"),
                            id,
                            AssetType::Mesh,
                            &asset_tx,
                        );
                    });
                }
            }
            SlCommand::FetchAsset {
                asset_id,
                asset_type,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ASSET).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, asset_type) = (*asset_id, *asset_type);
                    std::thread::spawn(move || {
                        run_generic_asset_fetch(&url, id, asset_type, &asset_tx);
                    });
                }
            }
            SlCommand::RequestWearables => {
                session.request_wearables(now).ok();
            }
            SlCommand::SetWearing(wearables) => {
                session.set_wearing(wearables, now).ok();
            }
            SlCommand::SetAppearance {
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
            SlCommand::RequestCachedTextures { serial, slots } => {
                session.request_cached_textures(*serial, slots, now).ok();
            }
            SlCommand::RequestServerAppearanceUpdate { cof_version } => {
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
            SlCommand::SetAnimations(animations) => {
                session.set_animations(animations, now).ok();
            }
            SlCommand::PlayAnimation(anim_id) => {
                session.play_animation(*anim_id, now).ok();
            }
            SlCommand::StopAnimation(anim_id) => {
                session.stop_animation(*anim_id, now).ok();
            }
            SlCommand::UploadAssetUdp {
                asset_type,
                data,
                temp_file,
                store_local,
            } => {
                session
                    .upload_asset_udp(*asset_type, data.clone(), *temp_file, *store_local, now)
                    .ok();
            }
            SlCommand::UploadAsset {
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
            }
            SlCommand::UploadBakedTexture { data } => {
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
            SlCommand::UpdateInventoryAsset {
                item_id,
                asset_type,
                data,
            } => match asset_type.update_item_cap() {
                Some(cap) => {
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
                None => emit_upload_failure(
                    caps.as_ref(),
                    "asset type has no inventory-update capability".to_owned(),
                ),
            },
            SlCommand::RequestObjectMedia { object_id } => {
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
            SlCommand::SetObjectMedia { object_id, faces } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_OBJECT_MEDIA).cloned()
                {
                    let body = build_object_media_update_request(*object_id, faces);
                    std::thread::spawn(move || {
                        run_object_media_post(&url, body);
                    });
                }
            }
            SlCommand::NavigateObjectMedia {
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
            SlCommand::RequestRenderMaterials { material_ids } => {
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
            SlCommand::ModifyMaterialParams { updates } => {
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
            SlCommand::Logout => session.initiate_logout(now),
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
            _ => {}
        }
        events.write(SlEvent(event));
    }
    if region_changed {
        caps = start_caps(&session);
    }

    if done || session.is_closed() {
        SlInner::Done
    } else {
        SlInner::Running {
            session,
            socket,
            recv_buf,
            caps,
        }
    }
}

/// Starts the CAPS subsystem for the session's current seed capability: a
/// background thread that fetches the capability map (reported over `map_rx`)
/// then long-polls `EventQueueGet`. Returns `None` if no seed is known yet.
fn start_caps(session: &Session) -> Option<Caps> {
    let seed = session.seed_capability()?.to_owned();
    let (events_tx, events_rx) = unbounded();
    let (asset_tx, asset_rx) = unbounded();
    let (map_tx, map_rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_events = events_tx.clone();
    std::thread::spawn(move || run_caps(seed, &thread_events, &map_tx, &thread_stop));
    Some(Caps {
        events_rx,
        events_tx,
        asset_rx,
        asset_tx,
        map_rx,
        map: HashMap::new(),
        stop,
    })
}

/// POSTs a neighbour region's seed capability (in a detached thread, result
/// ignored) so the simulator marks the agent's capabilities as sent and begins
/// streaming that region's scene to the child circuit.
fn post_neighbour_seed(seed_url: String) {
    std::thread::spawn(move || {
        let Ok(http) = ReqwestBlockingClient::builder()
            .timeout(EVENT_QUEUE_TIMEOUT)
            .build()
        else {
            return;
        };
        let _ignored = http
            .post(&seed_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_seed_request(REQUESTED_CAPABILITIES))
            .send();
    });
}

/// Fetches the capability map from `seed_url` (reporting it over `map_tx`), then
/// long-polls the `EventQueueGet` capability, forwarding each decoded event to
/// `caps_tx` until `stop` is set, a receiver is dropped (e.g. on region change),
/// or a request fails fatally.
fn run_caps(
    seed_url: String,
    caps_tx: &Sender<(String, Llsd)>,
    map_tx: &Sender<HashMap<String, String>>,
    stop: &AtomicBool,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(&seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    let Ok(capabilities) = parse_seed_response(&text) else {
        return;
    };
    map_tx.send(capabilities.clone()).ok();
    let Some(event_queue_url) = capabilities.get("EventQueueGet").cloned() else {
        return;
    };

    let mut ack: Option<i32> = None;
    while !stop.load(Ordering::Relaxed) {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
        {
            Ok(response) => response,
            Err(_error) => {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let Ok(text) = response.text() else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).is_err() {
                return;
            }
        }
    }
}

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`SlSessionEvent::InventoryDescendents`].
fn run_inventory_fetch(
    cap_url: &str,
    owner_id: Uuid,
    folder_ids: &[Uuid],
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_fetch_inventory_request(owner_id, folder_ids);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_FETCH_INVENTORY.to_owned(), llsd)).ok();
    }
}

/// POSTs a `GroupMemberData` request for `group_id` and forwards the LLSD roster
/// response to `caps_tx` tagged [`CAP_GROUP_MEMBER_DATA`], for the session to
/// decode into [`SlSessionEvent::GroupMembers`].
fn run_group_members_fetch(cap_url: &str, group_id: Uuid, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_group_member_data_request(group_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd)).ok();
    }
}

/// POSTs an `UpdateAvatarAppearance` request for `cof_version` (the modern
/// Second Life server-side bake) and forwards the LLSD reply to `caps_tx` tagged
/// [`CAP_UPDATE_AVATAR_APPEARANCE`], for the session to surface as a
/// [`SlSessionEvent::ServerAppearanceUpdate`]. The baked appearance itself
/// arrives separately over UDP as a [`SlSessionEvent::AvatarAppearance`].
fn run_server_appearance_update(cap_url: &str, cof_version: i32, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_update_avatar_appearance_request(cof_version);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd))
            .ok();
    }
}

/// POSTs an `ObjectMedia` GET for `object_id` and forwards the decoded LLSD
/// response to `caps_tx` tagged [`CAP_OBJECT_MEDIA`], for the session to surface
/// as a [`SlSessionEvent::ObjectMedia`].
fn run_object_media_fetch(cap_url: &str, object_id: Uuid, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let body = build_object_media_get_request(object_id);
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx.send((CAP_OBJECT_MEDIA.to_owned(), llsd)).ok();
    }
}

/// POSTs a pre-built `ObjectMedia` UPDATE or `ObjectMediaNavigate` `body` to
/// `cap_url`. Fire-and-forget: the simulator advances the object's media version
/// rather than replying with media, so a client re-fetches with
/// [`SlCommand::RequestObjectMedia`] to observe the change.
fn run_object_media_post(cap_url: &str, body: String) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    http.post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .ok();
}

/// POSTs a `RenderMaterials` request for `material_ids` (the zipped binary-LLSD
/// form) and forwards the decoded legacy materials to `asset_tx` as a
/// [`SlSessionEvent::RenderMaterials`]. Best-effort: a transport or decode
/// failure yields an empty list.
fn run_render_materials_fetch(
    cap_url: &str,
    material_ids: Vec<Uuid>,
    asset_tx: &Sender<SessionEvent>,
) {
    let materials = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()
        .and_then(|http| {
            let body = build_render_materials_request(&material_ids);
            http.post(cap_url)
                .header("Content-Type", "application/llsd+xml")
                .body(body)
                .send()
                .ok()
        })
        .and_then(|response| response.text().ok())
        .map(|text| parse_render_materials_response(&text))
        .unwrap_or_default();
    asset_tx.send(SessionEvent::RenderMaterials(materials)).ok();
}

/// POSTs a `ModifyMaterialParams` request and forwards the `{ success, message }`
/// reply to `caps_tx` tagged [`CAP_MODIFY_MATERIAL_PARAMS`], for the session to
/// surface as a [`SlSessionEvent::MaterialParamsResult`].
fn run_modify_material_params(cap_url: &str, body: String, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .post(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    let Ok(text) = response.text() else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_MODIFY_MATERIAL_PARAMS.to_owned(), llsd))
            .ok();
    }
}

/// Spawns the modern `NewFileAgentInventory` two-step CAPS upload on a background
/// thread, emitting [`SlSessionEvent::AssetUploaded`] /
/// [`SlSessionEvent::AssetUploadFailed`] over the asset channel. Emits a failure
/// immediately if the asset/inventory type is not uploadable or the capability
/// is unavailable.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the flat NewFileAgentInventory upload command fields"
)]
fn spawn_new_file_upload(
    caps: Option<&Caps>,
    folder_id: Uuid,
    asset_type: AssetType,
    inventory_type: InventoryType,
    name: &str,
    description: &str,
    next_owner_mask: u32,
    group_mask: u32,
    everyone_mask: u32,
    expected_upload_cost: i32,
    data: Vec<u8>,
) {
    let (Some(asset_name), Some(inv_name)) =
        (asset_type.caps_asset_name(), inventory_type.caps_name())
    else {
        emit_upload_failure(caps, "asset/inventory type is not uploadable".to_owned());
        return;
    };
    let Some(caps) = caps else {
        return;
    };
    let Some(url) = caps.map.get(CAP_NEW_FILE_AGENT_INVENTORY).cloned() else {
        let asset_tx = caps.asset_tx.clone();
        asset_tx
            .send(SessionEvent::AssetUploadFailed {
                reason: "NewFileAgentInventory capability not available".to_owned(),
            })
            .ok();
        return;
    };
    let body = build_new_file_agent_inventory_request(
        folder_id,
        asset_name,
        inv_name,
        name,
        description,
        next_owner_mask,
        group_mask,
        everyone_mask,
        expected_upload_cost,
    );
    let asset_tx = caps.asset_tx.clone();
    std::thread::spawn(move || {
        let event = run_caps_upload(&url, body, data);
        asset_tx.send(event).ok();
    });
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel naming a
/// capability that is not available on the current region.
fn emit_upload_unavailable(caps: Option<&Caps>, cap: &str) {
    emit_upload_failure(caps, format!("{cap} capability not available"));
}

/// Emits an [`SlSessionEvent::AssetUploadFailed`] over the asset channel with the
/// given reason (a no-op if no capabilities are established yet).
fn emit_upload_failure(caps: Option<&Caps>, reason: String) {
    if let Some(caps) = caps {
        caps.asset_tx
            .send(SessionEvent::AssetUploadFailed { reason })
            .ok();
    }
}

/// Runs both steps of a modern CAPS asset upload synchronously (on the calling
/// background thread): POST the LLSD `metadata` to `cap_url` for an `uploader`
/// URL, then POST the raw `data` bytes there. Returns
/// [`SlSessionEvent::AssetUploaded`] on success or
/// [`SlSessionEvent::AssetUploadFailed`] on any failure.
fn run_caps_upload(cap_url: &str, metadata: String, data: Vec<u8>) -> SessionEvent {
    // Step 1: POST the metadata, expecting an `uploader` URL back.
    let uploader = match caps_upload_step(cap_url, "application/llsd+xml", metadata.into_bytes()) {
        Ok(response) => match response.uploader {
            Some(url) => url,
            None => {
                return SessionEvent::AssetUploadFailed {
                    reason: response.error.unwrap_or_else(|| {
                        format!("upload metadata rejected (state {})", response.state)
                    }),
                };
            }
        },
        Err(reason) => return SessionEvent::AssetUploadFailed { reason },
    };
    // Step 2: POST the raw asset bytes to the uploader URL.
    match caps_upload_step(&uploader, "application/octet-stream", data) {
        Ok(response) => match response.new_asset {
            Some(new_asset) => SessionEvent::AssetUploaded {
                new_asset,
                new_inventory_item: response.new_inventory_item,
            },
            None => SessionEvent::AssetUploadFailed {
                reason: response.error.unwrap_or_else(|| {
                    format!("upload did not complete (state {})", response.state)
                }),
            },
        },
        Err(reason) => SessionEvent::AssetUploadFailed { reason },
    }
}

/// POSTs one step of a CAPS upload (blocking) and parses the LLSD response,
/// returning the parsed [`AssetUploadResponse`] or a failure reason.
fn caps_upload_step(
    url: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<AssetUploadResponse, String> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .map_err(|error| format!("HTTP client build failed: {error}"))?;
    let response = http
        .post(url)
        .header("Content-Type", content_type)
        .body(body)
        .send()
        .map_err(|error| format!("upload request failed: {error}"))?;
    let text = response
        .text()
        .map_err(|error| format!("upload response read failed: {error}"))?;
    parse_asset_upload_response(&text)
        .map_err(|error| format!("upload response parse failed: {error}"))
}

/// Performs a blocking HTTP `GET`, returning the body bytes on a 2xx response,
/// or `None` on any network/HTTP failure.
fn blocking_get_bytes(url: &str) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http.get(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// GETs a texture from the `GetTexture` capability and forwards a
/// [`SlSessionEvent::TextureReceived`] (its bytes truncated to the
/// `discard_level` LOD prefix via [`j2c::truncate_to_discard`] when non-zero) or
/// a [`SlSessionEvent::TextureNotFound`] over `asset_tx`.
fn run_texture_fetch(
    cap_url: &str,
    texture_id: Uuid,
    discard_level: u8,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match blocking_get_bytes(&url) {
        Some(bytes) => {
            let data = j2c::truncate_to_discard(&bytes, discard_level).to_vec();
            SessionEvent::TextureReceived(Box::new(Texture {
                id: texture_id,
                codec: ImageCodec::J2c,
                data,
            }))
        }
        None => SessionEvent::TextureNotFound(texture_id),
    };
    asset_tx.send(event).ok();
}

/// GETs an asset from `{cap_url}/{query}` and forwards a
/// [`SlSessionEvent::AssetReceived`] (or a [`SlSessionEvent::AssetTransferFailed`]
/// with the 404-equivalent [`TransferStatus::UnknownSource`]) over `asset_tx`.
fn run_asset_fetch(
    cap_url: &str,
    query: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/{query}");
    let event = match blocking_get_bytes(&url) {
        Some(data) => SessionEvent::AssetReceived(Box::new(Asset {
            id: asset_id,
            asset_type,
            data,
        })),
        None => SessionEvent::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::UnknownSource,
        },
    };
    asset_tx.send(event).ok();
}

/// GETs a generic asset from the `GetAsset` capability using the asset class's
/// query key, forwarding the result over `asset_tx` (or an
/// [`SlSessionEvent::AssetTransferFailed`] for a class the cap cannot serve).
fn run_generic_asset_fetch(
    cap_url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    asset_tx: &Sender<SessionEvent>,
) {
    match asset_type.get_asset_query_key() {
        Some(key) => {
            run_asset_fetch(
                cap_url,
                &format!("?{key}={asset_id}"),
                asset_id,
                asset_type,
                asset_tx,
            );
        }
        None => {
            asset_tx
                .send(SessionEvent::AssetTransferFailed {
                    asset_id,
                    asset_type,
                    status: TransferStatus::Error,
                })
                .ok();
        }
    }
}

/// Emits a disconnect event.
fn emit_disconnect(events: &mut EventWriter<SlEvent>, reason: DisconnectReason) {
    events.write(SlEvent(SessionEvent::Disconnected(reason)));
}
