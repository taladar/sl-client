//! Groups: membership, roles, notices, and management.

use sl_types::key::{AgentKey, GroupKey, GroupRoleKey, InventoryKey, TextureKey};
use sl_types::money::LindenAmount;

use crate::types::{LandArea, LindenBalance};
use uuid::Uuid;

/// A group **notice** id (the viewer's group-notice `mNoticeID`).
///
/// A notice is one posting in a group's notice list; this id fetches its full
/// body/attachment
/// ([`Session::request_group_notice`](crate::Session::request_group_notice)).
/// Kept client-local in `sl-proto` (per the standing "new types go local first,
/// batch-migrate to `sl-types` later" rule); mirrors the `sl-types` key
/// ergonomics (`From<Uuid>`/[`uuid`](Self::uuid)/`Display`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupNoticeKey(pub Uuid);

impl From<Uuid> for GroupNoticeKey {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl GroupNoticeKey {
    /// Returns the wrapped raw `Uuid`.
    #[must_use]
    pub const fn uuid(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for GroupNoticeKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A group **proposal** id (the viewer's `mVoteID` / a ballot's `proposal_id`).
///
/// Identifies one group proposal both while it is open
/// ([`GroupActiveProposalItem`]) and in the vote history
/// ([`GroupVoteHistoryItem`]); it is the id a ballot
/// ([`Command::GroupProposalBallot`](crate::Command::GroupProposalBallot)) votes
/// against. Distinct from [`ProposalCandidateId`] (an option *within* a
/// proposal), so the two can't be transposed. Kept client-local in `sl-proto`;
/// mirrors the `sl-types` key ergonomics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProposalVoteId(pub Uuid);

impl From<Uuid> for ProposalVoteId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl ProposalVoteId {
    /// Returns the wrapped raw `Uuid`.
    #[must_use]
    pub const fn uuid(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for ProposalVoteId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A proposal **candidate/option** id (the viewer's per-`VoteItem` `mCandidateID`
/// in a finished proposal's tally — or the voter for a yes/no proposal).
///
/// Identifies one option within a proposal's vote history ([`GroupVote`]).
/// Distinct from [`ProposalVoteId`] (the proposal itself). Kept client-local in
/// `sl-proto`; mirrors the `sl-types` key ergonomics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProposalCandidateId(pub Uuid);

impl From<Uuid> for ProposalCandidateId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl ProposalCandidateId {
    /// Returns the wrapped raw `Uuid`.
    #[must_use]
    pub const fn uuid(self) -> Uuid {
        self.0
    }
}

impl core::fmt::Display for ProposalCandidateId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The agent's active group and title, parsed from `AgentDataUpdate` (pushed on
/// login and whenever the active group changes via
/// [`Session::activate_group`](crate::Session::activate_group)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGroup {
    /// The agent the update is about.
    pub agent_id: AgentKey,
    /// The agent's first name.
    pub first_name: String,
    /// The agent's last name.
    pub last_name: String,
    /// The active group's title shown over the avatar (empty if no active group).
    pub group_title: String,
    /// The active group's id (nil if no active group).
    pub active_group_id: GroupKey,
    /// The agent's powers bitfield within the active group.
    pub group_powers: u64,
    /// The active group's name (empty if no active group).
    pub group_name: String,
}

/// One of the agent's group memberships, from an `AgentGroupDataUpdate` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembership {
    /// The group id.
    pub group_id: GroupKey,
    /// The agent's powers bitfield in the group.
    pub group_powers: u64,
    /// Whether the agent accepts notices from the group.
    pub accept_notices: bool,
    /// The group's insignia (texture id).
    pub group_insignia_id: TextureKey,
    /// The agent's land-tier contribution to the group, in square metres (NOT
    /// L$ — the wire `Contribution` is land area, surfaced by the viewer as
    /// `[AREA]`).
    pub contribution: LandArea,
    /// The group name.
    pub group_name: String,
}

/// One member of a group, from a `GroupMembersReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    /// The member's agent id.
    pub agent_id: AgentKey,
    /// The member's land-tier contribution, in square metres (NOT L$ — the wire
    /// `Contribution` is land area, surfaced by the viewer as `[AREA]`).
    pub contribution: LandArea,
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
    pub role_id: GroupRoleKey,
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
    pub role_id: GroupRoleKey,
    /// The member's agent id.
    pub member_id: AgentKey,
}

/// One title the agent may wear in a group, from a `GroupTitlesReply` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTitle {
    /// The title text.
    pub title: String,
    /// The role the title belongs to.
    pub role_id: GroupRoleKey,
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
    pub group_id: GroupKey,
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
    pub insignia_id: TextureKey,
    /// The group founder's agent id.
    pub founder_id: AgentKey,
    /// The L$ fee to join.
    pub membership_fee: LindenAmount,
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
    pub owner_role: GroupRoleKey,
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
    pub insignia_id: TextureKey,
    /// The L$ fee to join.
    pub membership_fee: LindenAmount,
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
    pub notice_id: GroupNoticeKey,
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
#[non_exhaustive]
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
    pub role_id: GroupRoleKey,
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
    pub role_id: GroupRoleKey,
    /// The member's agent id.
    pub member_id: AgentKey,
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
    pub item_id: InventoryKey,
    /// The item owner's agent id (usually the sender).
    pub owner_id: Uuid,
}

/// A group's financial summary for an accounting interval, parsed from
/// `GroupAccountSummaryReply` (the answer to
/// [`Command::RequestGroupAccountSummary`](crate::Command::RequestGroupAccountSummary)).
/// All monetary fields are L$. `current_interval` selects this period (0) or the
/// previous one (1); `interval_days` is the period length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccountSummary {
    /// The group the summary is for.
    pub group_id: GroupKey,
    /// The client-chosen request id echoed from the request, for correlation.
    pub request_id: Uuid,
    /// The accounting interval length in days.
    pub interval_days: i32,
    /// Which interval this is (0 = current, 1 = previous).
    pub current_interval: i32,
    /// The interval's start date (grid-formatted string).
    pub start_date: String,
    /// The group's current L$ balance (signed — a group can be in the red).
    pub balance: LindenBalance,
    /// Total L$ credited over the interval.
    pub total_credits: LindenAmount,
    /// Total L$ debited over the interval.
    pub total_debits: LindenAmount,
    /// Current object tax.
    pub object_tax_current: LindenAmount,
    /// Current light tax.
    pub light_tax_current: LindenAmount,
    /// Current land tax.
    pub land_tax_current: LindenAmount,
    /// Current group tax.
    pub group_tax_current: LindenAmount,
    /// Current parcel-directory listing fee.
    pub parcel_dir_fee_current: LindenAmount,
    /// Estimated object tax for the next interval.
    pub object_tax_estimate: LindenAmount,
    /// Estimated light tax for the next interval.
    pub light_tax_estimate: LindenAmount,
    /// Estimated land tax for the next interval.
    pub land_tax_estimate: LindenAmount,
    /// Estimated group tax for the next interval.
    pub group_tax_estimate: LindenAmount,
    /// Estimated parcel-directory listing fee for the next interval.
    pub parcel_dir_fee_estimate: LindenAmount,
    /// The number of members that count toward tax (non-exempt).
    pub non_exempt_members: i32,
    /// The date of the last tax assessment (grid-formatted string).
    pub last_tax_date: String,
    /// The date of the next tax assessment (grid-formatted string).
    pub tax_date: String,
}

/// One line of a group's accounting detail, from a `GroupAccountDetailsReply`
/// entry: a single tax/fee charge with its L$ amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccountDetailsEntry {
    /// What the charge is for (grid-formatted string).
    pub description: String,
    /// The L$ amount of the charge (signed — a credit is positive, a debit
    /// negative).
    pub amount: LindenBalance,
}

