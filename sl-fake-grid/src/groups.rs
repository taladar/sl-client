//! Groups: founding, membership, roles and titles, notices, invitations,
//! ejection and bans, held by the grid (a `GroupStore` on its `GridCore`) and
//! answered by a responder (`run_groups_responder`) on each session.
//!
//! What each live grid tells whom was measured by the `groups` conformance
//! case on aditi and the local OpenSim (2026-10-14,
//! `book/src/gridspec/groups.md`), and every divergence is a field of
//! [`GroupsPolicy`]:
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | founding | L$100, two balance updates, no text; a name cut to 35 | free, "Group created successfully", the group made active |
//! | stock roles | "Member" / "Officer" / "Owner", the owners every bit | "Member of …" / "Officer of …" / "Owner of …" |
//! | a profile | reads like a cache: 1 member, 2 roles, everyone's title and powers | as the group stands, the reader's own title and powers |
//! | the roster over UDP | empty | every member |
//! | role members | no Everyone pairs | an Everyone pair for every member |
//! | titles | by the role's title | by the role's name; a role assigned is worn |
//! | a member's powers | every role's | the worn role's |
//! | a role edit | tells nobody | sends the editor its active group |
//! | joining, leaving, an accept | alerts, failed balance updates, the drop over the event queue | the drop over UDP, the active group and the memberships; an accept a message box from "Groups" |
//! | a notice | from the sender, an id per recipient, listed under another | from the group, kept, the notice's id everywhere |
//! | accounts and proposals | answered, empty | not answered |
//! | an ejection | told by message, nothing more | told by message, dropped, the memberships |
//! | an invitation | from the sender's user name, with the charter; to a member too | from its name, with the group's region; not to a member |
//! | bans | over `GroupAPIv1` | no such capability |
//!
//! Not modelled: group chat (`gridspec-group-chat-and-conference`), a join
//! fee, a closed group, land and money held by a group, and proposals that
//! exist.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Instant;

use sl_proto::{
    ActiveGroup, CreateGroupParams, GroupAccountDetails, GroupAccountSummary,
    GroupAccountTransactions, GroupBan, GroupBanAction, GroupMember, GroupMembership, GroupNotice,
    GroupNoticeKey, GroupProfile, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleMember,
    GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, GroupVoteHistoryItem, ImDialog,
    InstantMessage, KeptMessage, LandArea, LindenBalance, MoneyBalance, MoneyTransaction,
    ProposalVoteId, ServerEvent, SimSession, UpdateGroupInfoParams,
};
use sl_types::key::{AgentKey, GroupKey, GroupRoleKey, InventoryKey, OwnerKey, TextureKey};
use sl_types::money::LindenAmount;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::driver::{SharedSim, SimState};
use crate::runtime::{GridCore, SessionRole};
use crate::social::{Whereabouts, legacy_name, message_from, now_epoch_seconds, tell, warn_on};

/// The power to deed an object to the group (`GP_OBJECT_DEED`).
pub const POWER_OBJECT_DEED: u64 = 1 << 33;

/// The power to move and edit the group's objects (`GP_OBJECT_MANIPULATE`).
pub const POWER_OBJECT_MANIPULATE: u64 = 1 << 34;

/// The description of a group's Everyone role, on both grids.
const EVERYONE_DESCRIPTION: &str = "Everyone in the group is in the everyone role.";

/// The description of a group's Officers role, on both grids.
const OFFICERS_DESCRIPTION: &str =
    "The officers of the group, with more powers than regular members.";

/// What Second Life charges for a founding, in L$.
pub const SECOND_LIFE_FOUNDING_FEE: i32 = 100;

/// The transaction type of a founding fee (`TRANS_GROUP_CREATE`).
const GROUP_CREATE_TRANSACTION: i32 = 1002;

/// What the grid says when a member is added, on Second Life by alert and on
/// OpenSim by a message box.
const ADDED: &str = "You have been added to the group.";

/// How a grid runs groups
/// ([`ImitatedGrid::groups_policy`](crate::ImitatedGrid::groups_policy)).
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one measured divergence between the two grids"
)]
#[expect(
    clippy::module_name_repetitions,
    reason = "read where it is used, `ImitatedGrid::groups_policy`, beside the other \
              policies, where a bare `Policy` would say nothing"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupsPolicy {
    /// What a founding costs, in L$; a fee is told in two balance updates.
    pub founding_fee: i32,
    /// How many characters of a name a founding keeps; `None` for all.
    pub name_limit: Option<usize>,
    /// The text a founding is answered with.
    pub founding_text: &'static str,
    /// The roles a group is founded with.
    pub stock_roles: StockRoles,
    /// Whether founding and joining make the group the agent's active one.
    pub activates_on_join: bool,
    /// Whether a profile reads like a cache — one member, two roles, and
    /// everyone's title and powers for whoever reads it — rather than as the
    /// group stands.
    pub stale_profile: bool,
    /// Whether `GroupMembersReply` lists the members, rather than none.
    pub roster_over_udp: bool,
    /// Whether every member is listed in the Everyone role among the role
    /// members, and the Everyone role counts them.
    pub everyone_pairs: bool,
    /// Whether a title is named by its role's name, rather than its title.
    pub titles_by_role_name: bool,
    /// Whether a member's powers are those of the role whose title it wears,
    /// rather than those of every role it holds.
    pub powers_of_worn_role: bool,
    /// Whether a role assigned to a member is worn at once.
    pub assignment_wears: bool,
    /// Whether an outsider is told a group's roles.
    pub roles_to_outsiders: bool,
    /// Whether an edit of a role, or of who holds it, sends the editor its
    /// active group.
    pub role_edits_told: bool,
    /// Whether a join, a leave and an accepted invitation are told by alert,
    /// a leave and an ejection followed by two failed balance updates and the
    /// drop over the event queue: Second Life. OpenSim tells by the drop over
    /// UDP, the active group and the memberships, and an accept by a message
    /// box from "Groups".
    pub told_by_alerts: bool,
    /// Whether a notice is from the group and marked kept, under the notice's
    /// id, rather than from its sender under an id per recipient.
    pub notices_from_group: bool,
    /// Whether the accounts and the proposals are answered.
    pub accounts_answered: bool,
    /// Whether an avatar that is already a member is sent an invitation.
    pub invites_members: bool,
    /// How an invitation is worded.
    pub invitation: InvitationForm,
    /// Whether a deed of an object not shared with the group is taken.
    pub deeds_unshared: bool,
    /// The alert a move of a group's object without the power is refused
    /// with; `None` for a silent refusal.
    pub refused_move: Option<&'static str>,
}

impl Default for GroupsPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.groups_policy()
    }
}

/// The roles a group is founded with ([`GroupsPolicy::stock_roles`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockRoles {
    /// "Member", "Officer" and "Owner", the owners every bit.
    SecondLife,
    /// "Member of …", "Officer of …" and "Owner of …".
    OpenSim,
}

/// How an invitation is worded ([`GroupsPolicy::invitation`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationForm {
    /// From the inviter's user name, from no region, naming the group and
    /// its charter.
    SecondLife,
    /// From the inviter's name, from its region, naming the group.
    OpenSim,
}

impl StockRoles {
    /// The Everyone role, the Officers and the Owners of a group called
    /// `name`, the latter two under `officers` and `owners`.
    fn roles(self, name: &str, officers: GroupRoleKey, owners: GroupRoleKey) -> Vec<Role> {
        let role = |id, role_name: &str, title: String, description: &str, powers| Role {
            id,
            name: role_name.to_owned(),
            title,
            description: description.to_owned(),
            powers,
        };
        match self {
            Self::SecondLife => vec![
                role(
                    None,
                    "Everyone",
                    "Member".to_owned(),
                    EVERYONE_DESCRIPTION,
                    0x0800_1801_0000,
                ),
                role(
                    Some(officers),
                    "Officers",
                    "Officer".to_owned(),
                    OFFICERS_DESCRIPTION,
                    0x0001_ccff_ffff_f8ce,
                ),
                role(
                    Some(owners),
                    "Owners",
                    "Owner".to_owned(),
                    "These are the owners of the group.  They always have FULL POWER over the \
                     group.",
                    u64::MAX,
                ),
            ],
            Self::OpenSim => vec![
                role(
                    None,
                    "Everyone",
                    format!("Member of {name}"),
                    EVERYONE_DESCRIPTION,
                    0x3900_1801_0000,
                ),
                role(
                    Some(officers),
                    "Officers",
                    format!("Officer of {name}"),
                    OFFICERS_DESCRIPTION,
                    0x0001_fdff_ff7f_f8ce,
                ),
                role(
                    Some(owners),
                    "Owners",
                    format!("Owner of {name}"),
                    "Owners of the group",
                    0x000f_ffff_ffff_fffe,
                ),
            ],
        }
    }
}

