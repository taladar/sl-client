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
    AssetUploadResponse, CAP_AGENT_EXPERIENCES, CAP_CREATE_INVENTORY_CATEGORY,
    CAP_EXPERIENCE_PREFERENCES, CAP_FETCH_INVENTORY, CAP_FIND_EXPERIENCE_BY_NAME,
    CAP_GET_ADMIN_EXPERIENCES, CAP_GET_ASSET, CAP_GET_CREATOR_EXPERIENCES, CAP_GET_EXPERIENCE_INFO,
    CAP_GET_EXPERIENCES, CAP_GET_MESH, CAP_GET_MESH2, CAP_GET_TEXTURE, CAP_GROUP_EXPERIENCES,
    CAP_GROUP_MEMBER_DATA, CAP_INVENTORY_API_V3, CAP_IS_EXPERIENCE_ADMIN,
    CAP_IS_EXPERIENCE_CONTRIBUTOR, CAP_MODIFY_MATERIAL_PARAMS, CAP_NEW_FILE_AGENT_INVENTORY,
    CAP_OBJECT_MEDIA, CAP_OBJECT_MEDIA_NAVIGATE, CAP_PARCEL_VOICE_INFO,
    CAP_PROVISION_VOICE_ACCOUNT, CAP_READ_OFFLINE_MSGS, CAP_REGION_EXPERIENCES,
    CAP_RENDER_MATERIALS, CAP_UPDATE_AVATAR_APPEARANCE, CAP_UPDATE_EXPERIENCE,
    CAP_UPLOAD_BAKED_TEXTURE, CAP_VOICE_SIGNALING, Event as SessionEvent, Llsd, LoginResponse,
    REQUESTED_CAPABILITIES, Session, ais_category_children_fetch_url, ais_category_children_url,
    ais_category_url, ais_create_category_url, ais_item_url, build_ais_create_category_body,
    build_ais_move_body, build_ais_rename_category_body, build_ais_update_item_body,
    build_create_inventory_category_request, build_event_queue_request,
    build_fetch_inventory_request, build_group_member_data_request,
    build_modify_material_params_request, build_new_file_agent_inventory_request,
    build_object_media_get_request, build_object_media_navigate_request,
    build_object_media_update_request, build_parcel_voice_info_request,
    build_provision_voice_account_request, build_region_experiences_request,
    build_render_materials_request, build_seed_request, build_set_experience_permission_request,
    build_update_avatar_appearance_request, build_update_experience_request,
    build_update_item_asset_request, build_upload_baked_texture_request,
    build_voice_signaling_request, experience_id_query, experience_info_query,
    find_experience_query, forget_experience_query, group_experiences_query, j2c,
    parse_asset_upload_response, parse_event_queue_response, parse_experience_ids,
    parse_experience_status, parse_llsd_xml, parse_login_response, parse_render_materials_response,
    parse_seed_response,
};

