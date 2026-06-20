//! High-level commands an I/O driver sends to a running [`Session`](crate::Session).
//!
//! Both the tokio and Bevy drivers consume this single command vocabulary; it
//! lives here so the two stay in lock-step rather than maintaining parallel
//! copies.

use crate::{
    AnyMessage, AssetType, AttachmentPoint, Camera, ChatType, ClassifiedUpdate, ClickAction,
    ControlFlags, CreateGroupParams, DeRezDestination, EstateAccessDelta, ExperiencePermission,
    ExperienceUpdate, FriendRights, GroupNoticeAttachment, GroupRoleEdit, GroupRoleMemberChange,
    IceCandidate, InterestsUpdate, InventoryItem, InventoryOffer, InventoryType, LindenAmount,
    MapItemType, Material, MaterialOverrideUpdate, MediaEntry, MoneyTransactionType, MuteFlags,
    MuteType, NewInventoryItem, ObjectFlagSettings, ObjectTransform, ParcelAccessEntry,
    ParcelAccessScope, ParcelReturnType, ParcelUpdate, PermissionField, PickUpdate, PrimShape,
    ProfileUpdate, RegionInfoUpdate, Reliability, RezAttachment, Rotation, SaleType,
    ScriptPermissions, Throttle, Uuid, Vector, ViewerEffect, VoiceProvisionRequest, Wearable,
};