/// One role of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Role {
    /// The role's id; `None` for the Everyone role.
    id: Option<GroupRoleKey>,
    /// Its name.
    name: String,
    /// The title its holders wear.
    title: String,
    /// Its description.
    description: String,
    /// The powers it grants.
    powers: u64,
}

/// One member of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    /// The roles it holds besides the Everyone role.
    roles: BTreeSet<GroupRoleKey>,
    /// The role whose title it wears; `None` for the Everyone role.
    worn: Option<GroupRoleKey>,
    /// Whether it takes the group's notices.
    accept_notices: bool,
    /// When it joined, as the grid counts: a membership list names the group
    /// joined last first.
    joined: u64,
}

impl Member {
    /// A member in the Everyone role alone, joined `joined`.
    const fn plain(joined: u64) -> Self {
        Self {
            roles: BTreeSet::new(),
            worn: None,
            accept_notices: true,
            joined,
        }
    }
}

/// One notice posted to a group.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Notice {
    /// The id the notice is listed under.
    listed_id: Uuid,
    /// Its sender.
    from: AgentKey,
    /// Its sender's name.
    from_name: String,
    /// Its subject.
    subject: String,
    /// Its text as posted: `subject|message`.
    text: String,
    /// When it was posted.
    timestamp: u32,
    /// Its attachment's asset type and name.
    attachment: Option<(u8, String)>,
}

/// One group.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four flags are the group's own, as `GroupProfile` states them"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Group {
    /// Its id.
    id: GroupKey,
    /// Its name.
    name: String,
    /// Its charter.
    charter: String,
    /// Whether it is shown in search.
    show_in_list: bool,
    /// Its insignia.
    insignia: Option<TextureKey>,
    /// Its founder.
    founder: AgentKey,
    /// Its fee to join.
    fee: LindenAmount,
    /// Whether anybody may join.
    open_enrollment: bool,
    /// Whether it is published on the web.
    allow_publish: bool,
    /// Whether it is mature.
    mature_publish: bool,
    /// Its Owners role.
    owner_role: GroupRoleKey,
    /// Its roles, the Everyone role first.
    roles: Vec<Role>,
    /// Its members.
    members: BTreeMap<AgentKey, Member>,
    /// Its notices, oldest first.
    notices: Vec<Notice>,
    /// Who is banned, and since when.
    bans: BTreeMap<AgentKey, String>,
}

impl Group {
    /// The role `id` names (`None` for the Everyone role).
    fn role(&self, id: Option<GroupRoleKey>) -> Option<&Role> {
        self.roles.iter().find(|role| role.id == id)
    }

    /// The Everyone role.
    fn everyone(&self) -> Option<&Role> {
        self.role(None)
    }

    /// `agent`'s powers, as `policy` counts them; 0 for a non-member.
    fn powers(&self, agent: AgentKey, policy: &GroupsPolicy) -> u64 {
        let Some(member) = self.members.get(&agent) else {
            return 0;
        };
        if policy.powers_of_worn_role {
            return self.role(member.worn).map_or(0, |role| role.powers);
        }
        member
            .roles
            .iter()
            .filter_map(|id| self.role(Some(*id)))
            .chain(self.everyone())
            .fold(0, |powers, role| powers | role.powers)
    }

    /// The title `agent` wears; empty for a non-member.
    fn title(&self, agent: AgentKey) -> String {
        self.members
            .get(&agent)
            .and_then(|member| self.role(member.worn))
            .map_or_else(String::new, |role| role.title.clone())
    }

    /// Whether `agent` holds the Owners role.
    fn is_owner(&self, agent: AgentKey) -> bool {
        self.members
            .get(&agent)
            .is_some_and(|member| member.roles.contains(&self.owner_role))
    }
}

/// An invitation sent and not yet answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Invitation {
    /// The group invited into.
    group: GroupKey,
    /// The avatar invited.
    invitee: AgentKey,
    /// The role offered; `None` for the Everyone role.
    role: Option<GroupRoleKey>,
}

/// The grid's groups.
#[derive(Debug, Default)]
pub(crate) struct GroupStore {
    /// Every group, by id.
    groups: BTreeMap<GroupKey, Group>,
    /// The invitations not yet answered, by the id they were sent under.
    invitations: HashMap<Uuid, Invitation>,
    /// Each avatar's active group.
    active: HashMap<AgentKey, GroupKey>,
    /// How many memberships the grid has made.
    joins: u64,
}

/// What a session knows of its agent's groups: what it says of the active
/// one in an `AgentDataUpdate`, and the agent's powers in each, which a deed
/// and a move of a group's object are checked against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentGroups {
    /// The active group: its id, name, the title worn and the powers.
    pub(crate) active: Option<(GroupKey, String, String, u64)>,
    /// The agent's powers in each of its groups.
    pub(crate) powers: BTreeMap<GroupKey, u64>,
}

impl AgentGroups {
    /// The `AgentDataUpdate` of `identity` this states.
    pub(crate) fn active_group(&self, identity: &crate::world::AvatarIdentity) -> ActiveGroup {
        let (active_group_id, group_name, group_title, group_powers) = self.active.clone().map_or(
            (None, String::new(), String::new(), 0),
            |(id, name, title, powers)| (Some(id), name, title, powers),
        );
        ActiveGroup {
            agent_id: identity.agent_id,
            first_name: identity.first_name.clone(),
            last_name: identity.last_name.clone(),
            group_title,
            active_group_id,
            group_powers,
            group_name,
        }
    }

    /// Whether the agent holds `power` in `group`.
    pub(crate) fn may(&self, group: GroupKey, power: u64) -> bool {
        self.powers
            .get(&group)
            .is_some_and(|powers| powers & power != 0)
    }
}

/// A group the grid holds from its start
/// ([`FakeGridBuilder::group`](crate::FakeGridBuilder::group)), founded by
/// `founder` with every other account in `members` a member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupConfig {
    /// The group's id.
    pub group_id: GroupKey,
    /// Its name.
    pub name: String,
    /// Its charter.
    pub charter: String,
    /// The `First Last` name of its founder, who owns it.
    pub founder: (String, String),
    /// The `First Last` names of its other members.
    pub members: Vec<(String, String)>,
}

impl GroupStore {
    /// A grid's groups, holding `stock` — each founded by its founder with
    /// its members in it, each group's two role ids minted by `mint`.
    pub(crate) fn with_groups(
        stock: Vec<(GroupConfig, AgentKey, Vec<AgentKey>)>,
        policy: &GroupsPolicy,
        mint: &dyn Fn() -> Uuid,
    ) -> Self {
        let mut groups = Self::default();
        for (config, founder, members) in stock {
            let params = CreateGroupParams {
                name: config.name,
                charter: config.charter,
                show_in_list: false,
                insignia_id: None,
                membership_fee: LindenAmount(0),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            };
            groups.found(config.group_id, &params, founder, policy, mint);
            for member in members {
                groups.add_member(config.group_id, member);
            }
        }
        groups
    }

    /// Founds a group `id` as `params` say, `founder` its owner.
    fn found(
        &mut self,
        id: GroupKey,
        params: &CreateGroupParams,
        founder: AgentKey,
        policy: &GroupsPolicy,
        mint: &dyn Fn() -> Uuid,
    ) {
        let name: String = match policy.name_limit {
            Some(limit) => params.name.chars().take(limit).collect(),
            None => params.name.clone(),
        };
        let officers = GroupRoleKey::from(mint());
        let owners = GroupRoleKey::from(mint());
        let roles = policy.stock_roles.roles(&name, officers, owners);
        let mut members = BTreeMap::new();
        members.insert(
            founder,
            Member {
                roles: BTreeSet::from([owners]),
                worn: Some(owners),
                accept_notices: true,
                joined: self.next_join(),
            },
        );
        self.groups.insert(
            id,
            Group {
                id,
                name,
                charter: params.charter.clone(),
                show_in_list: params.show_in_list,
                insignia: params.insignia_id,
                founder,
                fee: params.membership_fee.clone(),
                open_enrollment: params.open_enrollment,
                allow_publish: params.allow_publish,
                mature_publish: params.mature_publish,
                owner_role: owners,
                roles,
                members,
                notices: Vec::new(),
                bans: BTreeMap::new(),
            },
        );
    }