// Re-export the core types a consumer needs to configure the plugin, drive the
// survey commands, and read events. `Event` is aliased to avoid clashing with
// Bevy's `Event` derive.
pub use sl_proto::{
    ActiveGroup, AnyMessage, AvatarClassified, AvatarGroupMembership, AvatarInterests, AvatarPick,
    AvatarProperties, Camera, ChatAudible, ChatMessage, ChatSourceType, ChatType, ClassifiedInfo,
    ClassifiedUpdate, ClickAction, ControlFlags, CreateGroupParams, DeRezDestination,
    DisconnectReason, EconomyData, EstateAccessDelta, EstateAccessKind, EstateInfo, ExperienceInfo,
    ExperiencePermission, ExperienceProperties, ExperienceUpdate, ExtendedMesh, FlexibleData,
    Friend, FriendRights, GltfMaterialOverride, GroupMember, GroupMembership, GroupNotice,
    GroupNoticeAttachment, GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit,
    GroupRoleMember, GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, IceCandidate,
    ImDialog, InstantMessage, InterestsUpdate, InventoryFolder, InventoryItem, InventoryOffer,
    InventoryType, LandingType, LegacyMaterial, LightData, LightImage, LindenAmount,
    LoadUrlRequest, LoginParams, LoginRequest, MEDIA_PERM_ALL, MEDIA_PERM_ANYONE, MEDIA_PERM_GROUP,
    MEDIA_PERM_NONE, MEDIA_PERM_OWNER, MapItem, MapItemType, MapRegionInfo, Material,
    MaterialOverrideUpdate, Maturity, MediaEntry, MfaChallenge, MoneyBalance, MoneyTransaction,
    MoneyTransactionType, MuteEntry, MuteFlags, MuteType, NeighborInfo, NewInventoryItem, Object,
    ObjectExtraParams, ObjectFlagSettings, ObjectMediaResponse, ObjectMotion, ObjectProperties,
    ObjectTransform, ParcelAccessEntry, ParcelAccessScope, ParcelCategory, ParcelFlags, ParcelInfo,
    ParcelMediaCommand, ParcelMediaUpdateInfo, ParcelOverlayInfo, ParcelRequestResult,
    ParcelReturnType, ParcelStatus, ParcelUpdate, ParcelVoiceInfo, PermissionField, PickInfo,
    PickUpdate, PlayingAnimation, PrimShape, PrimShapeParams, ProductType, ProfileUpdate,
    ReflectionProbe, RegionFlags, RegionIdentity, RegionInfoUpdate, RegionLimits, Reliability,
    RenderMaterialEntry, RenderMaterialRef, Rotation, SaleType, ScriptDialog,
    ScriptPermissionRequest, ScriptPermissions, ScriptTeleportRequest, SculptData, SoundFlags,
    SoundPreload, TerrainLayerType, TerrainPatch, TextureEntry, TextureFace, Throttle, Transmit,
    Uuid, Vector, VoiceAccountInfo, VoiceProvisionRequest, Wearable, WearableType, avatar_texture,
    decode_texture_entry, grid_to_handle, group_powers, handle_to_global, handle_to_grid, pcode,
    sim_access,
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
    /// Set the agent's camera viewpoint (position and look axes); the simulator
    /// uses it to build the interest list, so the streamed scene follows where
    /// the agent looks. Build one with [`Camera::looking_at`] or directly.
    SetCamera(Camera),
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
    /// Request an avatar's classified ads. The reply arrives as
    /// [`SlSessionEvent::AvatarClassifieds`].
    RequestAvatarClassifieds(Uuid),
    /// Request the full details of one pick. `creator_id` is the pick's owner
    /// (the `target_id` from [`SlSessionEvent::AvatarPicks`]). The reply arrives
    /// as [`SlSessionEvent::PickInfo`].
    RequestPickInfo {
        /// The avatar that owns the pick.
        creator_id: Uuid,
        /// The pick id.
        pick_id: Uuid,
    },
    /// Request the full details of one classified ad. The reply arrives as
    /// [`SlSessionEvent::ClassifiedInfo`].
    RequestClassifiedInfo(Uuid),
    /// Replace the agent's own profile (`AvatarPropertiesUpdate`).
    UpdateProfile(ProfileUpdate),
    /// Replace the agent's own interests (`AvatarInterestsUpdate`).
    UpdateInterests(InterestsUpdate),
    /// Set the agent's private notes about an avatar (`AvatarNotesUpdate`).
    UpdateAvatarNotes {
        /// The avatar the notes are about.
        target_id: Uuid,
        /// The note text.
        notes: String,
    },
    /// Create or edit one of the agent's picks (`PickInfoUpdate`).
    UpdatePick(PickUpdate),
    /// Delete one of the agent's picks (`PickDelete`).
    DeletePick(Uuid),
    /// Delete any agent's pick (`PickGodDelete`, god-only).
    GodDeletePick {
        /// The pick id.
        pick_id: Uuid,
        /// The query id for the dataserver to resend the pick list under.
        query_id: Uuid,
    },
    /// Create or edit one of the agent's classifieds (`ClassifiedInfoUpdate`).
    UpdateClassified(ClassifiedUpdate),
    /// Delete one of the agent's classifieds (`ClassifiedDelete`).
    DeleteClassified(Uuid),
    /// Delete any agent's classified (`ClassifiedGodDelete`, god-only).
    GodDeleteClassified {
        /// The classified id.
        classified_id: Uuid,
        /// The query id for the dataserver to resend the classified list under.
        query_id: Uuid,
    },
    /// Request the contents (sub-folders and items) of an inventory folder over
    /// **UDP** (`FetchInventoryDescendents`). The reply arrives as
    /// [`SlSessionEvent::InventoryDescendents`]. The full folder skeleton arrives
    /// once at login as [`SlSessionEvent::InventorySkeleton`].
    RequestFolderContents(Uuid),
    /// Fetch the contents of one or more inventory folders over the **HTTP CAPS**
    /// path (`FetchInventoryDescendents2`) — the modern path used on Second Life.
    /// Each folder's contents arrive as an [`SlSessionEvent::InventoryDescendents`].
    FetchInventoryFolders(Vec<Uuid>),
    /// Create an inventory folder (`CreateInventoryFolder`, UDP). `folder_id` is a
    /// fresh, caller-chosen id; the simulator sends no reply (cache updated
    /// optimistically). Use [`SlCommand::CreateInventoryCategory`] for a reply.
    CreateInventoryFolder {
        /// The new folder's id (a fresh, caller-chosen UUID).
        folder_id: Uuid,
        /// The parent folder.
        parent_id: Uuid,
        /// The folder's preferred type (`FolderType`, or `-1` for none).
        folder_type: i8,
        /// The folder name.
        name: String,
    },
    /// Rename / re-type / re-parent an existing folder (`UpdateInventoryFolder`).
    UpdateInventoryFolder {
        /// The folder to update.
        folder_id: Uuid,
        /// The (possibly new) parent folder.
        parent_id: Uuid,
        /// The folder's preferred type (`FolderType`, or `-1`).
        folder_type: i8,
        /// The folder name.
        name: String,
    },
    /// Move a folder under a new parent (`MoveInventoryFolder`).
    MoveInventoryFolder {
        /// The folder to move.
        folder_id: Uuid,
        /// The new parent folder.
        parent_id: Uuid,
    },
    /// Delete folders (to the server trash) via `RemoveInventoryFolder`.
    RemoveInventoryFolders(Vec<Uuid>),
    /// Create an inventory item (`CreateInventoryItem`). The simulator allocates
    /// the id and replies with an [`SlSessionEvent::InventoryItemCreated`].
    CreateInventoryItem(NewInventoryItem),
    /// Rewrite an item's metadata / permissions (`UpdateInventoryItem`). A non-nil
    /// `transaction_id` binds a freshly uploaded asset to the item.
    UpdateInventoryItem {
        /// The item, with its fields set to the desired values.
        item: Box<InventoryItem>,
        /// The asset transaction id (nil if not replacing the asset).
        transaction_id: Uuid,
    },
    /// Move an item into a folder, optionally renaming it (an empty `new_name`
    /// keeps the name), via `MoveInventoryItem`.
    MoveInventoryItem {
        /// The item to move.
        item_id: Uuid,
        /// The destination folder.
        folder_id: Uuid,
        /// The new name, or empty to keep the current name.
        new_name: String,
    },
    /// Copy an item into a folder (`CopyInventoryItem`). The simulator answers
    /// with an [`SlSessionEvent::InventoryBulkUpdate`] for the new item.
    CopyInventoryItem {
        /// The current owner of the source item.
        old_agent_id: Uuid,
        /// The source item.
        old_item_id: Uuid,
        /// The destination folder.
        new_folder_id: Uuid,
        /// The new item's name.
        new_name: String,
    },
    /// Delete items (`RemoveInventoryItem`).
    RemoveInventoryItems(Vec<Uuid>),
    /// Rewrite an item's flags (`ChangeInventoryItemFlags`).
    ChangeInventoryItemFlags {
        /// The item to change.
        item_id: Uuid,
        /// The new flags bitfield.
        flags: u32,
    },
    /// Empty a folder's contents (e.g. the Trash) via `PurgeInventoryDescendents`.
    PurgeInventoryDescendents(Uuid),
    /// Delete a mixed set of folders and items in one `RemoveInventoryObjects`.
    RemoveInventoryObjects {
        /// The folders to delete.
        folder_ids: Vec<Uuid>,
        /// The items to delete.
        item_ids: Vec<Uuid>,
    },
    /// Create a folder via the `CreateInventoryCategory` capability (served by
    /// both OpenSim and Second Life), returning a synchronous reply surfaced as
    /// an [`SlSessionEvent::InventoryBulkUpdate`]. The runtime allocates the id.
    CreateInventoryCategory {
        /// The parent folder.
        parent_id: Uuid,
        /// The folder's preferred type (`FolderType`, or `-1`).
        folder_type: i32,
        /// The folder name.
        name: String,
    },
    /// Create a folder over the modern **AIS3** (`InventoryAPIv3`) cap
    /// (Second-Life only). The affected objects arrive as an
    /// [`SlSessionEvent::InventoryBulkUpdate`].
    Ais3CreateFolder {
        /// The parent folder.
        parent_id: Uuid,
        /// The folder's preferred type (`FolderType`, or `-1`).
        folder_type: i32,
        /// The folder name.
        name: String,
    },
    /// Rename a folder over AIS3 (`PATCH /category/<id>`). Second-Life only.
    Ais3RenameFolder {
        /// The folder to rename.
        folder_id: Uuid,
        /// The new name.
        name: String,
    },
    /// Move a folder over AIS3 (`PATCH /category/<id>` with `{ parent_id }`).
    /// Second-Life only.
    Ais3MoveFolder {
        /// The folder to move.
        folder_id: Uuid,
        /// The new parent folder.
        parent_id: Uuid,
    },
    /// Delete a folder over AIS3 (`DELETE /category/<id>`). Second-Life only.
    Ais3RemoveFolder(Uuid),
    /// Empty a folder over AIS3 (`DELETE /category/<id>/children`). Second-Life
    /// only.
    Ais3PurgeFolder(Uuid),
    /// Fetch a folder's children over AIS3 (`GET /category/<id>/children?depth=`).
    /// Second-Life only; the result arrives as an
    /// [`SlSessionEvent::InventoryBulkUpdate`].
    Ais3FetchFolderChildren {
        /// The folder whose children to fetch.
        folder_id: Uuid,
        /// The recursion depth (clamped to the AIS maximum).
        depth: i32,
    },
    /// Update an item's name and description over AIS3 (`PATCH /item/<id>`).
    /// Second-Life only.
    Ais3UpdateItem {
        /// The item to update.
        item_id: Uuid,
        /// The new name.
        name: String,
        /// The new description.
        description: String,
    },
    /// Move an item over AIS3 (`PATCH /item/<id>` with `{ parent_id }`).
    /// Second-Life only.
    Ais3MoveItem {
        /// The item to move.
        item_id: Uuid,
        /// The new parent folder.
        parent_id: Uuid,
    },
    /// Delete an item over AIS3 (`DELETE /item/<id>`). Second-Life only.
    Ais3RemoveItem(Uuid),
    /// Fetch a single item over AIS3 (`GET /item/<id>`). Second-Life only; the
    /// item arrives as an [`SlSessionEvent::InventoryBulkUpdate`].
    Ais3FetchItem(Uuid),
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
    /// Create, update, or delete group roles (`GroupRoleUpdate`), one
    /// [`GroupRoleEdit`] per role. Re-request the roles to observe the change.
    UpdateGroupRoles {
        /// The group whose roles to edit.
        group_id: Uuid,
        /// The role create/update/delete edits.
        roles: Vec<GroupRoleEdit>,
    },
    /// Add members to or remove members from group roles (`GroupRoleChanges`).
    ChangeGroupRoleMembers {
        /// The group whose role assignments to change.
        group_id: Uuid,
        /// The member↔role add/remove changes.
        changes: Vec<GroupRoleMemberChange>,
    },
    /// Eject members from a group (`EjectGroupMemberRequest`). The result arrives
    /// as [`SlSessionEvent::EjectGroupMemberResult`].
    EjectGroupMembers {
        /// The group to eject from.
        group_id: Uuid,
        /// The agent ids to eject.
        member_ids: Vec<Uuid>,
    },
    /// Post a group notice (`IM_GROUP_NOTICE`), optionally attaching an inventory
    /// item. The grid relays it to members who accept notices.
    SendGroupNotice {
        /// The group to post to.
        group_id: Uuid,
        /// The notice subject.
        subject: String,
        /// The notice body.
        message: String,
        /// An optional inventory item to attach (must be copy+transfer).
        attachment: Option<GroupNoticeAttachment>,
    },
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
    /// arrives as [`SlSessionEvent::AssetReceived`]. An optional `byte_range`
    /// (inclusive `(start, end)` byte offsets) issues an HTTP `Range` request so
    /// only that span is transferred — e.g. a single mesh LOD whose offsets the
    /// caller read from the mesh header. `None` fetches the whole asset.
    FetchMesh {
        /// The mesh asset's id.
        mesh_id: Uuid,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
    },
    /// Fetch a generic asset over the HTTP `GetAsset` capability; the data
    /// arrives as [`SlSessionEvent::AssetReceived`] (or
    /// [`SlSessionEvent::AssetTransferFailed`]). An optional `byte_range`
    /// (inclusive `(start, end)` byte offsets) issues an HTTP `Range` request so
    /// only that span is transferred; `None` fetches the whole asset.
    FetchAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class (selects the cap query parameter).
        asset_type: AssetType,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
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
    /// Request voice-chat account credentials over the
    /// `ProvisionVoiceAccountRequest` capability. A [`VoiceProvisionRequest::vivox`]
    /// asks for legacy Vivox SIP credentials; a [`VoiceProvisionRequest::webrtc`]
    /// negotiates a WebRTC session (the JSEP offer SDP is supplied by the
    /// caller's own — out-of-scope — WebRTC engine). The reply arrives as
    /// [`SlSessionEvent::VoiceAccountProvisioned`]. This handles the grid
    /// *signalling* only; the audio session itself is the caller's concern.
    RequestVoiceAccount {
        /// The provision request (backend selection + WebRTC offer/logout).
        request: VoiceProvisionRequest,
    },
    /// Request the current parcel's voice channel over the
    /// `ParcelVoiceInfoRequest` capability. The reply arrives as
    /// [`SlSessionEvent::ParcelVoiceInfo`].
    RequestParcelVoiceInfo,
    /// Trickle WebRTC ICE candidates (or signal end-of-gathering) over the
    /// `VoiceSignalingRequest` capability, keyed by the `viewer_session` from a
    /// prior [`SlSessionEvent::VoiceAccountProvisioned`]. Fire-and-forget — the
    /// simulator returns only an HTTP status. The candidates come from the
    /// caller's out-of-scope WebRTC engine.
    SendVoiceSignaling {
        /// The viewer session id from the provision reply.
        viewer_session: String,
        /// The ICE candidates to trickle (empty with `completed` to end).
        candidates: Vec<IceCandidate>,
        /// Whether this marks the end of ICE gathering.
        completed: bool,
    },
    /// Fetch experience metadata over the `GetExperienceInfo` capability, batching
    /// every id into one request. The reply arrives as
    /// [`SlSessionEvent::ExperienceInfo`].
    RequestExperienceInfo {
        /// The experiences whose metadata to fetch.
        experience_ids: Vec<Uuid>,
    },
    /// Search experiences by name over the `FindExperienceByName` capability. The
    /// reply (one page) arrives as [`SlSessionEvent::ExperienceSearchResults`].
    FindExperiences {
        /// The search text.
        query: String,
        /// The zero-based result page.
        page: i32,
    },
    /// Fetch the agent's per-experience preferences over the `GetExperiences`
    /// capability. The reply arrives as [`SlSessionEvent::ExperiencePermissions`].
    RequestExperiencePermissions,
    /// Set (or forget) the agent's preference for one experience over the
    /// `ExperiencePreferences` capability (`Allow`/`Block` via PUT, `Forget` via
    /// DELETE). The updated lists arrive as [`SlSessionEvent::ExperiencePermissions`].
    SetExperiencePermission {
        /// The experience to set the preference for.
        experience_id: Uuid,
        /// The preference to apply.
        permission: ExperiencePermission,
    },
    /// Fetch the experiences the agent owns over the `AgentExperiences`
    /// capability. The reply arrives as [`SlSessionEvent::OwnedExperiences`].
    RequestOwnedExperiences,
    /// Fetch the experiences the agent administers over the `GetAdminExperiences`
    /// capability. The reply arrives as [`SlSessionEvent::AdminExperiences`].
    RequestAdminExperiences,
    /// Fetch the experiences the agent created over the `GetCreatorExperiences`
    /// capability. The reply arrives as [`SlSessionEvent::CreatorExperiences`].
    RequestCreatorExperiences,
    /// Fetch the experiences a group owns over the `GroupExperiences` capability.
    /// The reply arrives as [`SlSessionEvent::GroupExperiences`].
    RequestGroupExperiences {
        /// The group to query.
        group_id: Uuid,
    },
    /// Test whether the agent administers an experience over the
    /// `IsExperienceAdmin` capability. The reply arrives as
    /// [`SlSessionEvent::ExperienceAdminStatus`].
    RequestExperienceAdmin {
        /// The experience to test.
        experience_id: Uuid,
    },
    /// Test whether the agent contributes to an experience over the
    /// `IsExperienceContributor` capability. The reply arrives as
    /// [`SlSessionEvent::ExperienceContributorStatus`].
    RequestExperienceContributor {
        /// The experience to test.
        experience_id: Uuid,
    },
    /// Edit an experience's metadata over the `UpdateExperience` capability. The
    /// updated experience arrives as [`SlSessionEvent::ExperienceUpdated`].
    UpdateExperience {
        /// The editable experience metadata to write.
        update: ExperienceUpdate,
    },
    /// Read the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability. The reply arrives as
    /// [`SlSessionEvent::RegionExperiences`].
    RequestRegionExperiences,
    /// Replace the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability (estate-gated). The updated lists arrive as
    /// [`SlSessionEvent::RegionExperiences`].
    SetRegionExperiences {
        /// The experiences the region allows.
        allowed: Vec<Uuid>,
        /// The experiences the region blocks.
        blocked: Vec<Uuid>,
        /// The experiences the region trusts.
        trusted: Vec<Uuid>,
    },
    /// Offer a teleport ("lure") to each `targets` agent (`StartLure`, #28).
    OfferTeleport {
        /// The agents to invite.
        targets: Vec<Uuid>,
        /// The accompanying message.
        message: String,
    },
    /// Accept a teleport lure (`TeleportLureRequest`); `lure_id` is the offer
    /// IM's [`InstantMessage::id`].
    AcceptTeleportLure {
        /// The lure id from the offer IM.
        lure_id: Uuid,
    },
    /// Decline a teleport lure (`IM_LURE_DECLINED`).
    DeclineTeleportLure {
        /// The offer IM's sender.
        from_agent_id: Uuid,
        /// The lure id from the offer IM.
        lure_id: Uuid,
    },
    /// Request a teleport from `to_agent_id` (`IM_TELEPORT_REQUEST`).
    RequestTeleport {
        /// The agent to ask.
        to_agent_id: Uuid,
        /// The accompanying message.
        message: String,
    },
    /// Offer an inventory item to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`).
    GiveInventory {
        /// The recipient agent.
        to_agent_id: Uuid,
        /// The offered item's id.
        item_id: Uuid,
        /// The offered item's asset class.
        asset_type: AssetType,
        /// The item's name (shown to the recipient).
        item_name: String,
        /// A fresh transaction id echoed back on accept/decline.
        transaction_id: Uuid,
    },
    /// Offer an inventory folder to `to_agent_id` over IM (`IM_INVENTORY_OFFERED`).
    GiveInventoryFolder {
        /// The recipient agent.
        to_agent_id: Uuid,
        /// The offered folder's id.
        folder_id: Uuid,
        /// The folder's name (shown to the recipient).
        folder_name: String,
        /// A fresh transaction id echoed back on accept/decline.
        transaction_id: Uuid,
    },
    /// Accept an inventory offer (`IM_INVENTORY_ACCEPTED`), filing it into
    /// `folder_id`.
    AcceptInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The destination folder to file the item into.
        folder_id: Uuid,
    },
    /// Decline an inventory offer (`IM_INVENTORY_DECLINED`); routed to
    /// `trash_folder_id`.
    DeclineInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The trash folder the simulator routes the declined item to.
        trash_folder_id: Uuid,
    },
    /// Start (or add invitees to) an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`).
    StartConference {
        /// A fresh, caller-chosen session id naming the conference.
        session_id: Uuid,
        /// The agents to invite.
        invitees: Vec<Uuid>,
        /// The opening message.
        message: String,
    },
    /// Send a message into a conference / ad-hoc IM session (`IM_SESSION_SEND`).
    SendConferenceMessage {
        /// The conference session id.
        session_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Leave a conference / ad-hoc IM session (`IM_SESSION_LEAVE`).
    LeaveConference {
        /// The conference session id.
        session_id: Uuid,
    },
    /// Flush stored offline instant messages over the legacy UDP trigger
    /// (`RetrieveInstantMessages`).
    RetrieveInstantMessages,
    /// Read stored offline instant messages over the modern `ReadOfflineMsgs`
    /// capability.
    RequestOfflineMessages,
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
            SlCommand::SetCamera(camera) => {
                session.set_camera(camera.clone(), now).ok();
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
            SlCommand::RequestAvatarClassifieds(target) => {
                session.request_avatar_classifieds(*target, now).ok();
            }
            SlCommand::RequestPickInfo {
                creator_id,
                pick_id,
            } => {
                session.request_pick_info(*creator_id, *pick_id, now).ok();
            }
            SlCommand::RequestClassifiedInfo(classified_id) => {
                session.request_classified_info(*classified_id, now).ok();
            }
            SlCommand::UpdateProfile(update) => {
                session.update_profile(update, now).ok();
            }
            SlCommand::UpdateInterests(update) => {
                session.update_interests(update, now).ok();
            }
            SlCommand::UpdateAvatarNotes { target_id, notes } => {
                session.update_avatar_notes(*target_id, notes, now).ok();
            }
            SlCommand::UpdatePick(update) => {
                session.update_pick(update, now).ok();
            }
            SlCommand::DeletePick(pick_id) => {
                session.delete_pick(*pick_id, now).ok();
            }
            SlCommand::GodDeletePick { pick_id, query_id } => {
                session.god_delete_pick(*pick_id, *query_id, now).ok();
            }
            SlCommand::UpdateClassified(update) => {
                session.update_classified(update, now).ok();
            }
            SlCommand::DeleteClassified(classified_id) => {
                session.delete_classified(*classified_id, now).ok();
            }
            SlCommand::GodDeleteClassified {
                classified_id,
                query_id,
            } => {
                session
                    .god_delete_classified(*classified_id, *query_id, now)
                    .ok();
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
            SlCommand::CreateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .create_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            SlCommand::UpdateInventoryFolder {
                folder_id,
                parent_id,
                folder_type,
                name,
            } => {
                session
                    .update_inventory_folder(*folder_id, *parent_id, *folder_type, name, now)
                    .ok();
            }
            SlCommand::MoveInventoryFolder {
                folder_id,
                parent_id,
            } => {
                session
                    .move_inventory_folder(*folder_id, *parent_id, now)
                    .ok();
            }
            SlCommand::RemoveInventoryFolders(folder_ids) => {
                session.remove_inventory_folders(folder_ids, now).ok();
            }
            SlCommand::CreateInventoryItem(new) => {
                session.create_inventory_item(new, now).ok();
            }
            SlCommand::UpdateInventoryItem {
                item,
                transaction_id,
            } => {
                session
                    .update_inventory_item(item, *transaction_id, now)
                    .ok();
            }
            SlCommand::MoveInventoryItem {
                item_id,
                folder_id,
                new_name,
            } => {
                session
                    .move_inventory_item(*item_id, *folder_id, new_name, now)
                    .ok();
            }
            SlCommand::CopyInventoryItem {
                old_agent_id,
                old_item_id,
                new_folder_id,
                new_name,
            } => {
                session
                    .copy_inventory_item(*old_agent_id, *old_item_id, *new_folder_id, new_name, now)
                    .ok();
            }
            SlCommand::RemoveInventoryItems(item_ids) => {
                session.remove_inventory_items(item_ids, now).ok();
            }
            SlCommand::ChangeInventoryItemFlags { item_id, flags } => {
                session
                    .change_inventory_item_flags(*item_id, *flags, now)
                    .ok();
            }
            SlCommand::PurgeInventoryDescendents(folder_id) => {
                session.purge_inventory_descendents(*folder_id, now).ok();
            }
            SlCommand::RemoveInventoryObjects {
                folder_ids,
                item_ids,
            } => {
                session
                    .remove_inventory_objects(folder_ids, item_ids, now)
                    .ok();
            }
            SlCommand::CreateInventoryCategory {
                parent_id,
                folder_type,
                name,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_CREATE_INVENTORY_CATEGORY).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    let body = build_create_inventory_category_request(
                        Uuid::new_v4(),
                        *parent_id,
                        *folder_type,
                        name,
                    );
                    std::thread::spawn(move || {
                        run_voice_cap(&url, body, CAP_CREATE_INVENTORY_CATEGORY, &events_tx);
                    });
                }
            }
            SlCommand::Ais3CreateFolder {
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
            SlCommand::Ais3RenameFolder { folder_id, name } => {
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
            SlCommand::Ais3MoveFolder {
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
            SlCommand::Ais3RemoveFolder(folder_id) => {
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
            SlCommand::Ais3PurgeFolder(folder_id) => {
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
            SlCommand::Ais3FetchFolderChildren { folder_id, depth } => {
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
            SlCommand::Ais3UpdateItem {
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
            SlCommand::Ais3MoveItem { item_id, parent_id } => {
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
            SlCommand::Ais3RemoveItem(item_id) => {
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
            SlCommand::Ais3FetchItem(item_id) => {
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
            SlCommand::UpdateGroupRoles { group_id, roles } => {
                session.update_group_roles(*group_id, roles, now).ok();
            }
            SlCommand::ChangeGroupRoleMembers { group_id, changes } => {
                session
                    .change_group_role_members(*group_id, changes, now)
                    .ok();
            }
            SlCommand::EjectGroupMembers {
                group_id,
                member_ids,
            } => {
                session.eject_group_members(*group_id, member_ids, now).ok();
            }
            SlCommand::SendGroupNotice {
                group_id,
                subject,
                message,
                attachment,
            } => {
                session
                    .send_group_notice(*group_id, subject, message, *attachment, now)
                    .ok();
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
            SlCommand::FetchMesh {
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
                    let (id, range) = (*mesh_id, *byte_range);
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
            SlCommand::FetchAsset {
                asset_id,
                asset_type,
                byte_range,
            } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ASSET).cloned()
                {
                    let asset_tx = caps.asset_tx.clone();
                    let (id, asset_type, range) = (*asset_id, *asset_type, *byte_range);
                    std::thread::spawn(move || {
                        run_generic_asset_fetch(&url, id, asset_type, range, &asset_tx);
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
            SlCommand::RequestVoiceAccount { request } => {
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
            SlCommand::RequestParcelVoiceInfo => {
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
            SlCommand::SendVoiceSignaling {
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
            SlCommand::RequestExperienceInfo { experience_ids } => {
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
            SlCommand::FindExperiences { query, page } => {
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
            SlCommand::RequestExperiencePermissions => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_EXPERIENCES, &events_tx);
                    });
                }
            }
            SlCommand::SetExperiencePermission {
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
            SlCommand::RequestOwnedExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_AGENT_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_AGENT_EXPERIENCES, &events_tx);
                    });
                }
            }
            SlCommand::RequestAdminExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_ADMIN_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_ADMIN_EXPERIENCES, &events_tx);
                    });
                }
            }
            SlCommand::RequestCreatorExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_GET_CREATOR_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_GET_CREATOR_EXPERIENCES, &events_tx);
                    });
                }
            }
            SlCommand::RequestGroupExperiences { group_id } => {
                if let Some(caps) = caps.as_ref()
                    && let Some(base) = caps.map.get(CAP_GROUP_EXPERIENCES).cloned()
                {
                    let url = format!("{base}{}", group_experiences_query(*group_id));
                    let group_id = *group_id;
                    let asset_tx = caps.asset_tx.clone();
                    std::thread::spawn(move || {
                        run_group_experiences(&url, group_id, &asset_tx);
                    });
                }
            }
            SlCommand::RequestExperienceAdmin { experience_id } => {
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
            SlCommand::RequestExperienceContributor { experience_id } => {
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
            SlCommand::UpdateExperience { update } => {
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
            SlCommand::RequestRegionExperiences => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_REGION_EXPERIENCES).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_REGION_EXPERIENCES, &events_tx);
                    });
                }
            }
            SlCommand::SetRegionExperiences {
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
            SlCommand::OfferTeleport { targets, message } => {
                session.offer_teleport(targets, message, now).ok();
            }
            SlCommand::AcceptTeleportLure { lure_id } => {
                session.accept_teleport_lure(*lure_id, now).ok();
            }
            SlCommand::DeclineTeleportLure {
                from_agent_id,
                lure_id,
            } => {
                session
                    .decline_teleport_lure(*from_agent_id, *lure_id, now)
                    .ok();
            }
            SlCommand::RequestTeleport {
                to_agent_id,
                message,
            } => {
                session.request_teleport(*to_agent_id, message, now).ok();
            }
            SlCommand::GiveInventory {
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
            SlCommand::GiveInventoryFolder {
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
            SlCommand::AcceptInventoryOffer { offer, folder_id } => {
                session.accept_inventory_offer(offer, *folder_id, now).ok();
            }
            SlCommand::DeclineInventoryOffer {
                offer,
                trash_folder_id,
            } => {
                session
                    .decline_inventory_offer(offer, *trash_folder_id, now)
                    .ok();
            }
            SlCommand::StartConference {
                session_id,
                invitees,
                message,
            } => {
                session
                    .start_conference(*session_id, invitees, message, now)
                    .ok();
            }
            SlCommand::SendConferenceMessage {
                session_id,
                message,
            } => {
                session
                    .send_conference_message(*session_id, message, now)
                    .ok();
            }
            SlCommand::LeaveConference { session_id } => {
                session.leave_conference(*session_id, now).ok();
            }
            SlCommand::RetrieveInstantMessages => {
                session.retrieve_instant_messages(now).ok();
            }
            SlCommand::RequestOfflineMessages => {
                if let Some(caps) = caps.as_ref()
                    && let Some(url) = caps.map.get(CAP_READ_OFFLINE_MSGS).cloned()
                {
                    let events_tx = caps.events_tx.clone();
                    std::thread::spawn(move || {
                        run_get_caps_llsd(&url, CAP_READ_OFFLINE_MSGS, &events_tx);
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

/// POSTs a voice-signalling capability (`ProvisionVoiceAccountRequest` or
/// `ParcelVoiceInfoRequest`) carrying the prepared `body` and forwards the LLSD
/// reply to `caps_tx` tagged with `cap`, for the session to surface as the
/// matching event ([`SlSessionEvent::VoiceAccountProvisioned`] /
/// [`SlSessionEvent::ParcelVoiceInfo`]). Only the grid signalling is handled;
/// the audio session is out of scope.
fn run_voice_cap(cap_url: &str, body: String, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
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
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// POSTs a `VoiceSignalingRequest` (WebRTC ICE trickle). Fire-and-forget: the
/// simulator returns only an HTTP status, so there is no event to surface.
fn run_voice_signaling(cap_url: &str, body: String) {
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

/// GETs `url` and parses the LLSD-XML reply, returning `None` on any
/// transport/parse failure. Shared by the experience capability fetches.
fn blocking_get_llsd(url: &str) -> Option<Llsd> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Accept", "application/llsd+xml")
        .send()
        .ok()?;
    let text = response.text().ok()?;
    parse_llsd_xml(&text).ok()
}

/// GETs an experience capability URL and forwards its LLSD reply to `caps_tx`
/// tagged `cap`, for the session to decode in
/// [`Session::handle_caps_event`](sl_proto::Session::handle_caps_event).
fn run_get_caps_llsd(url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    if let Some(llsd) = blocking_get_llsd(url) {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// PUTs `body` to an experience capability URL (the `Allow`/`Block` preference
/// set) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_put_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .put(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// Sends an HTTP PATCH of `body` to an AIS3 inventory capability URL (a folder /
/// item update or move) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_patch_caps_llsd(
    cap_url: &str,
    body: String,
    cap: &'static str,
    caps_tx: &Sender<(String, Llsd)>,
) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .patch(cap_url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// Sends an HTTP DELETE to an experience capability URL (the `Forget`
/// preference) and forwards the LLSD reply to `caps_tx` tagged `cap`.
fn run_delete_caps_llsd(cap_url: &str, cap: &'static str, caps_tx: &Sender<(String, Llsd)>) {
    let Ok(http) = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
    else {
        return;
    };
    let Ok(response) = http
        .delete(cap_url)
        .header("Accept", "application/llsd+xml")
        .send()
    else {
        return;
    };
    if let Ok(text) = response.text()
        && let Ok(llsd) = parse_llsd_xml(&text)
    {
        caps_tx.send((cap.to_owned(), llsd)).ok();
    }
}

/// GETs the `GroupExperiences` capability and forwards an
/// [`SlSessionEvent::GroupExperiences`] over `asset_tx`, echoing the queried
/// `group_id` (the cap reply does not carry it).
fn run_group_experiences(url: &str, group_id: Uuid, asset_tx: &Sender<SessionEvent>) {
    if let Some(llsd) = blocking_get_llsd(url) {
        asset_tx
            .send(SessionEvent::GroupExperiences {
                group_id,
                experience_ids: parse_experience_ids(&llsd),
            })
            .ok();
    }
}

/// GETs an `IsExperienceAdmin` (`admin` true) or `IsExperienceContributor`
/// (`admin` false) capability and forwards the corresponding status event over
/// `asset_tx`, echoing the queried `experience_id`.
fn run_experience_status(
    url: &str,
    experience_id: Uuid,
    admin: bool,
    asset_tx: &Sender<SessionEvent>,
) {
    let Some(llsd) = blocking_get_llsd(url) else {
        return;
    };
    let status = parse_experience_status(&llsd);
    let event = if admin {
        SessionEvent::ExperienceAdminStatus {
            experience_id,
            is_admin: status,
        }
    } else {
        SessionEvent::ExperienceContributorStatus {
            experience_id,
            is_contributor: status,
        }
    };
    asset_tx.send(event).ok();
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
/// or `None` on any network/HTTP failure. When `max_bytes` is `Some`, requests
/// only the first `max_bytes` via a `Range: bytes=0-(max_bytes-1)` header.
fn blocking_get_bytes(url: &str, max_bytes: Option<usize>) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let mut request = http.get(url);
    if let Some(max) = max_bytes {
        request = request.header("Range", format!("bytes=0-{}", max.saturating_sub(1)));
    }
    let response = request.send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// Performs a blocking HTTP `GET` for an inclusive `(start, end)` byte range via
/// a `Range: bytes=start-end` header, returning the body on a 2xx response.
fn blocking_get_range(url: &str, start: u32, end: u32) -> Option<Vec<u8>> {
    let http = ReqwestBlockingClient::builder()
        .timeout(EVENT_QUEUE_TIMEOUT)
        .build()
        .ok()?;
    let response = http
        .get(url)
        .header("Range", format!("bytes={start}-{end}"))
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.bytes().ok().map(|bytes| bytes.to_vec())
}

/// GETs a texture from the `GetTexture` capability and forwards a
/// [`SlSessionEvent::TextureReceived`] or a [`SlSessionEvent::TextureNotFound`]
/// over `asset_tx`. For a non-zero `discard_level` only the level-of-detail
/// prefix is fetched, using HTTP `Range` requests (see [`fetch_texture_lod`]).
fn run_texture_fetch(
    cap_url: &str,
    texture_id: Uuid,
    discard_level: u8,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/?texture_id={texture_id}");
    let event = match fetch_texture_lod(&url, discard_level) {
        Some(data) => SessionEvent::TextureReceived(Box::new(Texture {
            id: texture_id,
            codec: ImageCodec::J2c,
            data,
        })),
        None => SessionEvent::TextureNotFound(texture_id),
    };
    asset_tx.send(event).ok();
}

/// Fetches the codestream bytes for a texture at `discard_level` using HTTP
/// `Range` requests to transfer only the needed LOD prefix: a small probe reads
/// the J2C [`j2c::Header`], from which the prefix length is computed, then a
/// second `Range` request fetches exactly that prefix when the probe did not
/// already cover it. Returns `None` on a 404 / network failure.
fn fetch_texture_lod(url: &str, discard_level: u8) -> Option<Vec<u8>> {
    if discard_level == 0 {
        return blocking_get_bytes(url, None);
    }
    let probe = blocking_get_bytes(url, Some(j2c::FIRST_PACKET_SIZE))?;
    let Some(header) = j2c::parse_header(&probe) else {
        return Some(probe);
    };
    let target = header.discard_data_size(discard_level);
    if probe.len() >= target {
        return Some(probe.get(..target).unwrap_or(&probe).to_vec());
    }
    let body = blocking_get_bytes(url, Some(target))?;
    let size = target.min(body.len());
    Some(body.get(..size).unwrap_or(&body).to_vec())
}

/// GETs an asset from `{cap_url}/{query}` and forwards a
/// [`SlSessionEvent::AssetReceived`] (or a [`SlSessionEvent::AssetTransferFailed`]
/// with the 404-equivalent [`TransferStatus::UnknownSource`]) over `asset_tx`.
/// An inclusive `byte_range` issues an HTTP `Range` request for just that span.
fn run_asset_fetch(
    cap_url: &str,
    query: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    let url = format!("{cap_url}/{query}");
    let bytes = match byte_range {
        Some((start, end)) => blocking_get_range(&url, start, end),
        None => blocking_get_bytes(&url, None),
    };
    let event = match bytes {
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
/// [`SlSessionEvent::AssetTransferFailed`] for a class the cap cannot serve). An
/// inclusive `byte_range` issues an HTTP `Range` request for just that span.
fn run_generic_asset_fetch(
    cap_url: &str,
    asset_id: Uuid,
    asset_type: AssetType,
    byte_range: Option<(u32, u32)>,
    asset_tx: &Sender<SessionEvent>,
) {
    match asset_type.get_asset_query_key() {
        Some(key) => {
            run_asset_fetch(
                cap_url,
                &format!("?{key}={asset_id}"),
                asset_id,
                asset_type,
                byte_range,
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