/// A group's itemised accounting detail for an interval, parsed from
/// `GroupAccountDetailsReply` (the answer to
/// [`Command::RequestGroupAccountDetails`](crate::Command::RequestGroupAccountDetails)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccountDetails {
    /// The group the detail is for.
    pub group_id: GroupKey,
    /// The client-chosen request id echoed from the request, for correlation.
    pub request_id: Uuid,
    /// The accounting interval length in days.
    pub interval_days: i32,
    /// Which interval this is (0 = current, 1 = previous).
    pub current_interval: i32,
    /// The interval's start date (grid-formatted string).
    pub start_date: String,
    /// The detail lines for the interval.
    pub entries: Vec<GroupAccountDetailsEntry>,
}

/// One entry in a group's transaction log, from a `GroupAccountTransactionsReply`
/// entry: a single dated L$ transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccountTransaction {
    /// When the transaction happened (grid-formatted string).
    pub time: String,
    /// The other party's name (grid-formatted string).
    pub user: String,
    /// The transaction type code (matches the `MoneyTransactionType` family).
    pub transaction_type: i32,
    /// A description of the item/reason (grid-formatted string).
    pub item: String,
    /// The L$ amount (positive credit, negative debit).
    pub amount: LindenBalance,
}

/// A group's transaction log for an interval, parsed from
/// `GroupAccountTransactionsReply` (the answer to
/// [`Command::RequestGroupAccountTransactions`](crate::Command::RequestGroupAccountTransactions)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAccountTransactions {
    /// The group the log is for.
    pub group_id: GroupKey,
    /// The client-chosen request id echoed from the request, for correlation.
    pub request_id: Uuid,
    /// The accounting interval length in days.
    pub interval_days: i32,
    /// Which interval this is (0 = current, 1 = previous).
    pub current_interval: i32,
    /// The interval's start date (grid-formatted string).
    pub start_date: String,
    /// The transactions over the interval.
    pub entries: Vec<GroupAccountTransaction>,
}