    /// The next membership's place in the order they were made.
    const fn next_join(&mut self) -> u64 {
        self.joins = self.joins.saturating_add(1);
        self.joins
    }

    /// Makes `agent` a member of `group`, in the Everyone role alone, where it
    /// is none yet; answers its membership.
    fn add_member(&mut self, group: GroupKey, agent: AgentKey) -> Option<&mut Member> {
        let joined = self.next_join();
        Some(
            self.groups
                .get_mut(&group)?
                .members
                .entry(agent)
                .or_insert_with(|| Member::plain(joined)),
        )
    }

    /// Every group `agent` is in, as an `AgentGroupDataUpdate` lists them:
    /// the one joined last first.
    fn memberships(&self, agent: AgentKey, policy: &GroupsPolicy) -> Vec<GroupMembership> {
        let mut groups: Vec<(&Group, &Member)> = self
            .groups
            .values()
            .filter_map(|group| Some((group, group.members.get(&agent)?)))
            .collect();
        groups.sort_by_key(|(_group, member)| std::cmp::Reverse(member.joined));
        groups
            .into_iter()
            .map(|(group, member)| GroupMembership {
                group_id: group.id,
                group_powers: group.powers(agent, policy),
                accept_notices: member.accept_notices,
                group_insignia_id: TextureKey::from(
                    group
                        .insignia
                        .map_or_else(Uuid::nil, |insignia| insignia.uuid()),
                ),
                contribution: LandArea(0),
                group_name: group.name.clone(),
            })
            .collect()
    }

    /// What a session of `agent` knows of its groups.
    fn agent_groups(&self, agent: AgentKey, policy: &GroupsPolicy) -> AgentGroups {
        let active = self
            .active
            .get(&agent)
            .and_then(|id| self.groups.get(id))
            .filter(|group| group.members.contains_key(&agent))
            .map(|group| {
                (
                    group.id,
                    group.name.clone(),
                    group.title(agent),
                    group.powers(agent, policy),
                )
            });
        let powers = self
            .groups
            .values()
            .filter(|group| group.members.contains_key(&agent))
            .map(|group| (group.id, group.powers(agent, policy)))
            .collect();
        AgentGroups { active, powers }
    }

    /// What a session of `agent` starts out knowing of its groups.
    pub(crate) fn agent_groups_at_login(
        &self,
        agent: AgentKey,
        policy: &GroupsPolicy,
    ) -> AgentGroups {
        self.agent_groups(agent, policy)
    }
}

/// What the grid holds of `agent`'s groups now, pushed into every session of
/// it.
async fn sync(core: &GridCore, agent: AgentKey) {
    let known = core.groups.lock().agent_groups(agent, &core.groups_policy);
    for shared in core.sessions_of(agent).await {
        shared.state.lock().await.groups = known.clone();
    }
}

/// Sends `state`'s agent its active group.
fn send_active(state: &mut SimState, now: Instant) {
    let active = state.groups.active_group(&state.avatar);
    warn_on(
        state.sim.send_agent_data_update(&active, now),
        "the active group",
    );
}

/// Sends `agent` its memberships, as the grid holds them.
fn send_memberships(core: &GridCore, sim: &mut SimSession, agent: AgentKey, now: Instant) {
    let memberships = core.groups.lock().memberships(agent, &core.groups_policy);
    warn_on(
        sim.send_agent_group_data_update(&memberships, now),
        "the memberships",
    );
}

/// Sends `agent` its active group and its memberships.
async fn tell_standing(core: &GridCore, agent: AgentKey) {
    sync(core, agent).await;
    tell(core, agent, |state, now| {
        send_active(state, now);
        send_memberships(core, &mut state.sim, agent, now);
    })
    .await;
}

/// Sends `agent` two balance updates that state a transaction failed, as
/// Second Life follows a leave and an ejection with.
fn send_failed_balances(sim: &mut SimSession, agent: AgentKey, now: Instant) {
    let failed = MoneyBalance {
        agent_id: agent,
        transaction_id: Uuid::nil(),
        success: false,
        balance: LindenAmount(
            sim.money_balance()
                .and_then(|balance| u64::try_from(balance).ok())
                .unwrap_or(0),
        ),
        square_meters_credit: LandArea(0),
        square_meters_committed: LandArea(0),
        description: String::new(),
        transaction: None,
    };
    for _twice in 0..2 {
        warn_on(sim.send_money_balance_reply(&failed, now), "a balance");
    }
}

/// The user name of the account `agent`: `first` for a Resident,
/// `first.last` otherwise, in lower case.
fn user_name(core: &GridCore, agent: AgentKey) -> String {
    core.account_by_agent(agent)
        .map_or_else(String::new, |account| {
            let (first, last) = (&account.config.first_name, &account.config.last_name);
            if last.eq_ignore_ascii_case("Resident") {
                first.to_lowercase()
            } else {
                format!("{first}.{last}").to_lowercase()
            }
        })
}