/// A command sent to a running [`Session`](crate::Session) via an I/O driver.
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
    /// [`Event::ChatReceived`](crate::Event::ChatReceived).
    Chat {
        /// The message text.
        message: String,
        /// The chat type (whisper / normal / shout / …).
        chat_type: ChatType,
        /// The chat channel (`0` for ordinary local chat).
        channel: i32,
    },
    /// Broadcast a local-chat typing indicator (`true` = start, `false` = stop).
    /// Other clients see it as an [`Event::ChatTyping`](crate::Event::ChatTyping).
    Typing(bool),
    /// Send a direct (1:1) instant message. Incoming IMs arrive as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived).
    InstantMessage {
        /// The recipient's agent id.
        to_agent_id: Uuid,
        /// The message text.
        message: String,
    },
    /// Send an instant-message typing indicator to `to_agent_id` (`true` = start,
    /// `false` = stop). Other clients see it as an [`Event::ImTyping`](crate::Event::ImTyping).
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
    /// result arrives as an [`Event::SitResult`](crate::Event::SitResult).
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
    /// Request an avatar's profile. Replies arrive as [`Event::AvatarProperties`](crate::Event::AvatarProperties),
    /// [`Event::AvatarInterests`](crate::Event::AvatarInterests), and [`Event::AvatarGroups`](crate::Event::AvatarGroups).
    RequestAvatarProperties(Uuid),
    /// Request an avatar's picks. The reply arrives as [`Event::AvatarPicks`](crate::Event::AvatarPicks).
    RequestAvatarPicks(Uuid),
    /// Request the agent's private notes about an avatar. The reply arrives as
    /// [`Event::AvatarNotes`](crate::Event::AvatarNotes).
    RequestAvatarNotes(Uuid),
    /// Request an avatar's classified ads. The reply arrives as
    /// [`Event::AvatarClassifieds`](crate::Event::AvatarClassifieds).
    RequestAvatarClassifieds(Uuid),
    /// Request the full details of one pick. `creator_id` is the pick's owner
    /// (the `target_id` from [`Event::AvatarPicks`](crate::Event::AvatarPicks)). The reply arrives as
    /// [`Event::PickInfo`](crate::Event::PickInfo).
    RequestPickInfo {
        /// The avatar that owns the pick.
        creator_id: Uuid,
        /// The pick id.
        pick_id: Uuid,
    },
    /// Request the full details of one classified ad. The reply arrives as
    /// [`Event::ClassifiedInfo`](crate::Event::ClassifiedInfo).
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
    /// [`Event::InventoryDescendents`](crate::Event::InventoryDescendents). The full folder skeleton arrives once at
    /// login as [`Event::InventorySkeleton`](crate::Event::InventorySkeleton).
    RequestFolderContents(Uuid),
    /// Fetch the contents of one or more inventory folders over the **HTTP CAPS**
    /// path (`FetchInventoryDescendents2`) — the modern path used on Second Life.
    /// Each folder's contents arrive as an [`Event::InventoryDescendents`](crate::Event::InventoryDescendents).
    FetchInventoryFolders(Vec<Uuid>),
    /// Create an inventory folder (`CreateInventoryFolder`, UDP). `folder_id` is a
    /// fresh, caller-chosen id. The simulator sends no reply; the folder is added
    /// to the local cache optimistically. Use [`Command::CreateInventoryCategory`]
    /// for a confirmed reply.
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
    /// the id and replies with an [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated).
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
    /// with an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate) for the new item.
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
    /// both OpenSim and Second Life). Unlike the UDP `CreateInventoryFolder`, the
    /// grid replies synchronously, surfaced as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate)
    /// with the created folder. The runtime allocates the new folder id.
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
    /// [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
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
    /// Second-Life only; the result arrives as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
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
    /// item arrives as an [`Event::InventoryBulkUpdate`](crate::Event::InventoryBulkUpdate).
    Ais3FetchItem(Uuid),
    /// Set the friendship rights granted to a friend (`GrantUserRights`). The
    /// `rights` bitfield combines the [`FriendRights`] `CAN_*` flags. The change
    /// is echoed back as an [`Event::FriendRightsChanged`](crate::Event::FriendRightsChanged).
    GrantUserRights {
        /// The friend whose granted rights to set.
        target: Uuid,
        /// The new rights bitfield (combine `FriendRights::CAN_*`).
        rights: FriendRights,
    },
    /// Offer friendship to an agent (`ImprovedInstantMessage`,
    /// `IM_FRIENDSHIP_OFFERED`). The offer arrives at the recipient as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with [`ImDialog::FriendshipOffered`](crate::ImDialog::FriendshipOffered).
    OfferFriendship {
        /// The agent to offer friendship to.
        to_agent_id: Uuid,
        /// The offer message text.
        message: String,
    },
    /// End the friendship with an agent (`TerminateFriendship`).
    TerminateFriendship(Uuid),
    /// Accept a friendship offer (`AcceptFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming friendship-offer IM; the
    /// calling card goes into `calling_card_folder`.
    AcceptFriendship {
        /// The offer's transaction id (the friendship-offer IM's `id`).
        transaction_id: Uuid,
        /// The inventory folder to place the new calling card in.
        calling_card_folder: Uuid,
    },
    /// Decline a friendship offer (`DeclineFriendship`). The `transaction_id` is
    /// the [`InstantMessage::id`](crate::InstantMessage::id) of the incoming friendship-offer IM.
    DeclineFriendship(Uuid),
    /// Make a group the active group (`ActivateGroup`); nil clears it. Confirmed
    /// by [`Event::ActiveGroupChanged`](crate::Event::ActiveGroupChanged).
    ActivateGroup(Uuid),
    /// Request a group's member roster over **UDP** (`GroupMembersRequest`).
    /// Replies arrive as [`Event::GroupMembers`](crate::Event::GroupMembers).
    RequestGroupMembers(Uuid),
    /// Fetch a group's member roster over the **HTTP CAPS** path
    /// (`GroupMemberData`) — the modern path used on Second Life. The roster
    /// arrives as an [`Event::GroupMembers`](crate::Event::GroupMembers).
    FetchGroupMembers(Uuid),
    /// Request a group's roles. The reply arrives as [`Event::GroupRoleData`](crate::Event::GroupRoleData).
    RequestGroupRoles(Uuid),
    /// Request a group's role↔member pairings. The reply arrives as
    /// [`Event::GroupRoleMembers`](crate::Event::GroupRoleMembers).
    RequestGroupRoleMembers(Uuid),
    /// Request the agent's selectable titles in a group. The reply arrives as
    /// [`Event::GroupTitles`](crate::Event::GroupTitles).
    RequestGroupTitles(Uuid),
    /// Request a group's profile. The reply arrives as
    /// [`Event::GroupProfileReceived`](crate::Event::GroupProfileReceived).
    RequestGroupProfile(Uuid),
    /// Request a group's notice list. The reply arrives as [`Event::GroupNotices`](crate::Event::GroupNotices).
    RequestGroupNotices(Uuid),
    /// Request a single group notice's full body (by notice id). Delivered as an
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with the group-notice dialog.
    RequestGroupNotice(Uuid),
    /// Create a new group. The result arrives as [`Event::CreateGroupResult`](crate::Event::CreateGroupResult).
    CreateGroup(CreateGroupParams),
    /// Join an open-enrollment group. The result arrives as
    /// [`Event::JoinGroupResult`](crate::Event::JoinGroupResult).
    JoinGroup(Uuid),
    /// Leave a group. The result arrives as [`Event::LeaveGroupResult`](crate::Event::LeaveGroupResult).
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
    /// messages then arrive as [`Event::GroupSessionMessage`](crate::Event::GroupSessionMessage).
    StartGroupSession(Uuid),
    /// Send a message into a group's IM session. Other members receive it as
    /// [`Event::GroupSessionMessage`](crate::Event::GroupSessionMessage).
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
    /// as [`Event::EjectGroupMemberResult`](crate::Event::EjectGroupMemberResult).
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
    /// [`Event::ScriptDialog`](crate::Event::ScriptDialog) — the chosen button on its hidden `chat_channel`.
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
    /// [`Event::ScriptPermissionRequest`](crate::Event::ScriptPermissionRequest) — grants `permissions` (a subset of
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
    /// arrives as [`Event::MuteList`](crate::Event::MuteList) (or [`Event::MuteListUnchanged`](crate::Event::MuteListUnchanged)).
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
    /// Resolve agent ids to their legacy names (`UUIDNameRequest`); replies
    /// arrive as [`Event::AvatarNames`](crate::Event::AvatarNames). The session
    /// does not resolve or cache names on its own — a caller asks for the ids it
    /// needs (e.g. an estate's manager list) and decides what to do with the
    /// answers. Large lists are split across several requests automatically.
    RequestAvatarNames(Vec<Uuid>),
    /// Resolve group ids to their names (`UUIDGroupNameRequest`); replies arrive
    /// as [`Event::GroupNames`](crate::Event::GroupNames). See
    /// [`RequestAvatarNames`](Self::RequestAvatarNames).
    RequestGroupNames(Vec<Uuid>),
    /// Request the extended-environment (EEP) settings via the `ExtEnvironment`
    /// capability; the reply arrives as
    /// [`Event::Environment`](crate::Event::Environment). `parcel_id` selects a
    /// parcel's environment, or [`None`] for the whole region.
    RequestEnvironment {
        /// The parcel's region-local id, or [`None`] for the region environment.
        parcel_id: Option<i32>,
    },
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
    /// reply arrives as [`Event::ParcelAccessList`](crate::Event::ParcelAccessList).
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
    /// arrives as [`Event::ParcelDwell`](crate::Event::ParcelDwell).
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
    /// arrive as [`Event::EstateInfo`](crate::Event::EstateInfo) and [`Event::EstateAccessList`](crate::Event::EstateAccessList).
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
    /// as [`Event::MoneyBalance`](crate::Event::MoneyBalance).
    RequestMoneyBalance,
    /// Request the grid's economy data (`EconomyDataRequest`); the reply arrives
    /// as [`Event::EconomyData`](crate::Event::EconomyData).
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
    /// indices); each region arrives as an [`Event::MapBlock`](crate::Event::MapBlock).
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
    /// arrive as [`Event::MapBlock`](crate::Event::MapBlock).
    RequestMapByName {
        /// The region name (or prefix) to search for.
        name: String,
    },
    /// Request world-map overlay items of a given type (`MapItemRequest`); the
    /// reply arrives as [`Event::MapItems`](crate::Event::MapItems).
    RequestMapItems {
        /// The kind of item to request (avatars, telehubs, land for sale, …).
        item_type: MapItemType,
        /// The target region handle (0 = the current region).
        region_handle: u64,
    },
    /// Request the full `ObjectUpdate` for the given region-local ids
    /// (`RequestMultipleObjects`); updates arrive as [`Event::ObjectAdded`](crate::Event::ObjectAdded) /
    /// [`Event::ObjectUpdated`](crate::Event::ObjectUpdated).
    RequestObjects {
        /// The region-local ids to (re)fetch.
        local_ids: Vec<u32>,
    },
    /// Request objects' extended properties by selecting them (`ObjectSelect`);
    /// the reply arrives as [`Event::ObjectProperties`](crate::Event::ObjectProperties).
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
    /// reassembled image arrives as [`Event::TextureReceived`](crate::Event::TextureReceived) (or
    /// [`Event::TextureNotFound`](crate::Event::TextureNotFound)).
    RequestTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: i8,
        /// The download priority (larger is fetched sooner).
        priority: f32,
    },
    /// Request a generic asset over the UDP transfer path (`TransferRequest`);
    /// the reassembled asset arrives as [`Event::AssetReceived`](crate::Event::AssetReceived) (or
    /// [`Event::AssetTransferFailed`](crate::Event::AssetTransferFailed)).
    RequestAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class.
        asset_type: AssetType,
        /// The transfer priority.
        priority: f32,
    },
    /// Fetch a texture over the HTTP `GetTexture` capability; the image arrives
    /// as [`Event::TextureReceived`](crate::Event::TextureReceived) (or [`Event::TextureNotFound`](crate::Event::TextureNotFound)). When
    /// `discard_level` is non-zero the codestream is truncated to that
    /// level-of-detail prefix via [`j2c`](crate::j2c).
    FetchTexture {
        /// The texture's asset id.
        texture_id: Uuid,
        /// The level of detail (0 = full resolution; higher = coarser).
        discard_level: u8,
    },
    /// Fetch a mesh asset over the HTTP `GetMesh2`/`GetMesh` capability; the data
    /// arrives as [`Event::AssetReceived`](crate::Event::AssetReceived). An optional `byte_range` (inclusive
    /// `(start, end)` byte offsets) issues an HTTP `Range` request so only that
    /// span is transferred — e.g. a single mesh LOD whose offsets the caller read
    /// from the mesh header. `None` fetches the whole asset.
    FetchMesh {
        /// The mesh asset's id.
        mesh_id: Uuid,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
    },
    /// Fetch a generic asset over the HTTP `GetAsset` capability; the data
    /// arrives as [`Event::AssetReceived`](crate::Event::AssetReceived) (or [`Event::AssetTransferFailed`](crate::Event::AssetTransferFailed)).
    /// An optional `byte_range` (inclusive `(start, end)` byte offsets) issues an
    /// HTTP `Range` request so only that span is transferred; `None` fetches the
    /// whole asset.
    FetchAsset {
        /// The asset's id.
        asset_id: Uuid,
        /// The asset's class (selects the cap query parameter).
        asset_type: AssetType,
        /// Optional inclusive `(start, end)` byte range to fetch.
        byte_range: Option<(u32, u32)>,
    },
    /// Ask the simulator to (re-)send the agent's own wearables
    /// (`AgentWearablesRequest`); the reply arrives as [`Event::AgentWearables`](crate::Event::AgentWearables).
    RequestWearables,
    /// Set the agent's outfit (`AgentIsNowWearing`): the complete set of
    /// wearables to wear. The simulator acknowledges with a fresh
    /// [`Event::AgentWearables`](crate::Event::AgentWearables).
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
    /// reply arrives as [`Event::CachedTextureResponse`](crate::Event::CachedTextureResponse).
    RequestCachedTextures {
        /// The serial echoed back in the reply.
        serial: i32,
        /// The queried slots, as `(cache id, texture slot index)` pairs.
        slots: Vec<(Uuid, u8)>,
    },
    /// Trigger a modern server-side appearance bake over the HTTP
    /// `UpdateAvatarAppearance` capability (Second Life "central baking"): the
    /// grid composites the agent's Current Outfit Folder and broadcasts the
    /// result as [`Event::AvatarAppearance`](crate::Event::AvatarAppearance). The POST's own reply arrives as
    /// [`Event::ServerAppearanceUpdate`](crate::Event::ServerAppearanceUpdate).
    RequestServerAppearanceUpdate {
        /// The Current Outfit Folder version the grid should bake.
        cof_version: i32,
    },
    /// Start and/or stop several of the agent's own animations (`AgentAnimation`):
    /// each `(anim_id, start)` pair starts (`true`) or stops (`false`) one
    /// animation. Other avatars observe the result as an
    /// [`Event::AvatarAnimation`](crate::Event::AvatarAnimation).
    SetAnimations(Vec<(Uuid, bool)>),
    /// Start one of the agent's own animations (`AgentAnimation`); convenience
    /// for a single-element [`Command::SetAnimations`].
    PlayAnimation(Uuid),
    /// Stop one of the agent's own animations (`AgentAnimation`); convenience for
    /// a single-element [`Command::SetAnimations`].
    StopAnimation(Uuid),
    /// Attach an in-world object (selected by its region-local id) to the avatar
    /// (`ObjectAttach`). The object is worn at `attachment_point`; when `add` is
    /// `true` it is added alongside anything already on that point rather than
    /// replacing it ([`AttachmentPoint::Default`] lets the simulator pick the
    /// object's saved/scripted slot). To wear an item straight from inventory
    /// instead, use [`Command::RezAttachment`].
    AttachObject {
        /// The in-world object's region-local id.
        local_id: u32,
        /// The point to attach the object to.
        attachment_point: AttachmentPoint,
        /// Add the attachment (`true`) rather than replace what is on the point.
        add: bool,
        /// The rotation to wear the object at, relative to the attachment point.
        rotation: Rotation,
    },
    /// Detach attachments back to inventory by their region-local ids
    /// (`ObjectDetach`), marking each item as no longer "(worn)".
    DetachObjects {
        /// The attachments' region-local ids.
        local_ids: Vec<u32>,
    },
    /// Drop attachments from the avatar onto the ground by their region-local ids
    /// (`ObjectDrop`): they become ordinary in-world objects at the avatar's
    /// location rather than returning to inventory.
    DropAttachments {
        /// The attachments' region-local ids.
        local_ids: Vec<u32>,
    },
    /// Remove (take off) an attachment by its inventory item id
    /// (`RemoveAttachment`). Unlike [`Command::DetachObjects`] this names the
    /// inventory item rather than the rezzed object's region-local id.
    RemoveAttachment {
        /// The attachment point the item is worn on (the simulator resolves the
        /// item by id; [`AttachmentPoint::Default`] is accepted).
        attachment_point: AttachmentPoint,
        /// The worn item's inventory item id.
        item_id: Uuid,
    },
    /// Wear an inventory item as an attachment (`RezSingleAttachmentFromInv`):
    /// rez it directly onto the avatar from inventory. To attach an object that
    /// is already rezzed in-world, use [`Command::AttachObject`].
    RezAttachment(RezAttachment),
    /// Wear several inventory items as attachments in one compound message
    /// (`RezMultipleAttachmentsFromInv`).
    RezAttachments {
        /// A fresh, caller-chosen id correlating the compound message's parts
        /// (the viewer generates a new UUID per request).
        compound_id: Uuid,
        /// Detach everything currently worn before wearing these (`true`), e.g.
        /// when replacing the whole outfit.
        first_detach_all: bool,
        /// The items to wear.
        attachments: Vec<RezAttachment>,
    },
    /// Send one or more viewer effects (`ViewerEffect`): the look-at / point-at
    /// gaze hints, the editing/touch beam, and the other transient HUD effects
    /// other viewers render. The effects are batched into a single message;
    /// each carries its own id, source agent, type, duration, colour and
    /// effect-specific payload.
    ViewerEffect(Vec<ViewerEffect>),
    /// Track an agent's position on the world map (`TrackAgent`): the simulator
    /// streams the tracked ("prey") agent's coarse location back via
    /// `CoarseLocationUpdate` (surfaced as
    /// [`Event::CoarseLocationUpdate`](crate::Event::CoarseLocationUpdate), whose
    /// `prey` index then points at the tracked avatar).
    TrackAgent {
        /// The agent to track.
        prey_id: Uuid,
    },
    /// Ask the simulator for an agent's global position (`FindAgent`): an
    /// estate/god lookup. The simulator answers with a `FindAgent` carrying the
    /// found positions, surfaced as
    /// [`Event::FindAgentReply`](crate::Event::FindAgentReply).
    FindAgent {
        /// The requesting agent (the "hunter"); usually the agent's own id.
        hunter: Uuid,
        /// The agent to locate (the "prey").
        prey: Uuid,
    },
    /// Upload a new asset over the legacy UDP path (`AssetUploadRequest`): stores
    /// the asset bytes (small assets inline, larger ones over `Xfer`) without
    /// creating an inventory item. Completion arrives as
    /// [`Event::AssetUploadComplete`](crate::Event::AssetUploadComplete). For an upload that also creates an
    /// inventory item, use [`Command::UploadAsset`].
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
    /// `NewFileAgentInventory` capability (the two-step CAPS uploader): POST the
    /// metadata, then the raw bytes. The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) (or [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed)).
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
        /// The L$ price the client expects to be charged (the grid rejects a
        /// mismatch; 0 on free grids such as OpenSim).
        expected_upload_cost: i32,
        /// The raw asset bytes.
        data: Vec<u8>,
    },
    /// Upload a client-computed baked avatar texture over the
    /// `UploadBakedTexture` capability (the legacy appearance path): stores a
    /// *temporary* asset with no inventory item. The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) (with `new_inventory_item` = `None`) or
    /// [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed).
    UploadBakedTexture {
        /// The raw baked-texture bytes (a JPEG-2000 codestream).
        data: Vec<u8>,
    },
    /// Replace the asset of an existing inventory item over the matching
    /// `Update*AgentInventory` capability (gesture / notecard / script /
    /// settings, selected by `asset_type`). The result arrives as
    /// [`Event::AssetUploaded`](crate::Event::AssetUploaded) or [`Event::AssetUploadFailed`](crate::Event::AssetUploadFailed).
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
    /// [`Event::ObjectMedia`](crate::Event::ObjectMedia).
    RequestObjectMedia {
        /// The object whose media to fetch.
        object_id: Uuid,
    },
    /// Set an object's per-face media over the `ObjectMedia` capability (an
    /// UPDATE). `faces` is one entry per prim face in order; a face with no media
    /// is `None`. The simulator advances the object's media version (visible on a
    /// subsequent [`Command::RequestObjectMedia`]) rather than replying.
    SetObjectMedia {
        /// The object whose media to set.
        object_id: Uuid,
        /// Per-face media, one slot per prim face in order (`None` = no media).
        faces: Vec<Option<MediaEntry>>,
    },
    /// Navigate the media on a single prim face to a new URL over the
    /// `ObjectMediaNavigate` capability. The simulator advances the object's
    /// media version (visible on a subsequent [`Command::RequestObjectMedia`]).
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
    /// arrives as [`Event::RenderMaterials`](crate::Event::RenderMaterials). The ids are the per-face
    /// `TextureEntry` material ids carried by scene objects.
    RequestRenderMaterials {
        /// The material ids to fetch.
        material_ids: Vec<Uuid>,
    },
    /// Set GLTF (PBR) materials on object faces over the `ModifyMaterialParams`
    /// capability. Each update applies an opaque `gltf_json` override and/or a
    /// stored material `asset_id` to one face (`side`, or `-1` for all). The
    /// `{ success, message }` reply arrives as [`Event::MaterialParamsResult`](crate::Event::MaterialParamsResult).
    ModifyMaterialParams {
        /// The per-face material assignments to apply.
        updates: Vec<MaterialOverrideUpdate>,
    },
    /// Request voice-chat account credentials over the
    /// `ProvisionVoiceAccountRequest` capability. A [`VoiceProvisionRequest::vivox`]
    /// asks for legacy Vivox SIP credentials; a [`VoiceProvisionRequest::webrtc`]
    /// negotiates a WebRTC session (the JSEP offer SDP is supplied by the
    /// caller's own — out-of-scope — WebRTC engine). The reply arrives as
    /// [`Event::VoiceAccountProvisioned`](crate::Event::VoiceAccountProvisioned). This handles the grid *signalling*
    /// only; the audio session itself is the caller's concern.
    RequestVoiceAccount {
        /// The provision request (backend selection + WebRTC offer/logout).
        request: VoiceProvisionRequest,
    },
    /// Request the current parcel's voice channel over the
    /// `ParcelVoiceInfoRequest` capability. The reply arrives as
    /// [`Event::ParcelVoiceInfo`](crate::Event::ParcelVoiceInfo).
    RequestParcelVoiceInfo,
    /// Trickle WebRTC ICE candidates (or signal end-of-gathering) over the
    /// `VoiceSignalingRequest` capability, keyed by the `viewer_session` from a
    /// prior [`Event::VoiceAccountProvisioned`](crate::Event::VoiceAccountProvisioned). Fire-and-forget — the simulator
    /// returns only an HTTP status. The candidates come from the caller's
    /// out-of-scope WebRTC engine.
    SendVoiceSignaling {
        /// The viewer session id from the provision reply.
        viewer_session: String,
        /// The ICE candidates to trickle (empty with `completed` to end).
        candidates: Vec<IceCandidate>,
        /// Whether this marks the end of ICE gathering.
        completed: bool,
    },
    /// Fetch experience metadata over the `GetExperienceInfo` capability, batching
    /// every id into one request. The reply arrives as [`Event::ExperienceInfo`](crate::Event::ExperienceInfo).
    RequestExperienceInfo {
        /// The experiences whose metadata to fetch.
        experience_ids: Vec<Uuid>,
    },
    /// Search experiences by name over the `FindExperienceByName` capability. The
    /// reply (one page) arrives as [`Event::ExperienceSearchResults`](crate::Event::ExperienceSearchResults).
    FindExperiences {
        /// The search text.
        query: String,
        /// The zero-based result page.
        page: i32,
    },
    /// Fetch the agent's per-experience preferences over the `GetExperiences`
    /// capability. The reply arrives as [`Event::ExperiencePermissions`](crate::Event::ExperiencePermissions).
    RequestExperiencePermissions,
    /// Set (or forget) the agent's preference for one experience over the
    /// `ExperiencePreferences` capability (`Allow`/`Block` via PUT, `Forget` via
    /// DELETE). The updated lists arrive as [`Event::ExperiencePermissions`](crate::Event::ExperiencePermissions).
    SetExperiencePermission {
        /// The experience to set the preference for.
        experience_id: Uuid,
        /// The preference to apply.
        permission: ExperiencePermission,
    },
    /// Fetch the experiences the agent owns over the `AgentExperiences`
    /// capability. The reply arrives as [`Event::OwnedExperiences`](crate::Event::OwnedExperiences).
    RequestOwnedExperiences,
    /// Fetch the experiences the agent administers over the `GetAdminExperiences`
    /// capability. The reply arrives as [`Event::AdminExperiences`](crate::Event::AdminExperiences).
    RequestAdminExperiences,
    /// Fetch the experiences the agent created over the `GetCreatorExperiences`
    /// capability. The reply arrives as [`Event::CreatorExperiences`](crate::Event::CreatorExperiences).
    RequestCreatorExperiences,
    /// Fetch the experiences a group owns over the `GroupExperiences` capability.
    /// The reply arrives as [`Event::GroupExperiences`](crate::Event::GroupExperiences).
    RequestGroupExperiences {
        /// The group to query.
        group_id: Uuid,
    },
    /// Test whether the agent administers an experience over the
    /// `IsExperienceAdmin` capability. The reply arrives as
    /// [`Event::ExperienceAdminStatus`](crate::Event::ExperienceAdminStatus).
    RequestExperienceAdmin {
        /// The experience to test.
        experience_id: Uuid,
    },
    /// Test whether the agent contributes to an experience over the
    /// `IsExperienceContributor` capability. The reply arrives as
    /// [`Event::ExperienceContributorStatus`](crate::Event::ExperienceContributorStatus).
    RequestExperienceContributor {
        /// The experience to test.
        experience_id: Uuid,
    },
    /// Edit an experience's metadata over the `UpdateExperience` capability. The
    /// updated experience arrives as [`Event::ExperienceUpdated`](crate::Event::ExperienceUpdated).
    UpdateExperience {
        /// The editable experience metadata to write.
        update: ExperienceUpdate,
    },
    /// Read the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability. The reply arrives as
    /// [`Event::RegionExperiences`](crate::Event::RegionExperiences).
    RequestRegionExperiences,
    /// Replace the region's experience allow/block/trust lists over the
    /// `RegionExperiences` capability (estate-gated). The updated lists arrive as
    /// [`Event::RegionExperiences`](crate::Event::RegionExperiences).
    SetRegionExperiences {
        /// The experiences the region allows.
        allowed: Vec<Uuid>,
        /// The experiences the region blocks.
        blocked: Vec<Uuid>,
        /// The experiences the region trusts.
        trusted: Vec<Uuid>,
    },
    /// Offer a teleport ("lure") to each `targets` agent (`StartLure`, #28). Each
    /// recipient receives an [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived) with
    /// [`ImDialog::LureUser`](crate::ImDialog::LureUser).
    OfferTeleport {
        /// The agents to invite.
        targets: Vec<Uuid>,
        /// The accompanying message.
        message: String,
    },
    /// Accept a teleport lure (`TeleportLureRequest`), teleporting to the offer's
    /// location. `lure_id` is the [`InstantMessage::id`](crate::InstantMessage::id) of the received
    /// [`ImDialog::LureUser`](crate::ImDialog::LureUser) IM.
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
    /// Request a teleport from `to_agent_id` (`IM_TELEPORT_REQUEST`): ask them to
    /// offer this agent a teleport.
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
    /// `folder_id`. `offer` is decoded from the incoming
    /// [`InstantMessage::inventory_offer`](crate::InstantMessage::inventory_offer).
    AcceptInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The destination folder to file the item into.
        folder_id: Uuid,
    },
    /// Decline an inventory offer (`IM_INVENTORY_DECLINED`); the item is routed to
    /// `trash_folder_id`.
    DeclineInventoryOffer {
        /// The decoded inventory offer.
        offer: InventoryOffer,
        /// The trash folder the simulator routes the declined item to.
        trash_folder_id: Uuid,
    },
    /// Start (or add invitees to) an ad-hoc conference IM session
    /// (`IM_SESSION_CONFERENCE_START`). Messages arrive as
    /// [`Event::ConferenceSessionMessage`](crate::Event::ConferenceSessionMessage).
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
    /// (`RetrieveInstantMessages`); they arrive as offline
    /// [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived)s.
    RetrieveInstantMessages,
    /// Read stored offline instant messages over the modern `ReadOfflineMsgs`
    /// capability; they arrive as offline [`Event::InstantMessageReceived`](crate::Event::InstantMessageReceived)s.
    RequestOfflineMessages,
    /// Begin a clean logout.
    Logout,
}
