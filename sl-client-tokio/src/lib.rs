#![doc = include_str!("../README.md")]

use std::io::Error as IoError;
use std::time::{Duration, Instant};

use reqwest::Client as ReqwestClient;
use reqwest::Error as ReqwestError;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use std::collections::HashMap;

use sl_proto::{
    CAP_FETCH_INVENTORY, CAP_GET_ASSET, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE,
    CAP_GROUP_MEMBER_DATA, CAP_UPDATE_AVATAR_APPEARANCE, Llsd, REQUESTED_CAPABILITIES, Session,
    build_event_queue_request, build_fetch_inventory_request, build_group_member_data_request,
    build_seed_request, build_update_avatar_appearance_request, j2c, parse_event_queue_response,
    parse_llsd_xml, parse_login_response, parse_seed_response,
};

// Re-export the core types a consumer needs so they can depend on this crate
// alone.
pub use sl_proto::{
    ActiveGroup, AnyMessage, Asset, AssetType, AvatarGroupMembership, AvatarInterests, AvatarPick,
    AvatarProperties, ChatAudible, ChatMessage, ChatSourceType, ChatType, ClickAction,
    ControlFlags, CreateGroupParams, DeRezDestination, DisconnectReason, EconomyData,
    EstateAccessDelta, EstateAccessKind, EstateInfo, Event, Friend, FriendRights, GroupMember,
    GroupMembership, GroupNotice, GroupProfile, GroupRole, GroupRoleMember, GroupTitle, ImDialog,
    ImageCodec, InstantMessage, InventoryFolder, InventoryItem, LindenAmount, LoadUrlRequest,
    LoginParams, LoginRequest, LoginResponse, MapItem, MapItemType, MapRegionInfo, Material,
    Maturity, MfaChallenge, MoneyBalance, MoneyTransaction, MoneyTransactionType, MuteEntry,
    MuteFlags, MuteType, NeighborInfo, Object, ObjectFlagSettings, ObjectMotion, ObjectProperties,
    ObjectTransform, ParcelAccessEntry, ParcelAccessScope, ParcelCategory, ParcelFlags, ParcelInfo,
    ParcelOverlayInfo, ParcelReturnType, ParcelUpdate, PermissionField, PrimShape, ProductType,
    RegionFlags, RegionIdentity, RegionInfoUpdate, RegionLimits, Reliability, Rotation, SaleType,
    ScriptDialog, ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest,
    TerrainLayerType, TerrainPatch, Texture, TextureEntry, TextureFace, Throttle, TransferStatus,
    Transmit, Uuid, Vector, Wearable, WearableType, avatar_texture, decode_texture_entry,
    grid_to_handle, handle_to_global, handle_to_grid, pcode, sim_access,
};

/// The maximum UDP datagram size we are prepared to receive.
const RECV_BUFFER_SIZE: usize = 0x1_0000;

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
}