/// The per-session responder for groups: answers what its client asks of
/// the grid's groups and tells the other avatars, until the session closes
/// or the grid shuts down.
pub(crate) async fn run_groups_responder(core: Arc<GridCore>, shared: SharedSim) {
    let agent = shared.state.lock().await.avatar.agent_id;
    let mut events = shared.subscribe_events();
    let mut closed_rx = shared.closed_tx.subscribe();
    let mut shutdown_rx = shared.shutdown_rx.clone();
    loop {
        if *closed_rx.borrow_and_update() || *shutdown_rx.borrow_and_update() {
            break;
        }
        let received = tokio::select! {
            received = events.recv() => received,
            changed = closed_rx.changed() => {
                if changed.is_err() || *closed_rx.borrow() {
                    break;
                }
                continue;
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
        };
        match received {
            Ok(event) => answer(&core, &shared, agent, event).await,
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!("groups responder missed {missed} events");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Answers one thing the client did.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per group request, each handing on to its own function"
)]
async fn answer(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, event: ServerEvent) {
    match event {
        ServerEvent::AgentArrived => {
            let root = matches!(shared.state.lock().await.role, SessionRole::Root);
            if root {
                let now = shared.now();
                shared
                    .with_sim(|sim| send_memberships(core, sim, agent, now))
                    .await;
            }
        }
        ServerEvent::RequestAgentDataUpdate => {
            let now = shared.now();
            shared
                .with_sim(|sim| send_memberships(core, sim, agent, now))
                .await;
        }
        ServerEvent::ClientMessage(message) => {
            if let sl_proto::AnyMessage::MoneyBalanceRequest(request) = *message {
                answer_balance(shared, agent, request.money_data.transaction_id).await;
            }
        }
        ServerEvent::CreateGroup { params } => found(core, shared, agent, &params).await,
        ServerEvent::JoinGroup { group_id } => join(core, shared, agent, group_id).await,
        ServerEvent::LeaveGroup { group_id } => leave(core, shared, agent, group_id).await,
        ServerEvent::EjectGroupMembers {
            group_id,
            member_ids,
        } => {
            for member in member_ids {
                eject(core, shared, agent, group_id, member).await;
            }
        }
        ServerEvent::InviteToGroup { group_id, invitees } => {
            invite(core, shared, agent, group_id, &invitees).await;
        }
        ServerEvent::ActivateGroup { group_id } => activate(core, agent, group_id).await,
        ServerEvent::SetGroupAcceptNotices {
            group_id,
            accept_notices,
            ..
        } => {
            let mut groups = core.groups.lock();
            if let Some(member) = groups
                .groups
                .get_mut(&group_id)
                .and_then(|group| group.members.get_mut(&agent))
            {
                member.accept_notices = accept_notices;
            }
        }
        ServerEvent::UpdateGroupInfo { params } => update_info(core, agent, &params),
        ServerEvent::UpdateGroupTitle {
            group_id,
            title_role_id,
        } => wear(core, agent, group_id, title_role_id).await,
        ServerEvent::UpdateGroupRoles { group_id, roles } => {
            edit_roles(core, agent, group_id, &roles).await;
        }
        ServerEvent::ChangeGroupRoleMembers { group_id, changes } => {
            change_role_members(core, agent, group_id, &changes).await;
        }
        ServerEvent::RequestGroupProfile { group_id } => {
            reply_profile(core, shared, agent, group_id).await;
        }
        ServerEvent::RequestGroupMembers {
            group_id,
            request_id,
        } => reply_members(core, shared, group_id, request_id).await,
        ServerEvent::RequestGroupRoles {
            group_id,
            request_id,
        } => reply_roles(core, shared, agent, group_id, request_id).await,
        ServerEvent::RequestGroupRoleMembers {
            group_id,
            request_id,
        } => reply_role_members(core, shared, group_id, request_id).await,
        ServerEvent::RequestGroupTitles {
            group_id,
            request_id,
        } => reply_titles(core, shared, agent, group_id, request_id).await,
        ServerEvent::RequestGroupNotices { group_id } => {
            reply_notices(core, shared, group_id).await;
        }
        ServerEvent::RequestGroupNotice { notice_id } => {
            notice_again(core, shared, agent, notice_id).await;
        }
        ServerEvent::InstantMessage(im) => match im.dialog {
            ImDialog::GroupNotice => post_notice(core, shared, agent, &im).await,
            ImDialog::GroupInvitationAccept | ImDialog::GroupInvitationDecline => {
                let accept = im.dialog == ImDialog::GroupInvitationAccept;
                let invitation = take_invitation(core, im.id, agent);
                if let Some(invitation) = invitation {
                    answered(core, shared, invitation, accept).await;
                }
            }
            _ => {}
        },
        ServerEvent::KeptGroupInvitationAnswered { group_id, accept } => {
            kept_answered(core, shared, agent, group_id, accept).await;
        }
        ServerEvent::RequestGroupAccountSummary {
            group_id,
            request_id,
            interval_days,
            current_interval,
        } => {
            if core.groups_policy.accounts_answered {
                let members = core
                    .groups
                    .lock()
                    .groups
                    .get(&group_id)
                    .map_or(0, |group| group.members.len());
                let today = today();
                let summary = GroupAccountSummary {
                    group_id,
                    request_id,
                    interval_days,
                    current_interval,
                    start_date: today.clone(),
                    balance: LindenBalance::from_i32(0),
                    total_credits: LindenAmount(0),
                    total_debits: LindenAmount(0),
                    object_tax_current: LindenAmount(0),
                    light_tax_current: LindenAmount(0),
                    land_tax_current: LindenAmount(0),
                    group_tax_current: LindenAmount(0),
                    parcel_dir_fee_current: LindenAmount(0),
                    object_tax_estimate: LindenAmount(0),
                    light_tax_estimate: LindenAmount(0),
                    land_tax_estimate: LindenAmount(0),
                    group_tax_estimate: LindenAmount(0),
                    parcel_dir_fee_estimate: LindenAmount(0),
                    non_exempt_members: i32::try_from(members).unwrap_or(i32::MAX),
                    last_tax_date: today.clone(),
                    tax_date: today,
                };
                let now = shared.now();
                shared
                    .with_sim(|sim| {
                        warn_on(
                            sim.send_group_account_summary_reply(&summary, now),
                            "accounts",
                        );
                    })
                    .await;
            }
        }
        ServerEvent::RequestGroupAccountDetails {
            group_id,
            request_id,
            interval_days,
            current_interval,
        } => {
            if core.groups_policy.accounts_answered {
                let details = GroupAccountDetails {
                    group_id,
                    request_id,
                    interval_days,
                    current_interval,
                    start_date: today(),
                    entries: Vec::new(),
                };
                let now = shared.now();
                shared
                    .with_sim(|sim| {
                        warn_on(
                            sim.send_group_account_details_reply(&details, now),
                            "accounts",
                        );
                    })
                    .await;
            }
        }
        ServerEvent::RequestGroupAccountTransactions {
            group_id,
            request_id,
            interval_days,
            current_interval,
        } => {
            if core.groups_policy.accounts_answered {
                let transactions = GroupAccountTransactions {
                    group_id,
                    request_id,
                    interval_days,
                    current_interval,
                    start_date: today(),
                    entries: Vec::new(),
                };
                let now = shared.now();
                shared
                    .with_sim(|sim| {
                        warn_on(
                            sim.send_group_account_transactions_reply(&transactions, now),
                            "accounts",
                        );
                    })
                    .await;
            }
        }
        ServerEvent::RequestGroupActiveProposals {
            group_id,
            transaction_id,
        } => {
            if core.groups_policy.accounts_answered {
                let now = shared.now();
                shared
                    .with_sim(|sim| {
                        warn_on(
                            sim.send_group_active_proposals_reply(
                                group_id,
                                transaction_id,
                                0,
                                &[],
                                now,
                            ),
                            "proposals",
                        );
                    })
                    .await;
            }
        }
        ServerEvent::RequestGroupVoteHistory {
            group_id,
            transaction_id,
        } if core.groups_policy.accounts_answered => {
            let none = GroupVoteHistoryItem {
                vote_id: ProposalVoteId::from(Uuid::nil()),
                terse_date_id: String::new(),
                start_date_time: String::new(),
                end_date_time: String::new(),
                vote_initiator: AgentKey::from(Uuid::nil()),
                vote_type: String::new(),
                vote_result: String::new(),
                majority: 0.0,
                quorum: 0,
                proposal_text: String::new(),
                votes: Vec::new(),
            };
            let now = shared.now();
            shared
                .with_sim(|sim| {
                    warn_on(
                        sim.send_group_vote_history_reply(group_id, transaction_id, 0, &none, now),
                        "proposals",
                    );
                })
                .await;
        }
        _ => {}
    }
}

/// The calendar date and the time of day of `seconds` since the epoch, UTC.
fn calendar(seconds: i64) -> Option<(String, String)> {
    let at = time::OffsetDateTime::from_unix_timestamp(seconds).ok()?;
    let date = format!(
        "{:04}-{:02}-{:02}",
        at.year(),
        u8::from(at.month()),
        at.day()
    );
    let time = format!("{:02}:{:02}:{:02}", at.hour(), at.minute(), at.second());
    Some((date, time))
}

/// Today, as the accounts state a date.
fn today() -> String {
    calendar(i64::from(now_epoch_seconds())).map_or_else(String::new, |(date, _time)| date)
}

/// Now, as a ban states when it was made: an ISO-8601 date and time.
fn ban_date() -> String {
    calendar(i64::from(now_epoch_seconds()))
        .map_or_else(String::new, |(date, time)| format!("{date}T{time}Z"))
}

/// Sends a plain alert of `text`.
fn alert(sim: &mut SimSession, text: &str, now: Instant) {
    warn_on(sim.send_alert_message(text, &[], &[], now), "an alert");
}

/// Answers a `MoneyBalanceRequest` with the balance the session keeps, or
/// none.
async fn answer_balance(shared: &SharedSim, agent: AgentKey, transaction_id: Uuid) {
    let now = shared.now();
    shared
        .with_sim(|sim| {
            let balance = MoneyBalance {
                agent_id: agent,
                transaction_id,
                success: true,
                balance: LindenAmount(
                    sim.money_balance()
                        .and_then(|balance| u64::try_from(balance).ok())
                        .unwrap_or(0),
                ),
                square_meters_credit: LandArea(0),
                square_meters_committed: LandArea(0),
                description: String::new(),
                transaction: None,
            };
            warn_on(sim.send_money_balance_reply(&balance, now), "a balance");
        })
        .await;
}

/// An avatar founded a group.
async fn found(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    agent: AgentKey,
    params: &CreateGroupParams,
) {
    let policy = core.groups_policy;
    let id = GroupKey::from(core.minter.uuid());
    {
        let minter = core.minter.clone();
        let mut groups = core.groups.lock();
        groups.found(id, params, agent, &policy, &move || minter.uuid());
        if policy.activates_on_join {
            groups.active.insert(agent, id);
        }
    }
    sync(core, agent).await;
    let now = shared.now();
    shared
        .with_state(|state| {
            warn_on(
                state
                    .sim
                    .send_create_group_reply(id, true, policy.founding_text, now),
                "a founding",
            );
            if policy.founding_fee > 0 {
                state.sim.charge_money(policy.founding_fee);
                let fee = LindenAmount(u64::try_from(policy.founding_fee).unwrap_or(0));
                let paid = MoneyBalance {
                    agent_id: agent,
                    transaction_id: Uuid::nil(),
                    success: true,
                    balance: LindenAmount(
                        state
                            .sim
                            .money_balance()
                            .and_then(|balance| u64::try_from(balance).ok())
                            .unwrap_or(0),
                    ),
                    square_meters_credit: LandArea(0),
                    square_meters_committed: LandArea(0),
                    description: format!("You paid Governor Linden L${}.", policy.founding_fee),
                    transaction: Some(MoneyTransaction {
                        transaction_type: GROUP_CREATE_TRANSACTION,
                        source: OwnerKey::Agent(agent),
                        dest: OwnerKey::Agent(AgentKey::from(Uuid::nil())),
                        amount: fee,
                        item_description: String::new(),
                    }),
                };
                for _twice in 0..2 {
                    warn_on(
                        state.sim.send_money_balance_reply(&paid, now),
                        "a founding fee",
                    );
                }
            }
            if policy.activates_on_join {
                send_active(state, now);
                send_memberships(core, &mut state.sim, agent, now);
            }
        })
        .await;
}

/// Makes `agent` a member of `group` in `role` (besides the Everyone role);
/// answers whether it was not one already.
fn enrol(core: &GridCore, agent: AgentKey, group: GroupKey, role: Option<GroupRoleKey>) -> bool {
    let mut groups = core.groups.lock();
    let added = groups
        .groups
        .get(&group)
        .is_some_and(|entry| !entry.members.contains_key(&agent));
    let Some(member) = groups.add_member(group, agent) else {
        return false;
    };
    if let Some(role) = role {
        member.roles.insert(role);
    }
    groups.active.insert(agent, group);
    added
}

/// An avatar asked to join a group.
async fn join(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, group_id: GroupKey) {
    let policy = core.groups_policy;
    let (banned, open) = {
        let groups = core.groups.lock();
        groups
            .groups
            .get(&group_id)
            .map_or((false, false), |group| {
                (group.bans.contains_key(&agent), group.open_enrollment)
            })
    };
    let now = shared.now();
    if banned || !open {
        shared
            .with_sim(|sim| {
                if banned && policy.told_by_alerts {
                    alert(sim, "This group has banned you from joining.", now);
                }
                warn_on(sim.send_join_group_reply(group_id, false, now), "a join");
            })
            .await;
        return;
    }
    {
        let mut groups = core.groups.lock();
        let _member = groups.add_member(group_id, agent);
        if policy.activates_on_join {
            groups.active.insert(agent, group_id);
        }
    }
    sync(core, agent).await;
    shared
        .with_state(|state| {
            if policy.told_by_alerts {
                alert(&mut state.sim, ADDED, now);
            }
            warn_on(
                state.sim.send_join_group_reply(group_id, true, now),
                "a join",
            );
            if policy.activates_on_join {
                send_active(state, now);
                send_memberships(core, &mut state.sim, agent, now);
            }
        })
        .await;
}

/// Takes `agent` out of `group`: answers the group's name and how many
/// members are left, where it was a member.
fn take_out(core: &GridCore, agent: AgentKey, group: GroupKey) -> Option<(String, usize)> {
    let mut groups = core.groups.lock();
    let entry = groups.groups.get_mut(&group)?;
    entry.members.remove(&agent)?;
    let left = (entry.name.clone(), entry.members.len());
    if groups.active.get(&agent) == Some(&group) {
        groups.active.remove(&agent);
    }
    drop(groups);
    Some(left)
}

/// Tells `agent` it is out of `group`, as the grid tells a leaver or an
/// ejected member, after what `first` says.
fn tell_out(
    core: &GridCore,
    state: &mut SimState,
    agent: AgentKey,
    group: GroupKey,
    emptied: bool,
    now: Instant,
) {
    if core.groups_policy.told_by_alerts {
        // Second Life drops a group that is left empty without a word.
        if !emptied {
            state.sim.enqueue_agent_drop_group(group);
        }
        send_failed_balances(&mut state.sim, agent, now);
    } else {
        warn_on(state.sim.send_agent_drop_group(group, now), "a drop");
        send_active(state, now);
        send_memberships(core, &mut state.sim, agent, now);
    }
}

/// An avatar left a group.
async fn leave(core: &Arc<GridCore>, shared: &SharedSim, agent: AgentKey, group_id: GroupKey) {
    let policy = core.groups_policy;
    let left = take_out(core, agent, group_id);
    sync(core, agent).await;
    let now = shared.now();
    shared
        .with_state(|state| {
            let Some((name, members)) = left else {
                warn_on(
                    state.sim.send_leave_group_reply(group_id, false, now),
                    "a leave",
                );
                return;
            };
            if policy.told_by_alerts {
                if members == 0 {
                    alert(
                        &mut state.sim,
                        "0 users remain in group. Group will be disbanded by nightly cleanup.",
                        now,
                    );
                }
                alert(
                    &mut state.sim,
                    &format!("You have left the group '{name}'."),
                    now,
                );
            }
            warn_on(
                state.sim.send_leave_group_reply(group_id, true, now),
                "a leave",
            );
            tell_out(core, state, agent, group_id, members == 0, now);
        })
        .await;
}

/// An avatar ejected a member from a group.
async fn eject(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    agent: AgentKey,
    group_id: GroupKey,
    member: AgentKey,
) {
    let policy = core.groups_policy;
    let left = take_out(core, member, group_id);
    sync(core, member).await;
    let Some((name, members)) = left else {
        let now = shared.now();
        shared
            .with_sim(|sim| {
                warn_on(
                    sim.send_eject_group_member_reply(group_id, false, now),
                    "an ejection",
                );
            })
            .await;
        return;
    };
    let ejector = legacy_name(core, agent);
    let ejected = legacy_name(core, member);
    let here = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let message = |to: AgentKey, text: String, id: Uuid| {
        let mut im = message_from(&here, to, ImDialog::Message, &text);
        im.id = id;
        if policy.told_by_alerts {
            im.binary_bucket = vec![0];
        } else {
            im.region_id = Some(here.region_id);
            im.position = here.position;
        }
        im
    };
    let (told_ejector, told_member) = if policy.told_by_alerts {
        (
            message(
                agent,
                format!("{ejected} has  been ejected from '{name}' by {ejector}."),
                Uuid::nil(),
            ),
            message(
                member,
                format!("You have been ejected from '{name}' by {ejector}."),
                Uuid::nil(),
            ),
        )
    } else {
        (
            message(
                agent,
                format!("{ejected} has been ejected from '{name}' by {ejector}."),
                Uuid::nil(),
            ),
            message(
                member,
                format!("You have been ejected from '{name}' by {ejector}."),
                core.minter.uuid(),
            ),
        )
    };
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(sim.send_instant_message(&told_ejector, now), "an ejection");
            warn_on(
                sim.send_eject_group_member_reply(group_id, true, now),
                "an ejection",
            );
        })
        .await;
    tell(core, member, |state, now| {
        warn_on(
            state.sim.send_instant_message(&told_member, now),
            "an ejection",
        );
        if policy.told_by_alerts {
            send_failed_balances(&mut state.sim, member, now);
        } else {
            tell_out(core, state, member, group_id, members == 0, now);
        }
    })
    .await;
}

