//! Groups: membership, roles, notices, and management.

use uuid::Uuid;

/// The agent's active group and title, parsed from `AgentDataUpdate` (pushed on
/// login and whenever the active group changes via
/// [`Session::activate_group`](crate::Session::activate_group)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGroup {
    /// The agent the update is about.
    pub agent_id: Uuid,
    /// The agent's first name.
    pub first_name: String,
    /// The agent's last name.
    pub last_name: String,
    /// The active group's title shown over the avatar (empty if no active group).
    pub group_title: String,
    /// The active group's id (nil if no active group).
    pub active_group_id: Uuid,
    /// The agent's powers bitfield within the active group.
    pub group_powers: u64,
    /// The active group's name (empty if no active group).
    pub group_name: String,
}

/// One of the agent's group memberships, from an `AgentGroupDataUpdate` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembership {
    /// The group id.
    pub group_id: Uuid,
    /// The agent's powers bitfield in the group.
    pub group_powers: u64,
    /// Whether the agent accepts notices from the group.
    pub accept_notices: bool,
    /// The group's insignia (texture id).
    pub group_insignia_id: Uuid,
    /// The agent's L$ contribution to the group.
    pub contribution: i32,
    /// The group name.
    pub group_name: String,
}

/// One member of a group, from a `GroupMembersReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    /// The member's agent id.
    pub agent_id: Uuid,
    /// The member's L$ contribution.
    pub contribution: i32,
    /// The member's online status string (grid-formatted, e.g. `"Online"`).
    pub online_status: String,
    /// The member's powers bitfield.
    pub agent_powers: u64,
    /// The member's group title.
    pub title: String,
    /// Whether the member is a group owner.
    pub is_owner: bool,
}

/// One role within a group, from a `GroupRoleDataReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRole {
    /// The role id (nil for the "Everyone" default role).
    pub role_id: Uuid,
    /// The role name.
    pub name: String,
    /// The role title shown over members holding it.
    pub title: String,
    /// The role description.
    pub description: String,
    /// The powers granted by the role.
    pub powers: u64,
    /// The number of members holding the role.
    pub members: u32,
}

/// One role↔member pairing, from a `GroupRoleMembersReply` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRoleMember {
    /// The role id.
    pub role_id: Uuid,
    /// The member's agent id.
    pub member_id: Uuid,
}

/// One title the agent may wear in a group, from a `GroupTitlesReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTitle {
    /// The title text.
    pub title: String,
    /// The role the title belongs to.
    pub role_id: Uuid,
    /// Whether this is the agent's currently selected title.
    pub selected: bool,
}

/// A group's full profile, parsed from `GroupProfileReply`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four booleans mirror distinct wire flags in GroupProfileReply"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupProfile {
    /// The group id.
    pub group_id: Uuid,
    /// The group name.
    pub name: String,
    /// The group charter text.
    pub charter: String,
    /// Whether the group is shown in search.
    pub show_in_list: bool,
    /// The requesting agent's title in the group.
    pub member_title: String,
    /// The requesting agent's powers bitfield.
    pub powers: u64,
    /// The group insignia (texture id).
    pub insignia_id: Uuid,
    /// The group founder's agent id.
    pub founder_id: Uuid,
    /// The L$ fee to join.
    pub membership_fee: i32,
    /// Whether enrollment is open (no invitation needed).
    pub open_enrollment: bool,
    /// The group's L$ balance (owners only; otherwise 0).
    pub money: i32,
    /// The total member count.
    pub member_count: i32,
    /// The total role count.
    pub role_count: i32,
    /// Whether the group allows publishing on the web.
    pub allow_publish: bool,
    /// Whether the group is flagged mature.
    pub mature_publish: bool,
    /// The owner role id.
    pub owner_role: Uuid,
}

/// The parameters for creating a group via
/// [`Session::create_group`](crate::Session::create_group)
/// (`CreateGroupRequest`).
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four booleans mirror distinct wire flags in CreateGroupRequest"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateGroupParams {
    /// The group name (must be unique on the grid).
    pub name: String,
    /// The group charter text.
    pub charter: String,
    /// Whether the group is shown in search.
    pub show_in_list: bool,
    /// The group insignia (texture id); nil for none.
    pub insignia_id: Uuid,
    /// The L$ fee to join.
    pub membership_fee: i32,
    /// Whether enrollment is open (no invitation needed).
    pub open_enrollment: bool,
    /// Whether the group allows publishing on the web.
    pub allow_publish: bool,
    /// Whether the group is flagged mature.
    pub mature_publish: bool,
}

/// One group notice header, from a `GroupNoticesListReply` entry. Fetch the full
/// body/attachment with
/// [`Session::request_group_notice`](crate::Session::request_group_notice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNotice {
    /// The notice id.
    pub notice_id: Uuid,
    /// The Unix timestamp the notice was posted.
    pub timestamp: u32,
    /// The poster's name.
    pub from_name: String,
    /// The notice subject.
    pub subject: String,
    /// Whether the notice carries an inventory attachment.
    pub has_attachment: bool,
    /// The attachment's asset type (meaningful only if `has_attachment`).
    pub asset_type: u8,
}