/// A command sent to a running [`Client`].
#[derive(Debug)]
pub enum Command {
    /// Send an application message.
    Send {
        /// The message to send.
        message: Box<AnyMessage>,
        /// How to deliver it.
        reliability: Reliability,
    },
    /// Send local chat via `ChatFromViewer`. Incoming chat arrives as an
    /// [`Event::ChatReceived`].
    Chat {
        /// The message text.
        message: String,
        /// The chat type (whisper / normal / shout / …).
        chat_type: ChatType,
        /// The chat channel (`0` for ordinary local chat).
        channel: i32,
    },
    /// Broadcast a local-chat typing indicator (`true` = start, `false` = stop).
    /// Other clients see it as an [`Event::ChatTyping`].
    Typing(bool),
    /// Send a direct (1:1) instant message. Incoming IMs arrive as an
    /// [`Event::InstantMessageReceived`].
    InstantMessage {
        /// The recipient's agent id.
        to_agent_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Send an instant-message typing indicator to `to_agent_id` (`true` = start,
    /// `false` = stop). Other clients see it as an [`Event::ImTyping`].
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
    /// result arrives as an [`Event::SitResult`].
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
    /// Request an avatar's profile. Replies arrive as [`Event::AvatarProperties`],
    /// [`Event::AvatarInterests`], and [`Event::AvatarGroups`].
    RequestAvatarProperties(Uuid),
    /// Request an avatar's picks. The reply arrives as [`Event::AvatarPicks`].
    RequestAvatarPicks(Uuid),
    /// Request the agent's private notes about an avatar. The reply arrives as
    /// [`Event::AvatarNotes`].
    RequestAvatarNotes(Uuid),
    /// Request the contents (sub-folders and items) of an inventory folder over
    /// **UDP** (`FetchInventoryDescendents`). The reply arrives as
    /// [`Event::InventoryDescendents`]. The full folder skeleton arrives once at
    /// login as [`Event::InventorySkeleton`].
    RequestFolderContents(Uuid),
    /// Fetch the contents of one or more inventory folders over the **HTTP CAPS**
    /// path (`FetchInventoryDescendents2`) — the modern path used on Second Life.
    /// Each folder's contents arrive as an [`Event::InventoryDescendents`].
    FetchInventoryFolders(Vec<Uuid>),
    /// Set the friendship rights granted to a friend (`GrantUserRights`). The
    /// `rights` bitfield combines the [`FriendRights`] `CAN_*` flags. The change
    /// is echoed back as an [`Event::FriendRightsChanged`].
    GrantUserRights {
        /// The friend whose granted rights to set.
        target: Uuid,
        /// The new rights bitfield (combine `FriendRights::CAN_*`).
        rights: FriendRights,
    },
    /// Offer friendship to an agent (`ImprovedInstantMessage`,
    /// `IM_FRIENDSHIP_OFFERED`). The offer arrives at the recipient as an
    /// [`Event::InstantMessageReceived`] with [`ImDialog::FriendshipOffered`].
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
    /// by [`Event::ActiveGroupChanged`].
    ActivateGroup(Uuid),
    /// Request a group's member roster over **UDP** (`GroupMembersRequest`).
    /// Replies arrive as [`Event::GroupMembers`].
    RequestGroupMembers(Uuid),
    /// Fetch a group's member roster over the **HTTP CAPS** path
    /// (`GroupMemberData`) — the modern path used on Second Life. The roster
    /// arrives as an [`Event::GroupMembers`].
    FetchGroupMembers(Uuid),
    /// Request a group's roles. The reply arrives as [`Event::GroupRoleData`].
    RequestGroupRoles(Uuid),
    /// Request a group's role↔member pairings. The reply arrives as
    /// [`Event::GroupRoleMembers`].
    RequestGroupRoleMembers(Uuid),
    /// Request the agent's selectable titles in a group. The reply arrives as
    /// [`Event::GroupTitles`].
    RequestGroupTitles(Uuid),
    /// Request a group's profile. The reply arrives as
    /// [`Event::GroupProfileReceived`].
    RequestGroupProfile(Uuid),
    /// Request a group's notice list. The reply arrives as [`Event::GroupNotices`].
    RequestGroupNotices(Uuid),
    /// Request a single group notice's full body (by notice id). Delivered as an
    /// [`Event::InstantMessageReceived`] with the group-notice dialog.
    RequestGroupNotice(Uuid),
    /// Create a new group. The result arrives as [`Event::CreateGroupResult`].
    CreateGroup(CreateGroupParams),
    /// Join an open-enrollment group. The result arrives as
    /// [`Event::JoinGroupResult`].
    JoinGroup(Uuid),
    /// Leave a group. The result arrives as [`Event::LeaveGroupResult`].
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
    /// messages then arrive as [`Event::GroupSessionMessage`].
    StartGroupSession(Uuid),
    /// Send a message into a group's IM session. Other members receive it as
    /// [`Event::GroupSessionMessage`].
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
    /// [`Event::ScriptDialog`] — the chosen button on its hidden `chat_channel`.
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
    /// [`Event::ScriptPermissionRequest`] — grants `permissions` (a subset of
    /// those requested; [`ScriptPermissions::default`] denies everything).
    AnswerScriptPermissions {
        /// The task (object) id holding the script.
        task_id: Uuid,
        /// The script item id.
        item_id: Uuid,
        /// The permissions to grant.
        permissions: ScriptPermissions,
    },
    /// Request the agent's mute (block) list (`MuteListRequest`). The list
    /// arrives as [`Event::MuteList`] (or [`Event::MuteListUnchanged`]).
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
    /// reply arrives as [`Event::ParcelAccessList`].
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
    /// arrives as [`Event::ParcelDwell`].
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
    /// arrive as [`Event::EstateInfo`] and [`Event::EstateAccessList`].
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
    /// as [`Event::MoneyBalance`].
    RequestMoneyBalance,
    /// Request the grid's economy data (`EconomyDataRequest`); the reply arrives
    /// as [`Event::EconomyData`].
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
    /// indices); each region arrives as an [`Event::MapBlock`].
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
    /// arrive as [`Event::MapBlock`].
    RequestMapByName {
        /// The region name (or prefix) to search for.
        name: String,
    },
    /// Request world-map overlay items of a given type (`MapItemRequest`); the
    /// reply arrives as [`Event::MapItems`].
    RequestMapItems {
        /// The kind of item to request (avatars, telehubs, land for sale, …).
        item_type: MapItemType,
        /// The target region handle (0 = the current region).
        region_handle: u64,
    },
    /// Request the full `ObjectUpdate` for the given region-local ids
    /// (`RequestMultipleObjects`); updates arrive as [`Event::ObjectAdded`] /
    /// [`Event::ObjectUpdated`].
    RequestObjects {
        /// The region-local ids to (re)fetch.
        local_ids: Vec<u32>,
    },
    /// Request objects' extended properties by selecting them (`ObjectSelect`);
    /// the reply arrives as [`Event::ObjectProperties`].
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
    /// reassembled image arrives as [`Event::TextureReceived`] (or
    /// [`Event::TextureNotFound`]).
    RequestTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: i8,
        /// The download priority (larger is fetched sooner).
        priority: f32,
    },
    /// Request a generic asset over the UDP transfer path (`TransferRequest`);
    /// the reassembled asset arrives as [`Event::AssetReceived`] (or
    /// [`Event::AssetTransferFailed`]).
    RequestAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class.
        asset_type: AssetType,
        /// The transfer priority.
        priority: f32,
    },
    /// Fetch a texture over the HTTP `GetTexture` capability; the image arrives
    /// as [`Event::TextureReceived`] (or [`Event::TextureNotFound`]). When
    /// `discard_level` is non-zero the codestream is truncated to that
    /// level-of-detail prefix via [`j2c`].
    FetchTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: u8,
    },
    /// Fetch a mesh asset over the HTTP `GetMesh2`/`GetMesh` capability; the data
    /// arrives as [`Event::AssetReceived`].
    FetchMesh {
        /// The mesh asset's id.
        mesh_id: Uuid,
    },
    /// Fetch a generic asset over the HTTP `GetAsset` capability; the data
    /// arrives as [`Event::AssetReceived`] (or [`Event::AssetTransferFailed`]).
    FetchAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class (selects the cap query parameter).
        asset_type: AssetType,
    },
    /// Ask the simulator to (re-)send the agent's own wearables
    /// (`AgentWearablesRequest`); the reply arrives as [`Event::AgentWearables`].
    RequestWearables,
    /// Set the agent's outfit (`AgentIsNowWearing`): the complete set of
    /// wearables to wear. The simulator acknowledges with a fresh
    /// [`Event::AgentWearables`].
    SetWearing(Vec<Wearable>),
    /// Advertise the agent's own appearance (`AgentSetAppearance`): the legacy
    /// client-side bake path (used by OpenSim and pre-server-baking regions).
    /// `serial` must strictly increase across calls.
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
    /// reply arrives as [`Event::CachedTextureResponse`].
    RequestCachedTextures {
        /// The serial echoed back in the reply.
        serial: i32,
        /// The queried slots, as `(cache id, texture slot index)` pairs.
        slots: Vec<(Uuid, u8)>,
    },
    /// Trigger a modern server-side appearance bake over the HTTP
    /// `UpdateAvatarAppearance` capability (Second Life "central baking"): the
    /// grid composites the agent's Current Outfit Folder and broadcasts the
    /// result as [`Event::AvatarAppearance`]. The POST's own reply arrives as
    /// [`Event::ServerAppearanceUpdate`].
    RequestServerAppearanceUpdate {
        /// The Current Outfit Folder version the grid should bake.
        cof_version: i32,
    },
    /// Begin a clean logout.
    Logout,
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
            .post(&request.url)
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
        })
    }

    /// Runs the session until it is disconnected or logged out, forwarding
    /// events to `events` and applying commands from `commands`.
    ///
    /// # Errors
    ///
    /// Returns an [`enum@Error`] on an unrecoverable socket or protocol error.
    pub async fn run(
        mut self,
        events: mpsc::Sender<Event>,
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
        let mut caps = fetch_capabilities(self.session.seed_capability(), &http).await;
        let mut caps_task = spawn_event_queue(&caps, &http, &caps_tx);

        loop {
            while let Some(transmit) = self.session.poll_transmit() {
                self.socket
                    .send_to(&transmit.payload, transmit.destination)
                    .await?;
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
                events.send(event).await.ok();
                if region_changed {
                    abort_task(&mut caps_task);
                    caps = fetch_capabilities(self.session.seed_capability(), &http).await;
                    caps_task = spawn_event_queue(&caps, &http, &caps_tx);
                }
                if terminal {
                    abort_task(&mut caps_task);
                    return Ok(());
                }
            }
            if self.session.is_closed() {
                abort_task(&mut caps_task);
                return Ok(());
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
                        self.session.handle_caps_event(&message, &body, Instant::now())?;
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
                        Some(Command::RequestFolderContents(folder_id)) => {
                            self.session.request_folder_contents(folder_id, Instant::now())?;
                        }
                        Some(Command::FetchInventoryFolders(folder_ids)) => {
                            if let (Some(url), Some(owner)) =
                                (caps.get(CAP_FETCH_INVENTORY).cloned(), self.session.agent_id())
                            {
                                tokio::spawn(fetch_inventory(
                                    url, owner, folder_ids, http.clone(), caps_tx.clone(),
                                ));
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
                        Some(Command::AcceptFriendship { transaction_id, calling_card_folder }) => {
                            self.session.accept_friendship(transaction_id, calling_card_folder, Instant::now())?;
                        }
                        Some(Command::DeclineFriendship(transaction_id)) => {
                            self.session.decline_friendship(transaction_id, Instant::now())?;
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
                        }
                        Some(Command::LeaveGroupSession(group_id)) => {
                            self.session.leave_group_session(group_id, Instant::now())?;
                        }
                        Some(Command::ReplyScriptDialog { object_id, chat_channel, button_index, button_label }) => {
                            self.session.reply_script_dialog(object_id, chat_channel, button_index, &button_label, Instant::now())?;
                        }
                        Some(Command::AnswerScriptPermissions { task_id, item_id, permissions }) => {
                            self.session.answer_script_permissions(task_id, item_id, permissions, Instant::now())?;
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
                        Some(Command::DerezObjects { local_ids, destination, destination_id, transaction_id, group_id }) => {
                            self.session.derez_objects(&local_ids, destination, destination_id, transaction_id, group_id, Instant::now())?;
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
                            self.session.update_parcel_access_list(local_id, scope, &entries, Instant::now())?;
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
                        Some(Command::RequestAsset { asset_id, asset_type, priority }) => {
                            self.session.request_asset(asset_id, asset_type, priority, Instant::now())?;
                        }
                        Some(Command::FetchTexture { texture_id, discard_level }) => {
                            if let Some(url) = caps.get(CAP_GET_TEXTURE).cloned() {
                                tokio::spawn(fetch_texture_http(
                                    url, texture_id, discard_level, http.clone(), events.clone(),
                                ));
                            }
                        }
                        Some(Command::FetchMesh { mesh_id }) => {
                            // GetMesh2 is preferred when offered; fall back to GetMesh.
                            if let Some(url) = caps.get(CAP_GET_MESH2).or_else(|| caps.get(CAP_GET_MESH)).cloned() {
                                tokio::spawn(fetch_mesh_http(url, mesh_id, http.clone(), events.clone()));
                            }
                        }
                        Some(Command::FetchAsset { asset_id, asset_type }) => {
                            if let Some(url) = caps.get(CAP_GET_ASSET).cloned() {
                                tokio::spawn(fetch_asset_http(
                                    url, asset_id, asset_type, http.clone(), events.clone(),
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

/// Aborts a running task handle, if present.
fn abort_task(task: &mut Option<tokio::task::JoinHandle<()>>) {
    if let Some(handle) = task.take() {
        handle.abort();
    }
}

/// Fetches the region's capability map by POSTing the seed with the requested
/// capability names, returning the cap-name → URL map (empty on any failure or
/// if no seed is known yet).
async fn fetch_capabilities(seed: Option<&str>, http: &ReqwestClient) -> HashMap<String, String> {
    let Some(seed_url) = seed else {
        return HashMap::new();
    };
    let result = http
        .post(seed_url)
        .header("Content-Type", "application/llsd+xml")
        .body(build_seed_request(REQUESTED_CAPABILITIES))
        .send()
        .await;
    let Ok(response) = result else {
        return HashMap::new();
    };
    let Ok(text) = response.text().await else {
        return HashMap::new();
    };
    parse_seed_response(&text).unwrap_or_default()
}

/// Spawns the event-queue long-poll task for the `EventQueueGet` capability in
/// `caps`, or `None` if the region did not provide one.
fn spawn_event_queue(
    caps: &HashMap<String, String>,
    http: &ReqwestClient,
    caps_tx: &mpsc::Sender<(String, Llsd)>,
) -> Option<tokio::task::JoinHandle<()>> {
    let event_queue_url = caps.get("EventQueueGet")?.clone();
    Some(tokio::spawn(run_event_queue(
        event_queue_url,
        http.clone(),
        caps_tx.clone(),
    )))
}

/// POSTs a `FetchInventoryDescendents2` request for `folder_ids` and forwards the
/// LLSD response to `caps_tx` tagged [`CAP_FETCH_INVENTORY`], for the session to
/// decode into [`Event::InventoryDescendents`].
async fn fetch_inventory(
    cap_url: String,
    owner_id: Uuid,
    folder_ids: Vec<Uuid>,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_fetch_inventory_request(owner_id, &folder_ids);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_FETCH_INVENTORY.to_owned(), llsd))
            .await
            .ok();
    }
}

/// POSTs the `GroupMemberData` capability for `group_id`, forwarding the decoded
/// LLSD roster back over `caps_tx` to be surfaced as an [`Event::GroupMembers`].
async fn fetch_group_members(
    cap_url: String,
    group_id: Uuid,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_group_member_data_request(group_id);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_GROUP_MEMBER_DATA.to_owned(), llsd))
            .await
            .ok();
    }
}

/// POSTs the `UpdateAvatarAppearance` capability for `cof_version` (the modern
/// Second Life server-side bake), forwarding the LLSD reply back over `caps_tx`
/// to be surfaced as an [`Event::ServerAppearanceUpdate`]. The baked appearance
/// itself arrives separately as a UDP [`Event::AvatarAppearance`].
async fn request_server_appearance_update(
    cap_url: String,
    cof_version: i32,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let body = build_update_avatar_appearance_request(cof_version);
    let Ok(response) = http
        .post(&cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await
    else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    if let Ok(llsd) = parse_llsd_xml(&text) {
        caps_tx
            .send((CAP_UPDATE_AVATAR_APPEARANCE.to_owned(), llsd))
            .await
            .ok();
    }
}

/// GETs a texture from the `GetTexture` capability and surfaces it as an
/// [`Event::TextureReceived`] (its bytes truncated to the `discard_level` LOD
/// prefix via [`j2c::truncate_to_discard`] when non-zero), or an
/// [`Event::TextureNotFound`] on a 404 / network failure.
async fn fetch_texture_http(
    cap_url: String,
    texture_id: Uuid,
    discard_level: u8,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match http.get(&url).header("Accept", "image/x-j2c").send().await {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(bytes) => {
                let data = j2c::truncate_to_discard(&bytes, discard_level).to_vec();
                Event::TextureReceived(Box::new(Texture {
                    id: texture_id,
                    codec: ImageCodec::J2c,
                    data,
                }))
            }
            Err(_error) => Event::TextureNotFound(texture_id),
        },
        _ => Event::TextureNotFound(texture_id),
    };
    events.send(event).await.ok();
}

/// GETs a mesh asset from the `GetMesh2`/`GetMesh` capability and surfaces it as
/// an [`Event::AssetReceived`] (or [`Event::AssetTransferFailed`] on failure).
async fn fetch_mesh_http(
    cap_url: String,
    mesh_id: Uuid,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let url = format!("{cap_url}/?mesh_id={mesh_id}");
    let event = http_asset_event(&http, &url, mesh_id, AssetType::Mesh).await;
    events.send(event).await.ok();
}

/// GETs a generic asset from the `GetAsset` capability (using the asset class's
/// query parameter) and surfaces it as an [`Event::AssetReceived`] (or
/// [`Event::AssetTransferFailed`] on failure / an unsupported class).
async fn fetch_asset_http(
    cap_url: String,
    asset_id: Uuid,
    asset_type: AssetType,
    http: ReqwestClient,
    events: mpsc::Sender<Event>,
) {
    let event = match asset_type.get_asset_query_key() {
        Some(key) => {
            let url = format!("{cap_url}/?{key}={asset_id}");
            http_asset_event(&http, &url, asset_id, asset_type).await
        }
        None => Event::AssetTransferFailed {
            asset_id,
            asset_type,
            status: TransferStatus::Error,
        },
    };
    events.send(event).await.ok();
}

/// Performs an HTTP `GET` for an asset and builds the resulting event: an
/// [`Event::AssetReceived`] on success, or an [`Event::AssetTransferFailed`]
/// (with [`TransferStatus::UnknownSource`], the 404 equivalent) on any failure.
async fn http_asset_event(
    http: &ReqwestClient,
    url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
) -> Event {
    let failed = Event::AssetTransferFailed {
        asset_id,
        asset_type,
        status: TransferStatus::UnknownSource,
    };
    match http.get(url).send().await {
        Ok(response) if response.status().is_success() => match response.bytes().await {
            Ok(bytes) => Event::AssetReceived(Box::new(Asset {
                id: asset_id,
                asset_type,
                data: bytes.to_vec(),
            })),
            Err(_error) => failed,
        },
        _ => failed,
    }
}

/// Long-polls the `EventQueueGet` capability at `event_queue_url`, forwarding each
/// decoded event to `caps_tx` until a request fails fatally or the receiver is
/// dropped (e.g. on region change).
async fn run_event_queue(
    event_queue_url: String,
    http: ReqwestClient,
    caps_tx: mpsc::Sender<(String, Llsd)>,
) {
    let mut ack: Option<i32> = None;
    loop {
        let request_body = build_event_queue_request(ack, false);
        let response = match http
            .post(&event_queue_url)
            .header("Content-Type", "application/llsd+xml")
            .body(request_body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(_error) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        // A timeout with no events returns a non-2xx (e.g. 502); re-poll with
        // the same ack after a short pause.
        if !response.status().is_success() {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        let Ok(text) = response.text().await else {
            continue;
        };
        let Ok(parsed) = parse_event_queue_response(&text) else {
            continue;
        };
        ack = Some(parsed.id);
        for event in parsed.events {
            if caps_tx.send((event.message, event.body)).await.is_err() {
                return;
            }
        }
    }
}

/// Builds a sleep future firing at `deadline`, or far in the future when there
/// is no scheduled timeout.
fn make_sleep(deadline: Option<Instant>) -> tokio::time::Sleep {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)),
        None => tokio::time::sleep(IDLE_SLEEP),
    }
}