/// An avatar made a group its active one, or none.
async fn activate(core: &Arc<GridCore>, agent: AgentKey, group_id: Option<GroupKey>) {
    {
        let mut groups = core.groups.lock();
        match group_id {
            Some(group) => {
                groups.active.insert(agent, group);
            }
            None => {
                groups.active.remove(&agent);
            }
        }
    }
    tell_standing(core, agent).await;
}

/// An avatar edited a group's profile.
fn update_info(core: &GridCore, agent: AgentKey, params: &UpdateGroupInfoParams) {
    let mut groups = core.groups.lock();
    let Some(group) = groups.groups.get_mut(&params.group_id) else {
        return;
    };
    if !group.members.contains_key(&agent) {
        return;
    }
    group.charter.clone_from(&params.charter);
    group.show_in_list = params.show_in_list;
    group.insignia = params.insignia_id;
    group.fee = params.membership_fee.clone();
    group.open_enrollment = params.open_enrollment;
    group.allow_publish = params.allow_publish;
    group.mature_publish = params.mature_publish;
    drop(groups);
}

/// An avatar chose the title it wears in a group: it is sent its active
/// group again.
async fn wear(core: &Arc<GridCore>, agent: AgentKey, group_id: GroupKey, role: GroupRoleKey) {
    {
        let mut groups = core.groups.lock();
        if let Some(member) = groups
            .groups
            .get_mut(&group_id)
            .and_then(|group| group.members.get_mut(&agent))
        {
            let role = (!role.uuid().is_nil()).then_some(role);
            if role.is_none_or(|role| member.roles.contains(&role)) {
                member.worn = role;
            }
        }
    }
    sync(core, agent).await;
    tell(core, agent, send_active).await;
}