/// One active group proposal, from a `GroupActiveProposalItemReply` entry. The
/// agent votes on it via
/// [`Command::GroupProposalBallot`](crate::Command::GroupProposalBallot).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupActiveProposalItem {
    /// The proposal's id (used as the ballot's `proposal_id`).
    pub vote_id: ProposalVoteId,
    /// The agent that started the proposal.
    pub vote_initiator: AgentKey,
    /// A terse date id (grid-internal string).
    pub terse_date_id: String,
    /// When voting opened (grid-formatted string).
    pub start_date_time: String,
    /// When voting closes (grid-formatted string).
    pub end_date_time: String,
    /// Whether the requesting agent has already voted.
    pub already_voted: bool,
    /// The vote the requesting agent already cast (empty if none).
    pub vote_cast: String,
    /// The fraction of votes needed to pass (0.0–1.0).
    pub majority: f32,
    /// The minimum number of votes required for the result to count.
    pub quorum: i32,
    /// The proposal's text.
    pub proposal_text: String,
}

/// One candidate tally within a finished proposal, from a
/// `GroupVoteHistoryItemReply` `VoteItem` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupVote {
    /// The candidate/option id (or the voter for a yes/no proposal).
    pub candidate_id: ProposalCandidateId,
    /// The vote text cast (e.g. `"yes"`/`"no"`).
    pub vote_cast: String,
    /// How many votes this candidate received.
    pub num_votes: i32,
}

/// One finished proposal from a group's vote history, parsed from a
/// `GroupVoteHistoryItemReply` (the answer to
/// [`Command::RequestGroupVoteHistory`](crate::Command::RequestGroupVoteHistory)).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupVoteHistoryItem {
    /// The proposal's id.
    pub vote_id: ProposalVoteId,
    /// A terse date id (grid-internal string).
    pub terse_date_id: String,
    /// When voting opened (grid-formatted string).
    pub start_date_time: String,
    /// When voting closed (grid-formatted string).
    pub end_date_time: String,
    /// The agent that started the proposal.
    pub vote_initiator: AgentKey,
    /// The proposal/vote type (grid-formatted string).
    pub vote_type: String,
    /// The outcome (grid-formatted string, e.g. `"Success"`).
    pub vote_result: String,
    /// The fraction of votes that was needed to pass (0.0–1.0).
    pub majority: f32,
    /// The minimum number of votes that was required.
    pub quorum: i32,
    /// The proposal's text.
    pub proposal_text: String,
    /// The per-candidate tallies.
    pub votes: Vec<GroupVote>,
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

#[cfg(test)]
mod tests {
    use super::{GroupNoticeKey, GroupRoleKey, ProposalCandidateId, ProposalVoteId};
    use pretty_assertions::assert_eq;
    use sl_types::key::GroupKey;
    use uuid::Uuid;

    #[test]
    fn group_role_key_round_trips_bit_identically() {
        let raw = Uuid::from_u128(0x0102_0304_0506_0708_090a_0b0c_0d0e_0f10);
        let key = GroupRoleKey::from(raw);
        // Wrapping then extracting must yield the exact wire bytes back.
        assert_eq!(key.uuid(), raw);
        // The Display rendering matches the underlying UUID.
        assert_eq!(key.to_string(), raw.to_string());
        // The nil "Everyone" role survives the round trip too.
        assert_eq!(GroupRoleKey::from(Uuid::nil()).uuid(), Uuid::nil());
    }

    #[test]
    fn group_and_role_keys_are_distinct_types() {
        // A group id and a role id wrap the same UUID space but are different
        // types, so a role id can never be passed where a group id is expected
        // (this is the whole point of the sweep — enforced at compile time).
        let raw = Uuid::from_u128(0xdead_beef);
        let group = GroupKey::from(raw);
        let role = GroupRoleKey::from(raw);
        assert_eq!(group.uuid(), role.uuid());
    }

    /// The three new client-local group ids (`GroupNoticeKey`, `ProposalVoteId`,
    /// `ProposalCandidateId`) are transparent wrappers over the wire `Uuid`:
    /// wrapping then extracting is the identity, and `Display` matches the raw
    /// id's.
    #[test]
    fn new_group_ids_round_trip_raw_uuid() {
        let raw = Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888);
        assert_eq!(GroupNoticeKey::from(raw).uuid(), raw);
        assert_eq!(ProposalVoteId::from(raw).uuid(), raw);
        assert_eq!(ProposalCandidateId::from(raw).uuid(), raw);
        assert_eq!(GroupNoticeKey::from(raw).to_string(), raw.to_string());
        assert_eq!(ProposalVoteId::from(Uuid::nil()).uuid(), Uuid::nil());
    }

    /// A proposal id and a candidate (option-within-proposal) id are distinct
    /// types over the same UUID space, so the two can never be transposed (the
    /// point of the split — enforced at compile time).
    #[test]
    fn proposal_and_candidate_ids_are_distinct_types() {
        let raw = Uuid::from_u128(0xb01d_face);
        let vote = ProposalVoteId::from(raw);
        let candidate = ProposalCandidateId::from(raw);
        assert_eq!(vote.uuid(), candidate.uuid());
    }
}