/// How a [`GroupRoleEdit`] changes a group role (`GroupRoleUpdate`'s
/// `UpdateType`). The wire bytes match the viewer's `LLRoleChangeType`
/// (`roles_constants.h`) and OpenSim's `OpenMetaverse.GroupRoleUpdate` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleUpdateType {
    /// No change (a no-op `RoleData` block).
    NoUpdate,
    /// Update the role's name/title/description only.
    UpdateData,
    /// Update the role's powers only.
    UpdatePowers,
    /// Update both data and powers.
    UpdateAll,
    /// Create a new role (the simulator may assign a fresh `role_id`).
    Create,
    /// Delete the role.
    Delete,
}

impl GroupRoleUpdateType {
    /// The wire byte for this update type.
    #[must_use]
    pub const fn to_u8(self) -> u8 {
        match self {
            Self::NoUpdate => 0,
            Self::UpdateData => 1,
            Self::UpdatePowers => 2,
            Self::UpdateAll => 3,
            Self::Create => 4,
            Self::Delete => 5,
        }
    }
}

/// One role create/update/delete in a `GroupRoleUpdate`, passed to
/// [`Session::update_group_roles`](crate::Session::update_group_roles). For a
/// [`GroupRoleUpdateType::Create`] the `role_id` is the client-chosen id (the
/// simulator may substitute its own); for update/delete it names the existing
/// role. The `powers` bitfield is built from the [`group_powers`] constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRoleEdit {
    /// The role id (a fresh id for `Create`, the existing role for the rest).
    pub role_id: Uuid,
    /// The role name.
    pub name: String,
    /// The role description.
    pub description: String,
    /// The title members holding the role wear.
    pub title: String,
    /// The powers granted by the role (see [`group_powers`]).
    pub powers: u64,
    /// Which fields of the role this edit changes.
    pub update_type: GroupRoleUpdateType,
}

/// Whether a [`GroupRoleMemberChange`] adds a member to a role or removes them
/// (`GroupRoleChanges`'s `Change`). Add = 0, Remove = 1, matching OpenSim's
/// `GroupRoleChanges` handler and the viewer's `LLRoleMemberChangeType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRoleChange {
    /// Assign the member to the role.
    Add,
    /// Remove the member from the role.
    Remove,
}

impl GroupRoleChange {
    /// The wire `Change` value for this role-member change.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Add => 0,
            Self::Remove => 1,
        }
    }
}

/// One member↔role assignment change in a `GroupRoleChanges`, passed to
/// [`Session::change_group_role_members`](crate::Session::change_group_role_members).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupRoleMemberChange {
    /// The role to add the member to or remove them from.
    pub role_id: Uuid,
    /// The member's agent id.
    pub member_id: Uuid,
    /// Whether to add or remove the member.
    pub change: GroupRoleChange,
}

/// An inventory item attached to a group notice, passed to
/// [`Session::send_group_notice`](crate::Session::send_group_notice). The item
/// must be copy+transfer for the grid to accept it. The notice's recipients
/// receive an `IM_GROUP_NOTICE` they can accept to copy the item into their
/// inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupNoticeAttachment {
    /// The attached inventory item's id.
    pub item_id: Uuid,
    /// The item owner's agent id (usually the sender).
    pub owner_id: Uuid,
}

/// Group role power bits (`GP_*` in the viewer's `roles_constants.h`), combined
/// into the `powers` bitfield of a [`GroupRoleEdit`]. Only the commonly-set
/// powers are named; any bit may be set directly. Bit 0 is unused (the "none"
/// marker), so the enrollment power is bit 1, etc.
pub mod group_powers {
    /// No powers.
    pub const NONE: u64 = 0x0;
    /// All powers (the owner role).
    pub const ALL: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    /// Invite members to the group.
    pub const MEMBER_INVITE: u64 = 1 << 1;
    /// Eject members from the group.
    pub const MEMBER_EJECT: u64 = 1 << 2;
    /// Toggle "Open Enrollment" and the join fee.
    pub const MEMBER_OPTIONS: u64 = 1 << 3;
    /// Create new roles.
    pub const ROLE_CREATE: u64 = 1 << 4;
    /// Delete roles.
    pub const ROLE_DELETE: u64 = 1 << 5;
    /// Change a role's properties (name, title, description, powers).
    pub const ROLE_PROPERTIES: u64 = 1 << 6;
    /// Assign a member to a role the assigner does not hold "owner" over.
    pub const ROLE_ASSIGN_MEMBER_LIMITED: u64 = 1 << 7;
    /// Assign a member to any role.
    pub const ROLE_ASSIGN_MEMBER: u64 = 1 << 8;
    /// Remove a member from a role.
    pub const ROLE_REMOVE_MEMBER: u64 = 1 << 9;
    /// Change role/actions and members of roles.
    pub const ROLE_CHANGE_ACTIONS: u64 = 1 << 10;
    /// Change the group's identity (charter, insignia, listing, maturity).
    pub const GROUP_CHANGE_IDENTITY: u64 = 1 << 11;
    /// Deed land and buy land for the group.
    pub const LAND_DEED: u64 = 1 << 12;
    /// Send group notices.
    pub const NOTICES_SEND: u64 = 1 << 42;
    /// Receive group notices and view notice history.
    pub const NOTICES_RECEIVE: u64 = 1 << 43;
}