/// On a grid that tells of it, sends the editor of a group's roles its active
/// group.
async fn told_of_role_edit(core: &GridCore, agent: AgentKey) {
    sync(core, agent).await;
    if core.groups_policy.role_edits_told {
        tell(core, agent, send_active).await;
    }
}

/// An avatar created, changed or deleted roles of a group.
async fn edit_roles(
    core: &Arc<GridCore>,
    agent: AgentKey,
    group_id: GroupKey,
    edits: &[GroupRoleEdit],
) {
    let mut affected = Vec::new();
    {
        let mut groups = core.groups.lock();
        let Some(group) = groups.groups.get_mut(&group_id) else {
            return;
        };
        if !group.members.contains_key(&agent) {
            return;
        }
        for edit in edits {
            match edit.update_type {
                // The grid's own id, whatever the viewer chose.
                GroupRoleUpdateType::Create => group.roles.push(Role {
                    id: Some(GroupRoleKey::from(core.minter.uuid())),
                    name: edit.name.clone(),
                    title: edit.title.clone(),
                    description: edit.description.clone(),
                    powers: edit.powers,
                }),
                GroupRoleUpdateType::Delete => {
                    let Some(id) = edit.role_id.filter(|id| *id != group.owner_role) else {
                        continue;
                    };
                    group.roles.retain(|role| role.id != Some(id));
                    for (member_id, member) in &mut group.members {
                        if member.roles.remove(&id) {
                            affected.push(*member_id);
                        }
                        if member.worn == Some(id) {
                            member.worn = None;
                        }
                    }
                }
                GroupRoleUpdateType::NoUpdate => {}
                update => {
                    let Some(role) = group.roles.iter_mut().find(|role| role.id == edit.role_id)
                    else {
                        continue;
                    };
                    if matches!(
                        update,
                        GroupRoleUpdateType::UpdateData | GroupRoleUpdateType::UpdateAll
                    ) {
                        role.name.clone_from(&edit.name);
                        role.title.clone_from(&edit.title);
                        role.description.clone_from(&edit.description);
                    }
                    if matches!(
                        update,
                        GroupRoleUpdateType::UpdatePowers | GroupRoleUpdateType::UpdateAll
                    ) {
                        role.powers = edit.powers;
                    }
                    affected.extend(group.members.keys().copied());
                }
            }
        }
        drop(groups);
    }
    for member in affected {
        if member != agent {
            sync(core, member).await;
        }
    }
    told_of_role_edit(core, agent).await;
}

/// An avatar put members into roles of a group or took them out.
async fn change_role_members(
    core: &Arc<GridCore>,
    agent: AgentKey,
    group_id: GroupKey,
    changes: &[GroupRoleMemberChange],
) {
    let wears = core.groups_policy.assignment_wears;
    let mut affected = Vec::new();
    {
        let mut groups = core.groups.lock();
        let Some(group) = groups.groups.get_mut(&group_id) else {
            return;
        };
        for change in changes {
            let Some(role) = change.role_id else {
                continue;
            };
            if group.role(Some(role)).is_none() {
                continue;
            }
            let Some(member) = group.members.get_mut(&change.member_id) else {
                continue;
            };
            match change.change {
                GroupRoleChange::Add => {
                    member.roles.insert(role);
                    if wears {
                        member.worn = Some(role);
                    }
                }
                GroupRoleChange::Remove => {
                    member.roles.remove(&role);
                    if member.worn == Some(role) {
                        member.worn = None;
                    }
                }
            }
            affected.push(change.member_id);
        }
        drop(groups);
    }
    for member in affected {
        if member != agent {
            sync(core, member).await;
        }
    }
    told_of_role_edit(core, agent).await;
}

/// Answers a profile request.
async fn reply_profile(core: &GridCore, shared: &SharedSim, agent: AgentKey, group_id: GroupKey) {
    let policy = core.groups_policy;
    let profile = core.groups.lock().groups.get(&group_id).map(|group| {
        let (member_title, powers, member_count, role_count) = if policy.stale_profile {
            // Second Life's profile of any group, read by anybody.
            let everyone = group.everyone();
            (
                everyone.map_or_else(String::new, |role| role.title.clone()),
                everyone.map_or(0, |role| role.powers),
                1,
                2,
            )
        } else {
            (
                group.title(agent),
                group.powers(agent, &policy),
                i32::try_from(group.members.len()).unwrap_or(i32::MAX),
                i32::try_from(group.roles.len()).unwrap_or(i32::MAX),
            )
        };
        GroupProfile {
            group_id,
            name: group.name.clone(),
            charter: group.charter.clone(),
            show_in_list: group.show_in_list,
            member_title,
            powers,
            insignia_id: group.insignia,
            founder_id: group.founder,
            membership_fee: group.fee.clone(),
            open_enrollment: group.open_enrollment,
            money: 0,
            member_count,
            role_count,
            allow_publish: group.allow_publish,
            mature_publish: group.mature_publish,
            owner_role: group.owner_role,
        }
    });
    let Some(profile) = profile else {
        return;
    };
    let now = shared.now();
    shared
        .with_sim(|sim| warn_on(sim.send_group_profile_reply(&profile, now), "a profile"))
        .await;
}

/// The roster of `group_id` as both the UDP reply and `GroupMemberData` state
/// it: each member, its powers and title, whether it owns the group, and
/// whether it is online.
async fn roster(core: &GridCore, group_id: GroupKey) -> Vec<GroupMember> {
    let policy = core.groups_policy;
    let listed: Vec<(AgentKey, u64, String, bool)> = core
        .groups
        .lock()
        .groups
        .get(&group_id)
        .map_or_else(Vec::new, |group| {
            group
                .members
                .keys()
                .map(|member| {
                    (
                        *member,
                        group.powers(*member, &policy),
                        group.title(*member),
                        group.is_owner(*member),
                    )
                })
                .collect()
        });
    let mut members = Vec::with_capacity(listed.len());
    for (agent_id, agent_powers, title, is_owner) in listed {
        let online = core.root_session_of(agent_id).await.is_some();
        members.push(GroupMember {
            agent_id,
            contribution: LandArea(0),
            online_status: if online { "Online" } else { "" }.to_owned(),
            agent_powers,
            title,
            is_owner,
        });
    }
    members
}

/// The roster of `group_id`, for the `GroupMemberData` capability.
pub(crate) async fn roster_for_capability(core: &GridCore, group_id: GroupKey) -> Vec<GroupMember> {
    roster(core, group_id).await
}

/// Answers a members request: the roster, or on Second Life none — a count
/// of 0 and one block naming nobody.
async fn reply_members(core: &GridCore, shared: &SharedSim, group_id: GroupKey, request_id: Uuid) {
    let members = if core.groups_policy.roster_over_udp {
        roster(core, group_id).await
    } else {
        Vec::new()
    };
    let (count, blocks) = if members.is_empty() {
        (
            0,
            vec![GroupMember {
                agent_id: AgentKey::from(Uuid::nil()),
                contribution: LandArea(0),
                online_status: String::new(),
                agent_powers: 0,
                title: String::new(),
                is_owner: false,
            }],
        )
    } else {
        (i32::try_from(members.len()).unwrap_or(i32::MAX), members)
    };
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(
                sim.send_group_members_reply(group_id, request_id, count, &blocks, now),
                "the members",
            );
        })
        .await;
}

/// Answers a roles request: every role and how many hold it — or nothing, to
/// an outsider on a grid that keeps the roles to the members.
async fn reply_roles(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    group_id: GroupKey,
    request_id: Uuid,
) {
    let policy = core.groups_policy;
    let roles = core.groups.lock().groups.get(&group_id).and_then(|group| {
        if !policy.roles_to_outsiders && !group.members.contains_key(&agent) {
            return None;
        }
        Some(
            group
                .roles
                .iter()
                .map(|role| {
                    let holders = match role.id {
                        None if policy.everyone_pairs => group.members.len(),
                        None => 0,
                        Some(id) => group
                            .members
                            .values()
                            .filter(|member| member.roles.contains(&id))
                            .count(),
                    };
                    GroupRole {
                        role_id: role.id,
                        name: role.name.clone(),
                        title: role.title.clone(),
                        description: role.description.clone(),
                        powers: role.powers,
                        members: u32::try_from(holders).unwrap_or(u32::MAX),
                    }
                })
                .collect::<Vec<_>>(),
        )
    });
    let Some(roles) = roles else {
        return;
    };
    let count = i32::try_from(roles.len()).unwrap_or(i32::MAX);
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(
                sim.send_group_role_data_reply(group_id, request_id, count, &roles, now),
                "the roles",
            );
        })
        .await;
}

/// Answers a role-members request: who holds which role.
async fn reply_role_members(
    core: &GridCore,
    shared: &SharedSim,
    group_id: GroupKey,
    request_id: Uuid,
) {
    let everyone = core.groups_policy.everyone_pairs;
    let pairs: Vec<GroupRoleMember> =
        core.groups
            .lock()
            .groups
            .get(&group_id)
            .map_or_else(Vec::new, |group| {
                group
                    .members
                    .iter()
                    .flat_map(|(member_id, member)| {
                        let everyone = everyone.then_some(None);
                        everyone
                            .into_iter()
                            .chain(member.roles.iter().map(|role| Some(*role)))
                            .map(|role_id| GroupRoleMember {
                                role_id,
                                member_id: *member_id,
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            });
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(
                sim.send_group_role_members_reply(group_id, request_id, &pairs, now),
                "the role members",
            );
        })
        .await;
}

/// Answers a titles request: the title of each role `agent` holds, the one
/// it wears marked.
async fn reply_titles(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    group_id: GroupKey,
    request_id: Uuid,
) {
    let by_name = core.groups_policy.titles_by_role_name;
    let titles: Vec<GroupTitle> = core
        .groups
        .lock()
        .groups
        .get(&group_id)
        .and_then(|group| {
            let member = group.members.get(&agent)?;
            Some(
                group
                    .roles
                    .iter()
                    .filter(|role| role.id.is_none_or(|id| member.roles.contains(&id)))
                    .map(|role| GroupTitle {
                        title: if by_name {
                            role.name.clone()
                        } else {
                            role.title.clone()
                        },
                        role_id: role.id,
                        selected: role.id == member.worn,
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(
                sim.send_group_titles_reply(group_id, request_id, &titles, now),
                "the titles",
            );
        })
        .await;
}

/// Answers a notice-list request, the newest first.
async fn reply_notices(core: &GridCore, shared: &SharedSim, group_id: GroupKey) {
    let notices: Vec<GroupNotice> =
        core.groups
            .lock()
            .groups
            .get(&group_id)
            .map_or_else(Vec::new, |group| {
                group
                    .notices
                    .iter()
                    .rev()
                    .map(|notice| GroupNotice {
                        notice_id: GroupNoticeKey::from(notice.listed_id),
                        timestamp: notice.timestamp,
                        from_name: notice.from_name.clone(),
                        subject: notice.subject.clone(),
                        has_attachment: notice.attachment.is_some(),
                        asset_type: notice.attachment.as_ref().map_or(0, |(kind, _name)| *kind),
                    })
                    .collect()
            });
    let now = shared.now();
    shared
        .with_sim(|sim| {
            warn_on(
                sim.send_group_notices_list_reply(group_id, &notices, now),
                "the notices",
            );
        })
        .await;
}

/// The bucket a notice of `group` is sent with: whether it carries an item,
/// the item's asset type, the group, and the item's name.
fn notice_bucket(group: GroupKey, attachment: Option<&(u8, String)>) -> Vec<u8> {
    let (has, kind, name) = attachment.map_or((0, 0, ""), |(kind, name)| (1, *kind, name.as_str()));
    let mut bucket = vec![has, kind];
    bucket.extend_from_slice(group.uuid().as_bytes());
    bucket.extend_from_slice(name.as_bytes());
    bucket.push(0);
    bucket
}

/// The notice `notice` of `group` as `to` is sent it, under `id`.
fn notice_message(
    group: GroupKey,
    notice: &Notice,
    to: AgentKey,
    id: Uuid,
    dialog: ImDialog,
    from_name: &str,
    policy: &GroupsPolicy,
) -> InstantMessage {
    InstantMessage {
        from_agent_id: if policy.notices_from_group {
            AgentKey::from(group.uuid())
        } else {
            notice.from
        },
        from_agent_name: from_name.to_owned(),
        to_agent_id: to,
        dialog,
        from_group: true,
        region_id: None,
        position: sl_types::map::RegionCoordinates::new(0.0, 0.0, 0.0),
        offline: policy.notices_from_group,
        timestamp: Some(notice.timestamp),
        id,
        parent_estate_id: 1,
        message: notice.text.clone(),
        binary_bucket: notice_bucket(group, notice.attachment.as_ref()),
    }
}

/// The item a notice's bucket names: the reference sends
/// `<? LLSD/XML ?>\n` and a map of `item_id` and `owner_id`.
fn attached_item(bucket: &[u8]) -> Option<InventoryKey> {
    let text = std::str::from_utf8(bucket).ok()?;
    let body = text.trim_end_matches('\0');
    let body = body.strip_prefix("<? LLSD/XML ?>\n").unwrap_or(body);
    let llsd = sl_wire::parse_llsd_xml(body).ok()?;
    match llsd.get("item_id")? {
        sl_wire::Llsd::Uuid(id) => Some(InventoryKey::from(*id)),
        _ => None,
    }
}

/// An avatar posted a notice to a group: every member that takes notices is
/// sent it.
async fn post_notice(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    agent: AgentKey,
    im: &InstantMessage,
) {
    let policy = core.groups_policy;
    let group_id = GroupKey::from(im.to_agent_id.uuid());
    let attachment = match attached_item(&im.binary_bucket) {
        Some(item) => {
            let state = shared.state.lock().await;
            state
                .sim
                .agent_inventory()
                .item(item)
                .map(|item| (u8::try_from(item.item_type).unwrap_or(0), item.name.clone()))
        }
        None => None,
    };
    let id = core.minter.uuid();
    let notice = Notice {
        listed_id: if policy.notices_from_group {
            id
        } else {
            core.minter.uuid()
        },
        from: agent,
        from_name: legacy_name(core, agent),
        subject: im
            .message
            .split_once('|')
            .map_or(im.message.as_str(), |(subject, _message)| subject)
            .to_owned(),
        text: im.message.clone(),
        timestamp: now_epoch_seconds(),
        attachment,
    };
    let recipients: Vec<AgentKey> = {
        let mut groups = core.groups.lock();
        let Some(group) = groups.groups.get_mut(&group_id) else {
            return;
        };
        if !group.members.contains_key(&agent) {
            return;
        }
        group.notices.push(notice.clone());
        let recipients = group
            .members
            .iter()
            .filter(|(_member, member)| member.accept_notices)
            .map(|(member_id, _member)| *member_id)
            .collect();
        drop(groups);
        recipients
    };
    for to in recipients {
        // Second Life sends each recipient the notice under an id of its own.
        let id = if policy.notices_from_group {
            id
        } else {
            core.minter.uuid()
        };
        let message = notice_message(
            group_id,
            &notice,
            to,
            id,
            ImDialog::GroupNotice,
            &notice.from_name,
            &policy,
        );
        tell(core, to, |state, now| {
            warn_on(state.sim.send_instant_message(&message, now), "a notice");
        })
        .await;
    }
}

/// An avatar asked for a notice again, by the id it is listed under.
async fn notice_again(
    core: &GridCore,
    shared: &SharedSim,
    agent: AgentKey,
    notice_id: GroupNoticeKey,
) {
    let policy = core.groups_policy;
    let found = core.groups.lock().groups.values().find_map(|group| {
        group
            .notices
            .iter()
            .find(|notice| notice.listed_id == notice_id.uuid())
            .map(|notice| (group.id, notice.clone()))
    });
    let Some((group, notice)) = found else {
        return;
    };
    // Second Life sends it from its sender, named by an id, under yet another
    // id; OpenSim as it sent it first.
    let (id, from_name) = if policy.notices_from_group {
        (notice.listed_id, notice.from_name.clone())
    } else {
        (core.minter.uuid(), core.minter.uuid().to_string())
    };
    let message = notice_message(
        group,
        &notice,
        agent,
        id,
        ImDialog::GroupNoticeRequested,
        &from_name,
        &policy,
    );
    let now = shared.now();
    shared
        .with_sim(|sim| warn_on(sim.send_instant_message(&message, now), "a notice"))
        .await;
}

/// An avatar invited others into a group, each into a role.
async fn invite(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    agent: AgentKey,
    group_id: GroupKey,
    invitees: &[(AgentKey, Option<GroupRoleKey>)],
) {
    let policy = core.groups_policy;
    let Some((name, charter, fee)) = core
        .groups
        .lock()
        .groups
        .get(&group_id)
        .map(|group| (group.name.clone(), group.charter.clone(), group.fee.clone()))
    else {
        return;
    };
    let here = {
        let state = shared.state.lock().await;
        Whereabouts::of(&state)
    };
    let mut refused_a_banned = false;
    for (invitee, role) in invitees.iter().copied() {
        let (banned, member) = {
            let groups = core.groups.lock();
            groups
                .groups
                .get(&group_id)
                .map_or((false, false), |group| {
                    (
                        group.bans.contains_key(&invitee),
                        group.members.contains_key(&invitee),
                    )
                })
        };
        if banned {
            refused_a_banned = true;
            continue;
        }
        if member && !policy.invites_members {
            continue;
        }
        let id = core.minter.uuid();
        core.groups.lock().invitations.insert(
            id,
            Invitation {
                group: group_id,
                invitee,
                role,
            },
        );
        // The fee is a wire-defined big-endian S32 (`LLGroupMgr`'s bucket).
        #[expect(
            clippy::big_endian_bytes,
            reason = "an invitation's membership fee is a wire-defined big-endian S32"
        )]
        let mut bucket = i32::try_from(fee.0).unwrap_or(0).to_be_bytes().to_vec();
        bucket.extend_from_slice(role.map_or_else(Uuid::nil, |role| role.uuid()).as_bytes());
        let inviter = legacy_name(core, agent);
        let (from_name, text, region_id) = match policy.invitation {
            InvitationForm::SecondLife => {
                let user = user_name(core, agent);
                (
                    user.clone(),
                    format!(
                        "{inviter} ({user}) has invited you to join a group.\nThere is no cost to \
                         join this group.\nGroup:\n{name}: {charter}\n\n"
                    ),
                    None,
                )
            }
            InvitationForm::OpenSim => (
                inviter.clone(),
                format!(
                    "{inviter} has invited you to join a group called {name}. There is no cost \
                     to join this group."
                ),
                Some(here.region_id),
            ),
        };
        let message = InstantMessage {
            from_agent_id: AgentKey::from(group_id.uuid()),
            from_agent_name: from_name,
            to_agent_id: invitee,
            dialog: ImDialog::GroupInvitation,
            from_group: true,
            region_id,
            position: sl_types::map::RegionCoordinates::new(0.0, 0.0, 0.0),
            offline: false,
            timestamp: None,
            id,
            parent_estate_id: 1,
            message: text,
            binary_bucket: bucket,
        };
        let delivered = tell(core, invitee, |state, now| {
            warn_on(
                state.sim.send_instant_message(&message, now),
                "an invitation",
            );
        })
        .await;
        if !delivered {
            // Kept for the invitee: its record, on Second Life, is the
            // inviter's message about the group, from no region.
            let message = InstantMessage {
                offline: true,
                timestamp: Some(now_epoch_seconds()),
                ..message
            };
            let record = InstantMessage {
                from_agent_id: agent,
                from_agent_name: inviter,
                from_group: false,
                id: group_id.uuid(),
                region_id: None,
                ..message.clone()
            };
            core.social
                .lock()
                .keep(invitee, KeptMessage { message, record });
        }
    }
    if refused_a_banned && policy.told_by_alerts {
        let now = shared.now();
        shared
            .with_sim(|sim| {
                alert(
                    sim,
                    "Some residents have not been sent an invite due to being banned from the \
                     group.",
                    now,
                );
            })
            .await;
    }
}

/// Takes the invitation sent under `id` to `invitee`, if there is one.
fn take_invitation(core: &GridCore, id: Uuid, invitee: AgentKey) -> Option<Invitation> {
    let mut groups = core.groups.lock();
    let invitation = groups
        .invitations
        .get(&id)
        .copied()
        .filter(|invitation| invitation.invitee == invitee)?;
    groups.invitations.remove(&id);
    drop(groups);
    Some(invitation)
}

/// An invitation was answered: a decline tells nobody; an accept makes the
/// invitee a member, its active group the group.
async fn answered(core: &Arc<GridCore>, shared: &SharedSim, invitation: Invitation, accept: bool) {
    if !accept {
        return;
    }
    let agent = invitation.invitee;
    let group_id = invitation.group;
    enrol(core, agent, group_id, invitation.role);
    sync(core, agent).await;
    let policy = core.groups_policy;
    let now = shared.now();
    shared
        .with_state(|state| {
            if policy.told_by_alerts {
                alert(&mut state.sim, ADDED, now);
                warn_on(
                    state.sim.send_join_group_reply(group_id, true, now),
                    "a join",
                );
            } else {
                let added = InstantMessage {
                    from_agent_id: AgentKey::from(Uuid::nil()),
                    from_agent_name: "Groups".to_owned(),
                    to_agent_id: agent,
                    dialog: ImDialog::MessageBox,
                    from_group: false,
                    region_id: None,
                    position: sl_types::map::RegionCoordinates::new(0.0, 0.0, 0.0),
                    offline: false,
                    timestamp: None,
                    id: agent.uuid(),
                    parent_estate_id: 0,
                    message: ADDED.to_owned(),
                    binary_bucket: Vec::new(),
                };
                warn_on(
                    state.sim.send_instant_message(&added, now),
                    "an accepted invitation",
                );
            }
            send_active(state, now);
            send_memberships(core, &mut state.sim, agent, now);
        })
        .await;
}

/// An invitation kept for the avatar was answered through a capability,
/// which names its group: it is accepted or forgotten, and nothing of it is
/// kept for the avatar any more.
async fn kept_answered(
    core: &Arc<GridCore>,
    shared: &SharedSim,
    agent: AgentKey,
    group_id: GroupKey,
    accept: bool,
) {
    let invitation = {
        let mut groups = core.groups.lock();
        let ids: Vec<Uuid> = groups
            .invitations
            .iter()
            .filter(|(_id, invitation)| invitation.group == group_id && invitation.invitee == agent)
            .map(|(id, _invitation)| *id)
            .collect();
        let mut found = None;
        for id in ids {
            found = groups.invitations.remove(&id).or(found);
        }
        found
    };
    shared
        .with_sim(|sim| {
            for kept in sim.take_offline_messages() {
                let invited = kept.message.dialog == ImDialog::GroupInvitation
                    && kept.message.from_agent_id.uuid() == group_id.uuid();
                if !invited {
                    sim.store_offline_message(kept);
                }
            }
        })
        .await;
    if let Some(invitation) = invitation {
        answered(core, shared, invitation, accept).await;
    }
}

/// The bans of `group_id`.
pub(crate) fn bans_of(core: &GridCore, group_id: GroupKey) -> Vec<GroupBan> {
    core.groups
        .lock()
        .groups
        .get(&group_id)
        .map_or_else(Vec::new, |group| {
            group
                .bans
                .iter()
                .map(|(agent_id, date)| GroupBan {
                    agent_id: *agent_id,
                    ban_date: Some(date.clone()),
                })
                .collect()
        })
}

/// Bans `agents` from `group_id`, or lifts their bans; told to nobody.
pub(crate) fn change_bans(
    core: &GridCore,
    group_id: GroupKey,
    action: GroupBanAction,
    agents: &[AgentKey],
) {
    let mut groups = core.groups.lock();
    let Some(group) = groups.groups.get_mut(&group_id) else {
        return;
    };
    let date = ban_date();
    for agent in agents {
        match action {
            GroupBanAction::Ban => {
                group.bans.insert(*agent, date.clone());
            }
            GroupBanAction::Unban => {
                group.bans.remove(agent);
            }
        }
    }
    drop(groups);
}
