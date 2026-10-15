//! A census of groups: what founding one costs and brings, what a member, a
//! leaver and an outsider are told and may read, what roles, titles, notices
//! and invitations bring both avatars, what an ejection and a ban do, what the
//! grid keeps of an invitation for an avatar that is away, and what a deed by
//! an owner makes of an object.
//!
//! Avatar **A** owns the census group and does everything an owner does;
//! avatar **B** is the member it acts on. One question a leg:
//!
//! 1. **A deed by the owner**: A rezzes a cube, sets it to the group and
//!    deeds it; who may then move it, and who may take it away.
//! 2. **Founding**: A founds a fresh group — what it is told, what it is
//!    charged, and what the new group's profile, roles, members and titles
//!    read.
//! 3. **Leaving**: B leaves the census group, then reads its profile, roles
//!    and members as an outsider; A edits the charter and B reads it again.
//! 4. **Joining**: B joins; A reads the members over UDP and over
//!    `GroupMemberData`; B reads the profile and its titles as a member; B
//!    makes the group its active one and drops it again.
//! 5. **Roles**: A makes a role, puts B in it, B wears its title, A takes B
//!    out and deletes the role.
//! 6. **Notices**: A posts a notice; B asks for it again; B stops taking
//!    notices and A posts another; A posts one with a notecard.
//! 7. **Accounts and proposals**: A asks for the accounts and the proposals.
//! 8. **Ejection**: A ejects B.
//! 9. **Invitations**: A invites B, B declines; A invites again, B accepts; A
//!    invites B, who is a member.
//! 10. **Bans** over `GroupAPIv1`: A bans B, B tries to join, A unbans B.
//! 11. **Away**, three times: B logs out and A invites it; B comes back, and
//!     answers the kept invitation: read and accepted over UDP; read and
//!     accepted as the viewer does ([`Command::RequestOfflineMessages`], then
//!     the accept the client makes of what it read); and by reading
//!     `ReadOfflineMsgs` and declining through `DeclineGroupInvite` itself.
//! 12. **Leaving a group one founded**, and founding one with a name of 36
//!     characters.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/groups.md`; every run is held to it.
//!
//! `2av`, `[both]`. The census measures in the fixtures' `census_group`, and
//! founds one where none is named (the run then measures in that one, and
//! keeps it). B is left a member of the census group, also when a leg fails:
//! Second Life disbands a young group of fewer than two members. The fake
//! grid holds a census group of its own from its start, owned by A with B a
//! member, and imitates each grid's groups.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    ActiveGroup, AgentKey, AssetType, Command, CreateGroupParams, DeRezDestination, Event,
    GroupKey, GroupMember, GroupMembership, GroupNotice, GroupNoticeAttachment, GroupNoticeKey,
    GroupProfile, GroupRequestId, GroupRole, GroupRoleChange, GroupRoleEdit, GroupRoleKey,
    GroupRoleMember, GroupRoleMemberChange, GroupRoleUpdateType, GroupTitle, ImDialog,
    InstantMessage, InventoryItem, InventoryType, LindenAmount, MoneyBalance, Object, OwnerKey,
    PermissionField, Permissions, TransactionId, UpdateGroupInfoParams, Uuid, group_powers,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::friends_presence::{answer_text, kept_records};
use crate::cases::object_edit::{lifted, moved, near, record_masks, record_of, reread};
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up, joined,
    listen, rez_cube,
};
use crate::cases::task_inventory::{
    DRAIN, OPENSIM_START, cap_get, cap_post, create_item, hold_to_measured, http_client,
    login_folders, shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, secs_metric, settle_scene};

/// What every text of the census begins with.
const TEXT_TAG: &str = "gr-";

/// How long the acting avatar listens after a gesture.
const LISTEN: Duration = Duration::from_secs(6);

/// How long the acting avatar waits for what a leg came for, at most.
const ECHO_WINDOW: Duration = Duration::from_secs(20);

/// How long a grid is given to act on a change before the next leg.
const SETTLE: Duration = Duration::from_secs(3);

/// How long a founding is waited for.
const CREATE_WINDOW: Duration = Duration::from_secs(30);

/// How long a grid is given to drop an avatar that logged out, so that an
/// invitation is kept for it rather than sent after a closing session.
const AWAY_SETTLE: Duration = Duration::from_secs(8);

/// How long an avatar that came back is left alone, to see what it is sent
/// without asking.
const UNASKED_WINDOW: Duration = Duration::from_secs(25);

/// How long an avatar waits for what it asked for.
const FETCH_WINDOW: Duration = Duration::from_secs(20);

/// How long after the first kept message a fetch is still listened to.
const FETCH_QUIET: Duration = Duration::from_secs(5);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// How long B waits before the leg that founds a second group: Second Life
/// drops a founding that follows another too closely.
const SECOND_FOUNDING_PAUSE: Duration = Duration::from_secs(30);

/// How long after a notice its group's list is read: Second Life lists a
/// notice some seconds after it is posted.
const NOTICE_LIST_PAUSE: Duration = Duration::from_secs(10);

/// The capability that serves what was kept for an avatar.
const READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// The capability that accepts an invitation that was kept.
const ACCEPT_GROUP_INVITE: &str = "AcceptGroupInvite";

/// The capability that declines an invitation that was kept.
const DECLINE_GROUP_INVITE: &str = "DeclineGroupInvite";

/// The capability that reads a group's members.
const GROUP_MEMBER_DATA: &str = "GroupMemberData";

/// The capability that reads and writes a group's bans.
const GROUP_API_V1: &str = "GroupAPIv1";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// A name of 36 characters: one more than Second Life takes.
const LONG_NAME_LENGTH: usize = 36;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/groups.md (groups, 2026-10-14)";

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
///
/// Not held: the timings, which group the census measured in, and whether B
/// was a member of it before the run.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "accounts_details",
        each(
            Answer::Text("details: 7 days, interval 0, started a date, 0 entries"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "accounts_summary",
        each(
            Answer::Text(
                "summary: 7 days, interval 0, started a date, balance 0 L$, credits 0, debits 0, 2 members taxed, last tax a date, next tax a date",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "accounts_transactions",
        each(
            Answer::Text("transactions: 7 days, interval 0, started a date, 0 entries"),
            Answer::Text("nothing"),
        ),
    ),
    ("activate_told_a", both(Answer::Text("nothing"))),
    (
        "activate_told_b",
        each(
            Answer::Text(
                "active group the group, the group's name, title \"Member\", powers 0x80018010000 | memberships: the group (the group's name): powers 0x80018010000, notices taken, contribution 0, insignia none",
            ),
            Answer::Text(
                "active group the group, the group's name, title \"Member of <the group's name>\", powers 0x390018010000 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    (
        "ban_answer",
        each(
            Answer::Text("201, \"Created\""),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ban_list",
        each(
            Answer::Text("200, {ban_list=[avatar B {ban_date:date}], group_id=string}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ban_list_after",
        each(
            Answer::Text("200, {ban_list=[], group_id=string}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ban_list_before",
        each(
            Answer::Text("200, {ban_list=[], group_id=string}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "ban_told_a",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "ban_told_b",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "cap_decline_answer",
        each(
            Answer::Text("200, {success=true}"),
            Answer::Text("no capability"),
        ),
    ),
    ("cap_decline_told_a", both(Answer::Text("nothing"))),
    ("cap_decline_told_b", both(Answer::Text("nothing"))),
    (
        "cap_invite_while_away_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "cap_kept_after_the_decline",
        each(Answer::Text("0 messages"), Answer::Text("no capability")),
    ),
    (
        "cap_kept_invitation",
        each(
            Answer::Text(
                "{asset_id=the group, binary_bucket={a bucket of 20 bytes: fee 0, role nobody}, dialog=3, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=\"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=nobody, timestamp=a time, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("cap_return_told_a", both(Answer::Text("nothing"))),
    (
        "cap_return_unasked_b",
        both(Answer::Text(
            "active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
        )),
    ),
    ("charter_told_a", both(Answer::Text("nothing"))),
    ("charter_told_b", both(Answer::Text("nothing"))),
    ("client_accept_made_a_member", both(Answer::Flag(true))),
    ("client_accept_told_a", both(Answer::Text("nothing"))),
    (
        "client_accept_told_b",
        each(
            Answer::Text(
                "alert \"You have been added to the group.\" | join of the group taken | active group the group, the group's name, title \"Member\", powers 0x80018010000 | memberships: the group (the group's name): powers 0x80018010000, notices taken, contribution 0, insignia none",
            ),
            Answer::Text(
                "MessageBox from nobody, the name \"Groups\", to avatar B, id avatar B, no region, not kept, text \"You have been added to the group.\", no bucket | active group the group, the group's name, title \"Member of <the group's name>\", powers 0x390018010000 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    (
        "client_invite_while_away_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "client_kept_invitation",
        each(
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id nobody, no region, kept, text \"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", a bucket of 20 bytes: fee 0, role nobody",
            ),
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, a region, kept, text \"<A> has invited you to join a group called <the group's name>. There is no cost to join this group.\", a bucket of 20 bytes: fee 0, role nobody",
            ),
        ),
    ),
    ("client_return_told_a", both(Answer::Text("nothing"))),
    (
        "client_return_unasked_b",
        both(Answer::Text(
            "active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
        )),
    ),
    ("deactivate_told_a", both(Answer::Text("nothing"))),
    (
        "deactivate_told_b",
        each(
            Answer::Text(
                "active group none, no name, title \"\", powers 0x0 | memberships: the group (the group's name): powers 0x80018010000, notices taken, contribution 0, insignia none",
            ),
            Answer::Text(
                "active group none, no name, title \"\", powers 0x0 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    ("deed_delete_alerts", both(Answer::Text("none"))),
    (
        "deed_delete_items",
        each(
            Answer::Text("item of type 6 in trash, owned by avatar A"),
            Answer::Text("item of type 6 in lost and found, owned by avatar A"),
        ),
    ),
    ("deed_delete_took", both(Answer::Flag(true))),
    (
        "deed_member_move_alerts",
        each(
            Answer::Text("Can't reposition -- permission denied"),
            Answer::Text("none"),
        ),
    ),
    ("deed_member_move_took", both(Answer::Flag(false))),
    ("deed_owner_move_alerts", both(Answer::Text("none"))),
    ("deed_owner_move_took", both(Answer::Flag(true))),
    ("deed_set_group_alerts", both(Answer::Text("none"))),
    ("deed_set_group_reads", both(Answer::Text("the group"))),
    (
        "deed_shared_alerts",
        each(
            Answer::Text("none"),
            Answer::Text("deeded unshared already"),
        ),
    ),
    (
        "deed_shared_answered_with",
        each(
            Answer::Text("full update+properties"),
            Answer::Text("deeded unshared already"),
        ),
    ),
    (
        "deed_shared_group_reads",
        each(
            Answer::Text("the group"),
            Answer::Text("deeded unshared already"),
        ),
    ),
    (
        "deed_shared_last_owner_reads",
        each(
            Answer::Text("avatar A"),
            Answer::Text("deeded unshared already"),
        ),
    ),
    (
        "deed_shared_masks",
        each(
            Answer::Text(
                "base 0x00082000 owner 0x00082000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
            Answer::Text("deeded unshared already"),
        ),
    ),
    (
        "deed_shared_owner_reads",
        each(
            Answer::Text("the group"),
            Answer::Text("deeded unshared already"),
        ),
    ),
    ("deed_unshared_alerts", both(Answer::Text("none"))),
    (
        "deed_unshared_answered_with",
        both(Answer::Text("full update+properties")),
    ),
    ("deed_unshared_group_reads", both(Answer::Text("the group"))),
    (
        "deed_unshared_last_owner_reads",
        both(Answer::Text("avatar A")),
    ),
    (
        "deed_unshared_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
            Answer::Text(
                "base 0x00092000 owner 0x00082000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    (
        "deed_unshared_owner_reads",
        each(Answer::Text("avatar A"), Answer::Text("the group")),
    ),
    ("eject_again_later_told_b", both(Answer::Text("nothing"))),
    (
        "eject_again_told_a",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id nobody, no region, not kept, text \"<B> has  been ejected from '<the group's name>' by <A>.\", a bucket of one zero byte | ejection of the group taken",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id nobody, a region, not kept, text \"<B> has been ejected from '<the group's name>' by <A>.\", no bucket | ejection of the group taken",
            ),
        ),
    ),
    (
        "eject_again_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id nobody, no region, not kept, text \"You have been ejected from '<the group's name>' by <A>.\", a bucket of one zero byte | balance (failed), description \"\", no transaction | balance (failed), description \"\", no transaction",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id another id, a region, not kept, text \"You have been ejected from '<the group's name>' by <A>.\", no bucket | dropped from the group (AgentDropGroup) | active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
            ),
        ),
    ),
    ("eject_later_told_b", both(Answer::Text("nothing"))),
    (
        "eject_told_a",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id nobody, no region, not kept, text \"<B> has  been ejected from '<the group's name>' by <A>.\", a bucket of one zero byte | ejection of the group taken",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id nobody, a region, not kept, text \"<B> has been ejected from '<the group's name>' by <A>.\", no bucket | ejection of the group taken",
            ),
        ),
    ),
    (
        "eject_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id nobody, no region, not kept, text \"You have been ejected from '<the group's name>' by <A>.\", a bucket of one zero byte | balance (failed), description \"\", no transaction | balance (failed), description \"\", no transaction",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id another id, a region, not kept, text \"You have been ejected from '<the group's name>' by <A>.\", no bucket | dropped from the group (AgentDropGroup) | active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
            ),
        ),
    ),
    ("found_charged", each(Answer::Count(100), Answer::Count(0))),
    ("found_later_told_a", both(Answer::Text("nothing"))),
    (
        "found_long_charged",
        each(Answer::Count(100), Answer::Count(0)),
    ),
    ("found_long_later_told_a", both(Answer::Text("nothing"))),
    (
        "found_long_told_a",
        each(
            Answer::Text(
                "founding taken: the long-named group, \"\" | balance, description \"You paid Governor Linden L$100.\", transaction of type 1002 for 100 | balance, description \"You paid Governor Linden L$100.\", transaction of type 1002 for 100",
            ),
            Answer::Text(
                "founding taken: the long-named group, \"Group created successfully\" | active group the long-named group, the long-named group's name, title \"Owner of <the long-named group's name>\", powers 0xffffffffffffe | memberships: the long-named group (the long-named group's name): powers 0xffffffffffffe, notices taken, contribution 0, insignia none; the group (the group's name): powers 0xffffffffffffe, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    ("found_long_told_b", both(Answer::Text("nothing"))),
    (
        "found_told_a",
        each(
            Answer::Text(
                "founding taken: the fresh group, \"\" | balance, description \"You paid Governor Linden L$100.\", transaction of type 1002 for 100 | balance, description \"You paid Governor Linden L$100.\", transaction of type 1002 for 100",
            ),
            Answer::Text(
                "founding taken: the fresh group, \"Group created successfully\" | active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe | memberships: the fresh group (the fresh group's name): powers 0xffffffffffffe, notices taken, contribution 0, insignia none; the group (the group's name): powers 0xffffffffffffe, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    ("found_told_b", both(Answer::Text("nothing"))),
    (
        "founder_leaves_told_a",
        each(
            Answer::Text(
                "alert \"0 users remain in group. Group will be disbanded by nightly cleanup.\" | alert \"You have left the group '<the fresh group's name>'.\" | leave of the fresh group taken | balance (failed), description \"\", no transaction | balance (failed), description \"\", no transaction",
            ),
            Answer::Text(
                "leave of the fresh group taken | dropped from the fresh group (AgentDropGroup) | active group none, no name, title \"\", powers 0x0 | memberships: the group (the group's name): powers 0xffffffffffffe, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    ("founder_leaves_told_b", both(Answer::Text("nothing"))),
    (
        "fresh_members",
        each(
            Answer::Text("members (said 0): "),
            Answer::Text(
                "members (said 1): avatar A: powers 0xffffffffffffe, title \"Owner of <the fresh group's name>\", an owner, status \"Online\", contribution 0",
            ),
        ),
    ),
    (
        "fresh_profile",
        each(
            Answer::Text(
                "profile: the fresh group, the fresh group's name, charter as sent, not listed, our title \"Member\", our powers 0x80018010000, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 2 roles, not published, not mature, owners' role another id",
            ),
            Answer::Text(
                "profile: the fresh group, the fresh group's name, charter as sent, not listed, our title \"Owner of <the fresh group's name>\", our powers 0xffffffffffffe, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 3 roles, not published, not mature, owners' role another id",
            ),
        ),
    ),
    (
        "fresh_role_members",
        each(
            Answer::Text("role members: avatar A in a role the grid made"),
            Answer::Text(
                "role members: avatar A in a role the grid made; avatar A in the Everyone role",
            ),
        ),
    ),
    (
        "fresh_roles",
        each(
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member\", description \"Everyone in the group is in the everyone role.\", powers 0x80018010000, 0 members; \"Officers\" (a role the grid made): title \"Officer\", description \"The officers of the group, with more powers than regular members.\", powers 0x1ccfffffff8ce, 0 members; \"Owners\" (a role the grid made): title \"Owner\", description \"These are the owners of the group.  They always have FULL POWER over the group.\", powers 0xffffffffffffffff, 1 members",
            ),
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member of <the fresh group's name>\", description \"Everyone in the group is in the everyone role.\", powers 0x390018010000, 1 members; \"Officers\" (a role the grid made): title \"Officer of <the fresh group's name>\", description \"The officers of the group, with more powers than regular members.\", powers 0x1fdffff7ff8ce, 0 members; \"Owners\" (a role the grid made): title \"Owner of <the fresh group's name>\", description \"Owners of the group\", powers 0xffffffffffffe, 1 members",
            ),
        ),
    ),
    (
        "fresh_titles",
        each(
            Answer::Text(
                "titles: \"Member\" of the Everyone role; \"Owner\" of a role the grid made (worn)",
            ),
            Answer::Text(
                "titles: \"Everyone\" of the Everyone role; \"Owners\" of a role the grid made (worn)",
            ),
        ),
    ),
    (
        "group_capabilities_granted",
        each(
            Answer::Text(
                "ReadOfflineMsgs, AcceptGroupInvite, DeclineGroupInvite, GroupMemberData, GroupAPIv1",
            ),
            Answer::Text("GroupMemberData"),
        ),
    ),
    ("invite_a_member_told_a", both(Answer::Text("nothing"))),
    (
        "invite_a_member_told_b",
        each(
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id another id, no region, not kept, text \"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", a bucket of 20 bytes: fee 0, role nobody",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "invite_accepted_answer_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "invite_accepted_answer_told_b",
        each(
            Answer::Text(
                "alert \"You have been added to the group.\" | join of the group taken | active group the group, the group's name, title \"Member\", powers 0x80018010000 | memberships: the group (the group's name): powers 0x80018010000, notices taken, contribution 0, insignia none",
            ),
            Answer::Text(
                "MessageBox from nobody, the name \"Groups\", to avatar B, id avatar B, no region, not kept, text \"You have been added to the group.\", no bucket | active group the group, the group's name, title \"Member of <the group's name>\", powers 0x390018010000 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    ("invite_accepted_told_a", both(Answer::Text("nothing"))),
    (
        "invite_accepted_told_b",
        each(
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, no region, not kept, text \"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", a bucket of 20 bytes: fee 0, role nobody",
            ),
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, a region, not kept, text \"<A> has invited you to join a group called <the group's name>. There is no cost to join this group.\", a bucket of 20 bytes: fee 0, role nobody",
            ),
        ),
    ),
    (
        "invite_declined_answer_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "invite_declined_answer_told_b",
        both(Answer::Text("nothing")),
    ),
    ("invite_declined_told_a", both(Answer::Text("nothing"))),
    (
        "invite_declined_told_b",
        each(
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, no region, not kept, text \"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", a bucket of 20 bytes: fee 0, role nobody",
            ),
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, a region, not kept, text \"<A> has invited you to join a group called <the group's name>. There is no cost to join this group.\", a bucket of 20 bytes: fee 0, role nobody",
            ),
        ),
    ),
    (
        "invite_while_banned_told_a",
        each(
            Answer::Text(
                "alert \"Some residents have not been sent an invite due to being banned from the group.\"",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "invite_while_banned_told_b",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    ("join_told_a", both(Answer::Text("nothing"))),
    (
        "join_told_b",
        each(
            Answer::Text("alert \"You have been added to the group.\" | join of the group taken"),
            Answer::Text(
                "join of the group taken | active group the group, the group's name, title \"Member of <the group's name>\", powers 0x390018010000 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    (
        "join_while_banned_told_a",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "join_while_banned_told_b",
        each(
            Answer::Text(
                "alert \"This group has banned you from joining.\" | join of the group refused",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("leave_told_a", both(Answer::Text("nothing"))),
    (
        "leave_told_b",
        each(
            Answer::Text(
                "alert \"You have left the group '<the group's name>'.\" | leave of the group taken | dropped from the group (AgentDropGroup over the event queue) | balance (failed), description \"\", no transaction | balance (failed), description \"\", no transaction",
            ),
            Answer::Text(
                "leave of the group taken | dropped from the group (AgentDropGroup) | active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
            ),
        ),
    ),
    (
        "long_profile",
        each(
            Answer::Text(
                "profile: the long-named group, the long-named group cut to 35's name, charter as sent, not listed, our title \"Member\", our powers 0x80018010000, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 2 roles, not published, not mature, owners' role another id",
            ),
            Answer::Text(
                "profile: the long-named group, the long-named group's name, charter as sent, not listed, our title \"Owner of <the long-named group's name>\", our powers 0xffffffffffffe, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 3 roles, not published, not mature, owners' role another id",
            ),
        ),
    ),
    (
        "member_profile",
        each(
            Answer::Text(
                "profile: the group, the group's name, charter \"gr-2-charter\", not listed, our title \"Member\", our powers 0x80018010000, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 2 roles, not published, not mature, owners' role the owners' role",
            ),
            Answer::Text(
                "profile: the group, the group's name, charter \"gr-2-charter\", not listed, our title \"Member of <the group's name>\", our powers 0x390018010000, insignia none, founded by avatar A, fee 0, open, money 0, 2 members, 3 roles, not published, not mature, owners' role the owners' role",
            ),
        ),
    ),
    (
        "member_roles",
        each(
            Answer::Text(
                "roles: \"Everyone\" (the Everyone role): title \"Member\", description \"Everyone in the group is in the everyone role.\", powers 0x80018010000, 0 members; \"Officers\" (another id): title \"Officer\", description \"The officers of the group, with more powers than regular members.\", powers 0x1ccfffffff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner\", description \"These are the owners of the group.  They always have FULL POWER over the group.\", powers 0xffffffffffffffff, 1 members",
            ),
            Answer::Text(
                "roles: \"Everyone\" (the Everyone role): title \"Member of <the group's name>\", description \"Everyone in the group is in the everyone role.\", powers 0x390018010000, 2 members; \"Officers\" (another id): title \"Officer of <the group's name>\", description \"The officers of the group, with more powers than regular members.\", powers 0x1fdffff7ff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner of <the group's name>\", description \"Owners of the group\", powers 0xffffffffffffe, 1 members",
            ),
        ),
    ),
    (
        "member_titles",
        each(
            Answer::Text("titles: \"Member\" of the Everyone role (worn)"),
            Answer::Text("titles: \"Everyone\" of the Everyone role (worn)"),
        ),
    ),
    (
        "members_capability",
        each(
            Answer::Text(
                "members (said 2): avatar A: powers 0xffffffffffffffff, title \"Owner\", an owner, status \"Online\", contribution 0; avatar B: powers 0x80018010000, title \"Member\", not an owner, status \"Online\", contribution 0",
            ),
            Answer::Text(
                "members (said 2): avatar A: powers 0xffffffffffffe, title \"Owner of <the group's name>\", an owner, status \"Online\", contribution 0; avatar B: powers 0x390018010000, title \"Member of <the group's name>\", not an owner, status \"Online\", contribution 0",
            ),
        ),
    ),
    ("members_capability_granted", both(Answer::Flag(true))),
    (
        "members_udp",
        each(
            Answer::Text("members (said 0): "),
            Answer::Text(
                "members (said 2): avatar A: powers 0xffffffffffffe, title \"Owner of <the group's name>\", an owner, status \"Online\", contribution 0; avatar B: powers 0x390018010000, title \"Member of <the group's name>\", not an owner, status \"Online\", contribution 0",
            ),
        ),
    ),
    (
        "members_with_the_title",
        each(
            Answer::Text("members (said 0): "),
            Answer::Text(
                "members (said 2): avatar A: powers 0xffffffffffffe, title \"Owner of <the group's name>\", an owner, status \"Online\", contribution 0; avatar B: powers 0x40000000002, title \"gr-4-title\", not an owner, status \"Online\", contribution 0",
            ),
        ),
    ),
    ("notice_asked_again_told_a", both(Answer::Text("nothing"))),
    (
        "notice_asked_again_told_b",
        each(
            Answer::Text(
                "GroupNoticeRequested from avatar A (a group), the name \"<id>\", to avatar B, id another id, no region, not kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
            Answer::Text(
                "GroupNoticeRequested from the group (a group), A's name, to avatar B, id the notice's id, no region, kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
        ),
    ),
    (
        "notice_list",
        each(
            Answer::Text(
                "notice list: notice the listed notice's id from A's name, subject as sent, no attachment, a time",
            ),
            Answer::Text(
                "notice list: notice the notice's id from A's name, subject as sent, no attachment, a time",
            ),
        ),
    ),
    (
        "notice_list_with_the_notecard",
        each(
            Answer::Text(
                "notice list: notice the listed notice with the notecard's id from A's name, subject as sent, an attachment of type 7, a time; notice the listed notice's id from A's name, subject \"gr-5-notice\", no attachment, a time",
            ),
            Answer::Text(
                "notice list: notice the notice with the notecard's id from A's name, subject as sent, an attachment of type 7, a time; notice the notice's id from A's name, subject \"gr-5-notice\", no attachment, a time",
            ),
        ),
    ),
    (
        "notice_told_a",
        each(
            Answer::Text(
                "GroupNotice from avatar A (a group), A's name, to avatar A, id another id, no region, not kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
            Answer::Text(
                "GroupNotice from the group (a group), A's name, to avatar A, id the notice's id, no region, kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
        ),
    ),
    (
        "notice_told_b",
        each(
            Answer::Text(
                "GroupNotice from avatar A (a group), A's name, to avatar B, id the notice's id, no region, not kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
            Answer::Text(
                "GroupNotice from the group (a group), A's name, to avatar B, id the notice's id, no region, kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
        ),
    ),
    (
        "notice_while_refused_told_a",
        each(
            Answer::Text(
                "GroupNotice from avatar A (a group), A's name, to avatar A, id another id, no region, not kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
            Answer::Text(
                "GroupNotice from the group (a group), A's name, to avatar A, id another id, no region, kept, text as sent, a bucket of 19 bytes: inventory 0, type 0, the group, item name \"\"",
            ),
        ),
    ),
    ("notice_while_refused_told_b", both(Answer::Text("nothing"))),
    (
        "notice_with_a_notecard_told_a",
        each(
            Answer::Text(
                "GroupNotice from avatar A (a group), A's name, to avatar A, id another id, no region, not kept, text as sent, a bucket of 32 bytes: inventory 1, type 7, the group, item name \"gr-7-notecard\"",
            ),
            Answer::Text(
                "GroupNotice from the group (a group), A's name, to avatar A, id the notice with the notecard's id, no region, kept, text as sent, a bucket of 32 bytes: inventory 1, type 7, the group, item name \"gr-7-notecard\"",
            ),
        ),
    ),
    (
        "notice_with_a_notecard_told_b",
        each(
            Answer::Text(
                "GroupNotice from avatar A (a group), A's name, to avatar B, id the notice with the notecard's id, no region, not kept, text as sent, a bucket of 32 bytes: inventory 1, type 7, the group, item name \"gr-7-notecard\"",
            ),
            Answer::Text(
                "GroupNotice from the group (a group), A's name, to avatar B, id the notice with the notecard's id, no region, kept, text as sent, a bucket of 32 bytes: inventory 1, type 7, the group, item name \"gr-7-notecard\"",
            ),
        ),
    ),
    ("notices_refused_told_a", both(Answer::Text("nothing"))),
    ("notices_refused_told_b", both(Answer::Text("nothing"))),
    ("notices_taken_again_told_a", both(Answer::Text("nothing"))),
    ("notices_taken_again_told_b", both(Answer::Text("nothing"))),
    (
        "outsider_members",
        each(
            Answer::Text("members (said 0): "),
            Answer::Text(
                "members (said 1): avatar A: powers 0xffffffffffffe, title \"Owner of <the group's name>\", an owner, status \"Online\", contribution 0",
            ),
        ),
    ),
    (
        "outsider_profile",
        each(
            Answer::Text(
                "profile: the group, the group's name, charter \"gr-2-charter\", not listed, our title \"Member\", our powers 0x80018010000, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 2 roles, not published, not mature, owners' role the owners' role",
            ),
            Answer::Text(
                "profile: the group, the group's name, charter \"gr-2-charter\", not listed, our title \"\", our powers 0x0, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 3 roles, not published, not mature, owners' role the owners' role",
            ),
        ),
    ),
    (
        "outsider_profile_after_the_charter",
        each(
            Answer::Text(
                "profile: the group, the group's name, charter as sent, not listed, our title \"Member\", our powers 0x80018010000, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 2 roles, not published, not mature, owners' role the owners' role",
            ),
            Answer::Text(
                "profile: the group, the group's name, charter as sent, not listed, our title \"\", our powers 0x0, insignia none, founded by avatar A, fee 0, open, money 0, 1 members, 3 roles, not published, not mature, owners' role the owners' role",
            ),
        ),
    ),
    (
        "outsider_roles",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "roles: \"Everyone\" (the Everyone role): title \"Member of <the group's name>\", description \"Everyone in the group is in the everyone role.\", powers 0x390018010000, 1 members; \"Officers\" (another id): title \"Officer of <the group's name>\", description \"The officers of the group, with more powers than regular members.\", powers 0x1fdffff7ff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner of <the group's name>\", description \"Owners of the group\", powers 0xffffffffffffe, 1 members",
            ),
        ),
    ),
    (
        "proposals_active",
        each(
            Answer::Text("active proposals: 0 in all, 0 here"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "proposals_history",
        each(
            Answer::Text("vote history: 0 in all"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "role_assign_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe",
            ),
        ),
    ),
    ("role_assign_told_b", both(Answer::Text("nothing"))),
    (
        "role_create_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe",
            ),
        ),
    ),
    ("role_create_told_b", both(Answer::Text("nothing"))),
    (
        "role_delete_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe",
            ),
        ),
    ),
    ("role_delete_told_b", both(Answer::Text("nothing"))),
    (
        "role_members_after_the_assignment",
        each(
            Answer::Text(
                "role members: avatar A in the owners' role; avatar B in the role the grid made",
            ),
            Answer::Text(
                "role members: avatar A in the Everyone role; avatar A in the owners' role; avatar B in the Everyone role; avatar B in the role the grid made",
            ),
        ),
    ),
    (
        "role_powers_changed_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe",
            ),
        ),
    ),
    ("role_powers_changed_told_b", both(Answer::Text("nothing"))),
    (
        "role_unassign_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "active group the fresh group, the fresh group's name, title \"Owner of <the fresh group's name>\", powers 0xffffffffffffe",
            ),
        ),
    ),
    ("role_unassign_told_b", both(Answer::Text("nothing"))),
    (
        "roles_after_the_delete",
        each(
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member\", description \"Everyone in the group is in the everyone role.\", powers 0x80018010000, 0 members; \"Officers\" (another id): title \"Officer\", description \"The officers of the group, with more powers than regular members.\", powers 0x1ccfffffff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner\", description \"These are the owners of the group.  They always have FULL POWER over the group.\", powers 0xffffffffffffffff, 1 members",
            ),
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member of <the group's name>\", description \"Everyone in the group is in the everyone role.\", powers 0x390018010000, 2 members; \"Officers\" (another id): title \"Officer of <the group's name>\", description \"The officers of the group, with more powers than regular members.\", powers 0x1fdffff7ff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner of <the group's name>\", description \"Owners of the group\", powers 0xffffffffffffe, 1 members",
            ),
        ),
    ),
    (
        "roles_with_the_new_role",
        each(
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member\", description \"Everyone in the group is in the everyone role.\", powers 0x80018010000, 0 members; \"Officers\" (another id): title \"Officer\", description \"The officers of the group, with more powers than regular members.\", powers 0x1ccfffffff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner\", description \"These are the owners of the group.  They always have FULL POWER over the group.\", powers 0xffffffffffffffff, 1 members; \"gr-3-role\" (the role the grid made): title \"gr-4-title\", description \"a role of the groups census\", powers 0x40000000002, 0 members",
            ),
            Answer::Text(
                "\"Everyone\" (the Everyone role): title \"Member of <the group's name>\", description \"Everyone in the group is in the everyone role.\", powers 0x390018010000, 2 members; \"Officers\" (another id): title \"Officer of <the group's name>\", description \"The officers of the group, with more powers than regular members.\", powers 0x1fdffff7ff8ce, 0 members; \"Owners\" (the owners' role): title \"Owner of <the group's name>\", description \"Owners of the group\", powers 0xffffffffffffe, 1 members; \"gr-3-role\" (the role the grid made): title \"gr-4-title\", description \"a role of the groups census\", powers 0x40000000002, 0 members",
            ),
        ),
    ),
    ("title_worn_told_a", both(Answer::Text("nothing"))),
    (
        "title_worn_told_b",
        both(Answer::Text(
            "active group none, no name, title \"\", powers 0x0",
        )),
    ),
    (
        "titles_in_the_role",
        each(
            Answer::Text(
                "titles: \"Member\" of the Everyone role (worn); \"gr-4-title\" of the role the grid made",
            ),
            Answer::Text(
                "titles: \"Everyone\" of the Everyone role; \"gr-3-role\" of the role the grid made (worn)",
            ),
        ),
    ),
    ("udp_accept_made_a_member", both(Answer::Flag(true))),
    ("udp_accept_told_a", both(Answer::Text("nothing"))),
    (
        "udp_accept_told_b",
        each(
            Answer::Text(
                "alert \"You have been added to the group.\" | join of the group taken | active group the group, the group's name, title \"Member\", powers 0x80018010000 | memberships: the group (the group's name): powers 0x80018010000, notices taken, contribution 0, insignia none",
            ),
            Answer::Text(
                "MessageBox from nobody, the name \"Groups\", to avatar B, id avatar B, no region, not kept, text \"You have been added to the group.\", no bucket | active group the group, the group's name, title \"Member of <the group's name>\", powers 0x390018010000 | memberships: the group (the group's name): powers 0x390018010000, notices taken, contribution 0, insignia none",
            ),
        ),
    ),
    (
        "udp_invite_while_away_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "udp_kept_invitation",
        each(
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, no region, kept, text \"<A> (<A>) has invited you to join a group.\\nThere is no cost to join this group.\\nGroup:\\n<the group's name>: gr-2-charter\\n\\n\", a bucket of 20 bytes: fee 0, role nobody",
            ),
            Answer::Text(
                "GroupInvitation from the group (a group), A's name, to avatar B, id the invitation's id, a region, kept, text \"<A> has invited you to join a group called <the group's name>. There is no cost to join this group.\", a bucket of 20 bytes: fee 0, role nobody",
            ),
        ),
    ),
    ("udp_return_told_a", both(Answer::Text("nothing"))),
    (
        "udp_return_unasked_b",
        both(Answer::Text(
            "active group none, no name, title \"\", powers 0x0 | memberships: none of the census's",
        )),
    ),
    (
        "unban_answer",
        each(Answer::Text("200, \"OK\""), Answer::Text("no capability")),
    ),
    (
        "unban_told_a",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "unban_told_b",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
];

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every instant message, in arrival order.
    ims: Vec<InstantMessage>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Every change of the active group.
    active: Vec<ActiveGroup>,
    /// Every list of memberships.
    memberships: Vec<Vec<GroupMembership>>,
    /// Every founding answered: the group, whether it was founded, the text.
    created: Vec<(Uuid, bool, String)>,
    /// Every join answered.
    joined: Vec<(Uuid, bool)>,
    /// Every leave answered.
    left: Vec<(Uuid, bool)>,
    /// Every ejection answered.
    ejected: Vec<(Uuid, bool)>,
    /// Every group the agent was dropped from, and how it was said.
    dropped: Vec<(Uuid, &'static str)>,
    /// Every member read, and the member counts the replies stated.
    members: Vec<GroupMember>,
    /// The member counts the replies stated.
    member_counts: Vec<i32>,
    /// Every role read.
    roles: Vec<GroupRole>,
    /// Every pairing of a role and a member read.
    role_members: Vec<GroupRoleMember>,
    /// Every title read.
    titles: Vec<GroupTitle>,
    /// Every profile read.
    profiles: Vec<GroupProfile>,
    /// Every notice list read.
    notice_lists: Vec<Vec<GroupNotice>>,
    /// Every answer about the accounts or the proposals, as a metric states
    /// it.
    accounts: Vec<String>,
    /// Every balance.
    money: Vec<MoneyBalance>,
    /// Every inventory item created or updated.
    items: Vec<InventoryItem>,
}

impl Got {
    /// Whether anything that was kept for the avatar is among the messages.
    fn any_kept(&self) -> bool {
        self.ims.iter().any(|im| im.offline)
    }

    /// Adds every message `other` holds.
    fn absorb(&mut self, mut other: Self) {
        self.ims.append(&mut other.ims);
        self.alerts.append(&mut other.alerts);
    }
}

/// A date as a metric states it: whether there is one.
fn date_text(date: &str) -> &'static str {
    if date.trim().is_empty() {
        "no date"
    } else {
        "a date"
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
        }
        Event::ActiveGroupChanged(active) => got.active.push((**active).clone()),
        Event::GroupMemberships(groups) => got.memberships.push(groups.clone()),
        Event::CreateGroupResult {
            group_id,
            success,
            message,
        } => got
            .created
            .push((group_id.uuid(), *success, message.clone())),
        Event::JoinGroupResult { group_id, success } => {
            got.joined.push((group_id.uuid(), *success));
        }
        Event::LeaveGroupResult { group_id, success } => {
            got.left.push((group_id.uuid(), *success));
        }
        Event::EjectGroupMemberResult { group_id, success } => {
            got.ejected.push((group_id.uuid(), *success));
        }
        Event::DroppedFromGroup { group_id } => {
            got.dropped.push((group_id.uuid(), "AgentDropGroup"));
        }
        Event::AgentDroppedFromGroup { group } => {
            got.dropped
                .push((group.uuid(), "AgentDropGroup over the event queue"));
        }
        Event::GroupMembers {
            member_count,
            members,
            ..
        } => {
            got.member_counts.push(*member_count);
            got.members.extend(members.iter().cloned());
        }
        Event::GroupRoleData { roles, .. } => got.roles.extend(roles.iter().cloned()),
        Event::GroupRoleMembers { pairs, .. } => got.role_members.extend(pairs.iter().copied()),
        Event::GroupTitles { titles, .. } => got.titles.extend(titles.iter().cloned()),
        Event::GroupProfileReceived(profile) => got.profiles.push((**profile).clone()),
        Event::GroupNotices { notices, .. } => got.notice_lists.push(notices.clone()),
        Event::GroupAccountSummary(summary) => got.accounts.push(format!(
            "summary: {} days, interval {}, started {}, balance {}, credits {}, debits {}, \
             {} members taxed, last tax {}, next tax {}",
            summary.interval_days,
            summary.current_interval,
            date_text(&summary.start_date),
            summary.balance,
            summary.total_credits.0,
            summary.total_debits.0,
            summary.non_exempt_members,
            date_text(&summary.last_tax_date),
            date_text(&summary.tax_date),
        )),
        Event::GroupAccountDetails(details) => got.accounts.push(format!(
            "details: {} days, interval {}, started {}, {} entries",
            details.interval_days,
            details.current_interval,
            date_text(&details.start_date),
            details.entries.len(),
        )),
        Event::GroupAccountTransactions(transactions) => got.accounts.push(format!(
            "transactions: {} days, interval {}, started {}, {} entries",
            transactions.interval_days,
            transactions.current_interval,
            date_text(&transactions.start_date),
            transactions.entries.len(),
        )),
        Event::GroupActiveProposals {
            total_num_items,
            proposals,
            ..
        } => got.accounts.push(format!(
            "active proposals: {total_num_items} in all, {} here",
            proposals.len()
        )),
        Event::GroupVoteHistory {
            total_num_items, ..
        } => got
            .accounts
            .push(format!("vote history: {total_num_items} in all")),
        Event::MoneyBalance(balance) => got.money.push(balance.clone()),
        Event::InventoryItemCreated { item, .. } => got.items.push(item.clone()),
        Event::InventoryBulkUpdate { items, .. } => got.items.extend(items.iter().cloned()),
        _ => return None,
    }
    Some(())
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
async fn watch_for(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Got) -> bool,
) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&got) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || limit.saturating_sub(started.elapsed()),
            |at| quiet.saturating_sub(at.elapsed()),
        );
        if remaining.is_zero() {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event| fold(&mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, window, window, |_got| false).await
}

/// Which of the two avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The owner.
    A,
    /// The member.
    B,
}

/// What a leg brought each avatar.
struct Brought {
    /// What A was sent.
    a: Got,
    /// What B was sent.
    b: Got,
}

/// The ids and names a metric names by what they are rather than by their
/// value.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// Every way the grid writes A's name: the legacy name, the first name of
    /// a Resident, the user name.
    a_names: Vec<String>,
    /// Every way the grid writes B's name.
    b_names: Vec<String>,
    /// The census group.
    group: Uuid,
    /// The census group's name.
    group_name: String,
    /// The census group's owners' role.
    owner_role: Uuid,
    /// The names of the groups the census founded, by what a metric calls
    /// them.
    group_names: Vec<(String, &'static str)>,
    /// Ids the census made or was given, by what a metric calls them.
    named: Vec<(Uuid, &'static str)>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else if id == self.group {
            "the group"
        } else if id == self.owner_role {
            "the owners' role"
        } else {
            self.named
                .iter()
                .rev()
                .find(|(named, _what)| *named == id)
                .map_or("another id", |(_named, what)| *what)
        }
    }

    /// Whose name `name` is.
    fn name_text(&self, name: &str) -> String {
        let is = |names: &[String]| names.iter().any(|known| name.eq_ignore_ascii_case(known));
        if name.is_empty() {
            "no name".to_owned()
        } else if is(&self.a_names) {
            "A's name".to_owned()
        } else if is(&self.b_names) {
            "B's name".to_owned()
        } else {
            format!("the name {:?}", self.scrubbed(name))
        }
    }

    /// `text` with nothing in it that is one run's alone: the avatars' names,
    /// the groups' names, every id and every date.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = text.to_owned();
        let mut names: Vec<(&str, String)> = self
            .group_names
            .iter()
            .map(|(name, what)| (name.as_str(), format!("<{what}'s name>")))
            .collect();
        names.push((self.group_name.as_str(), "<the group's name>".to_owned()));
        names.extend(
            self.a_names
                .iter()
                .map(|name| (name.as_str(), "<A>".to_owned())),
        );
        names.extend(
            self.b_names
                .iter()
                .map(|name| (name.as_str(), "<B>".to_owned())),
        );
        // The longest first, so that a name inside another is not cut out of it.
        names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
        for (name, word) in names {
            if !name.is_empty() {
                out = out.replace(name, &word);
            }
        }
        out = without_dates(&without_ids(&out));
        if out.len() > 240 {
            let cut = (0..=240)
                .rev()
                .find(|at| out.is_char_boundary(*at))
                .unwrap_or(0);
            out.truncate(cut);
            out.push('…');
        }
        out
    }

    /// What a group's name is.
    fn group_name_text(&self, name: &str) -> String {
        if name == self.group_name {
            return "the group's name".to_owned();
        }
        self.group_names
            .iter()
            .find(|(named, _what)| named == name)
            .map_or_else(
                || format!("the name {:?}", self.scrubbed(name)),
                |(_named, what)| format!("{what}'s name"),
            )
    }

    /// What a bucket holds, read as `dialog` reads it.
    #[expect(
        clippy::big_endian_bytes,
        reason = "a group invitation's membership fee is a wire-defined big-endian S32"
    )]
    fn bucket_text(&self, dialog: ImDialog, bucket: &[u8]) -> String {
        if bucket.is_empty() {
            return "no bucket".to_owned();
        }
        if bucket == [0] {
            return "a bucket of one zero byte".to_owned();
        }
        match dialog {
            ImDialog::GroupNotice | ImDialog::GroupNoticeRequested => {
                let group = bucket
                    .get(2..18)
                    .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
                    .map_or("no group", |bytes| self.id_text(Uuid::from_bytes(bytes)));
                let name = bucket.get(18..).map_or_else(String::new, |tail| {
                    String::from_utf8_lossy(tail)
                        .trim_end_matches('\0')
                        .to_owned()
                });
                format!(
                    "a bucket of {} bytes: inventory {}, type {}, {group}, item name {:?}",
                    bucket.len(),
                    bucket.first().copied().unwrap_or(0),
                    bucket.get(1).copied().unwrap_or(0),
                    self.scrubbed(&name),
                )
            }
            ImDialog::GroupInvitation => {
                let fee = bucket
                    .get(0..4)
                    .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
                    .map(i32::from_be_bytes);
                let role = bucket
                    .get(4..20)
                    .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
                    .map(Uuid::from_bytes);
                format!(
                    "a bucket of {} bytes: fee {}, role {}",
                    bucket.len(),
                    fee.map_or_else(|| "none".to_owned(), |fee| fee.to_string()),
                    role.map_or("none", |role| self.id_text(role)),
                )
            }
            _ => {
                let text = String::from_utf8_lossy(bucket);
                format!(
                    "a bucket of {} bytes reading {:?}",
                    bucket.len(),
                    self.scrubbed(text.trim_end_matches('\0'))
                )
            }
        }
    }

    /// One message as a metric states it; `sent` is the text the leg sent,
    /// where the message is the leg's own.
    fn im_text(&self, im: &InstantMessage, sent: Option<&str>) -> String {
        let text = match sent {
            Some(sent) if im.message == sent => "as sent".to_owned(),
            _ => format!("{:?}", self.scrubbed(&im.message)),
        };
        format!(
            "{:?} from {}{}, {}, to {}, id {}, {}, {}, text {text}, {}",
            im.dialog,
            self.id_text(im.from_agent_id.uuid()),
            if im.from_group { " (a group)" } else { "" },
            self.name_text(&im.from_agent_name),
            self.id_text(im.to_agent_id.uuid()),
            self.id_text(im.id),
            if im.region_id.is_some() {
                "a region"
            } else {
                "no region"
            },
            if im.offline { "kept" } else { "not kept" },
            self.bucket_text(im.dialog, &im.binary_bucket),
        )
    }

    /// One membership as a metric states it.
    fn membership_text(&self, membership: &GroupMembership) -> String {
        format!(
            "{} ({}): powers {:#x}, notices {}, contribution {}, insignia {}",
            self.id_text(membership.group_id.uuid()),
            self.group_name_text(&membership.group_name),
            membership.group_powers,
            if membership.accept_notices {
                "taken"
            } else {
                "refused"
            },
            membership.contribution.0,
            if membership.group_insignia_id.uuid().is_nil() {
                "none"
            } else {
                "some"
            },
        )
    }

    /// Whether `group` is one the census knows.
    fn knows(&self, group: Uuid) -> bool {
        self.id_text(group) != "another id"
    }

    /// One member as a metric states it.
    fn member_text(&self, member: &GroupMember) -> String {
        format!(
            "{}: powers {:#x}, title {:?}, {}, status {:?}, contribution {}",
            self.id_text(member.agent_id.uuid()),
            member.agent_powers,
            self.scrubbed(&member.title),
            if member.is_owner {
                "an owner"
            } else {
                "not an owner"
            },
            self.scrubbed(&member.online_status),
            member.contribution.0,
        )
    }

    /// One role as a metric states it.
    fn role_text(&self, role: &GroupRole) -> String {
        format!(
            "{:?} ({}): title {:?}, description {:?}, powers {:#x}, {} members",
            role.name,
            role.role_id
                .map_or("the Everyone role", |id| self.id_text(id.uuid())),
            self.scrubbed(&role.title),
            self.scrubbed(&role.description),
            role.powers,
            role.members,
        )
    }

    /// One profile as a metric states it.
    fn profile_text(&self, profile: &GroupProfile, charter: Option<&str>) -> String {
        let charter = match charter {
            Some(sent) if profile.charter == sent => "as sent".to_owned(),
            _ => format!("{:?}", self.scrubbed(&profile.charter)),
        };
        format!(
            "{}, {}, charter {charter}, {}, our title {:?}, our powers {:#x}, insignia {}, \
             founded by {}, fee {}, {}, money {}, {} members, {} roles, {}, {}, owners' role {}",
            self.id_text(profile.group_id.uuid()),
            self.group_name_text(&profile.name),
            if profile.show_in_list {
                "listed"
            } else {
                "not listed"
            },
            self.scrubbed(&profile.member_title),
            profile.powers,
            if profile.insignia_id.is_some() {
                "some"
            } else {
                "none"
            },
            self.id_text(profile.founder_id.uuid()),
            profile.membership_fee.0,
            if profile.open_enrollment {
                "open"
            } else {
                "by invitation"
            },
            profile.money,
            profile.member_count,
            profile.role_count,
            if profile.allow_publish {
                "published"
            } else {
                "not published"
            },
            if profile.mature_publish {
                "mature"
            } else {
                "not mature"
            },
            self.id_text(profile.owner_role.uuid()),
        )
    }

    /// One notice header as a metric states it; `sent` is the subject the
    /// leg posted.
    fn notice_text(&self, notice: &GroupNotice, sent: Option<&str>) -> String {
        let subject = match sent {
            Some(sent) if notice.subject == sent => "as sent".to_owned(),
            _ => format!("{:?}", self.scrubbed(&notice.subject)),
        };
        format!(
            "notice {} from {}, subject {subject}, {}, {}",
            self.id_text(notice.notice_id.uuid()),
            self.name_text(&notice.from_name),
            if notice.has_attachment {
                format!("an attachment of type {}", notice.asset_type)
            } else {
                "no attachment".to_owned()
            },
            if notice.timestamp == 0 {
                "no time"
            } else {
                "a time"
            },
        )
    }

    /// One balance as a metric states it.
    fn money_text(balance: &MoneyBalance) -> String {
        let transaction = balance.transaction.as_ref().map_or_else(
            || "no transaction".to_owned(),
            |transaction| {
                format!(
                    "transaction of type {} for {}",
                    transaction.transaction_type, transaction.amount.0
                )
            },
        );
        format!(
            "balance{}, description {:?}, {transaction}",
            if balance.success { "" } else { " (failed)" },
            without_ids(&balance.description),
        )
    }

    /// Everything `got` holds: each message as the grid worded it, every
    /// notice, every reply; `sent` is the text of the leg's own message,
    /// where it sent one.
    #[expect(
        clippy::too_many_lines,
        reason = "one line of text a kind of thing a session may be sent"
    )]
    fn everything(&self, got: &Got, sent: Option<&str>) -> String {
        let mut all: Vec<String> = got.ims.iter().map(|im| self.im_text(im, sent)).collect();
        all.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scrubbed(alert))),
        );
        all.extend(got.created.iter().map(|(group, success, message)| {
            format!(
                "founding {}: {}, {:?}",
                if *success { "taken" } else { "refused" },
                self.id_text(*group),
                self.scrubbed(message),
            )
        }));
        for (word, list) in [
            ("join", &got.joined),
            ("leave", &got.left),
            ("ejection", &got.ejected),
        ] {
            all.extend(list.iter().map(|(group, success)| {
                format!(
                    "{word} of {} {}",
                    self.id_text(*group),
                    if *success { "taken" } else { "refused" },
                )
            }));
        }
        all.extend(
            got.dropped
                .iter()
                .map(|(group, how)| format!("dropped from {} ({how})", self.id_text(*group))),
        );
        all.extend(got.active.iter().map(|active| {
            format!(
                "active group {}, {}, title {:?}, powers {:#x}",
                active
                    .active_group_id
                    .map_or("none", |group| self.id_text(group.uuid())),
                if active.group_name.is_empty() {
                    "no name".to_owned()
                } else {
                    self.group_name_text(&active.group_name)
                },
                self.scrubbed(&active.group_title),
                active.group_powers,
            )
        }));
        all.extend(got.memberships.iter().map(|groups| {
            let ours: Vec<String> = groups
                .iter()
                .filter(|membership| self.knows(membership.group_id.uuid()))
                .map(|membership| self.membership_text(membership))
                .collect();
            if ours.is_empty() {
                "memberships: none of the census's".to_owned()
            } else {
                format!("memberships: {}", ours.join("; "))
            }
        }));
        if !got.member_counts.is_empty() {
            let mut members: Vec<String> = got
                .members
                .iter()
                .map(|member| self.member_text(member))
                .collect();
            members.sort_unstable();
            all.push(format!(
                "members (said {}): {}",
                got.member_counts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                members.join("; ")
            ));
        }
        if !got.roles.is_empty() {
            let mut roles: Vec<String> =
                got.roles.iter().map(|role| self.role_text(role)).collect();
            roles.sort_unstable();
            all.push(format!("roles: {}", roles.join("; ")));
        }
        if !got.role_members.is_empty() {
            let mut pairs: Vec<String> = got
                .role_members
                .iter()
                .map(|pair| {
                    format!(
                        "{} in {}",
                        self.id_text(pair.member_id.uuid()),
                        pair.role_id
                            .map_or("the Everyone role", |role| self.id_text(role.uuid())),
                    )
                })
                .collect();
            pairs.sort_unstable();
            all.push(format!("role members: {}", pairs.join("; ")));
        }
        if !got.titles.is_empty() {
            let mut titles: Vec<String> = got
                .titles
                .iter()
                .map(|title| {
                    format!(
                        "{:?} of {}{}",
                        self.scrubbed(&title.title),
                        title
                            .role_id
                            .map_or("the Everyone role", |role| self.id_text(role.uuid())),
                        if title.selected { " (worn)" } else { "" },
                    )
                })
                .collect();
            titles.sort_unstable();
            all.push(format!("titles: {}", titles.join("; ")));
        }
        all.extend(
            got.profiles
                .iter()
                .map(|profile| format!("profile: {}", self.profile_text(profile, sent))),
        );
        all.extend(got.notice_lists.iter().map(|notices| {
            // In no order a grid keeps.
            let mut ours: Vec<String> = notices
                .iter()
                .filter(|notice| self.knows(notice.notice_id.uuid()))
                .map(|notice| self.notice_text(notice, sent))
                .collect();
            ours.sort_unstable();
            format!(
                "notice list: {}",
                if ours.is_empty() {
                    "none of the census's".to_owned()
                } else {
                    ours.join("; ")
                }
            )
        }));
        all.extend(got.accounts.iter().cloned());
        all.extend(got.money.iter().map(Self::money_text));
        all.extend(got.items.iter().map(|item| {
            format!(
                "item {:?} of type {}, in {}",
                self.scrubbed(&item.name),
                item.item_type,
                self.id_text(item.folder_id.uuid()),
            )
        }));
        if all.is_empty() {
            "nothing".to_owned()
        } else {
            all.join(" | ")
        }
    }

    /// One record of the `ReadOfflineMsgs` capability as a metric states it:
    /// every key, and what its value is.
    fn record_text(&self, record: &Llsd, sent: &str) -> String {
        let Llsd::Map(map) = record else {
            return shape_of(record);
        };
        let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
        entries.sort_unstable_by_key(|(key, _value)| *key);
        let entries: Vec<String> = entries
            .into_iter()
            .map(|(key, value)| {
                let text = match value {
                    Llsd::Uuid(id) => self.id_text(*id).to_owned(),
                    Llsd::String(text) => match (key.as_str(), Uuid::parse_str(text)) {
                        (_, Ok(id)) => format!("the text of {}", self.id_text(id)),
                        ("message", _) if text == sent => "as sent".to_owned(),
                        ("from_agent_name", _) => self.name_text(text),
                        ("message_time", _) => "a date and time".to_owned(),
                        _ => format!("{:?}", self.scrubbed(text)),
                    },
                    Llsd::Integer(_) if key == "timestamp" => "a time".to_owned(),
                    Llsd::Integer(number) => number.to_string(),
                    Llsd::Real(number) if *number == 0.0 => "0".to_owned(),
                    Llsd::Real(_) => "a number".to_owned(),
                    Llsd::Boolean(flag) => flag.to_string(),
                    Llsd::Binary(bytes) => {
                        format!("{{{}}}", self.bucket_text(ImDialog::GroupInvitation, bytes))
                    }
                    Llsd::Array(items) => format!("[{}]", items.len()),
                    other => shape_of(other),
                };
                format!("{key}={text}")
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }

    /// A `GroupAPIv1` answer as a metric states it: the status, and who is
    /// banned since when.
    fn bans_text(&self, status: u16, text: &str) -> String {
        if !text.contains("<llsd") {
            return answer_text(status, text);
        }
        let Ok(body) = parse_llsd_xml(text) else {
            return format!("{status}, LLSD that does not parse");
        };
        let Llsd::Map(map) = &body else {
            return format!("{status}, {}", shape_of(&body));
        };
        let mut parts: Vec<String> = Vec::new();
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort_unstable();
        for key in keys {
            let Some(value) = map.get(key) else {
                continue;
            };
            match (key.as_str(), value) {
                ("ban_list", Llsd::Map(bans)) => {
                    let mut banned: Vec<String> = bans
                        .iter()
                        .map(|(agent, ban)| {
                            let who = Uuid::parse_str(agent)
                                .map_or("an agent that is no id", |id| self.id_text(id));
                            format!("{who} {}", shape_of(ban))
                        })
                        .collect();
                    banned.sort_unstable();
                    parts.push(format!("ban_list=[{}]", banned.join(", ")));
                }
                ("group_id", Llsd::Uuid(id)) => {
                    parts.push(format!("group_id={}", self.id_text(*id)));
                }
                (_, other) => parts.push(format!("{key}={}", shape_of(other))),
            }
        }
        format!("{status}, {{{}}}", parts.join(", "))
    }
}

/// `text` with every date in it, `12/31/2026` or `2026-12-31`, written
/// `<date>`.
fn without_dates(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, word) in text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let core = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let numbers: Vec<&str> = core.split(['/', '-']).collect();
        let is_date = numbers.len() == 3
            && numbers
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        if is_date {
            out.push_str(&word.replace(core, "<date>"));
        } else {
            out.push_str(word);
        }
    }
    out
}

/// How the avatar that comes back answers the invitation that was kept for
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answering {
    /// `RetrieveInstantMessages`, then an accept over UDP.
    Udp,
    /// The client's drain ([`Command::RequestOfflineMessages`]), then the
    /// accept the viewer makes of what it read.
    Client,
    /// The `ReadOfflineMsgs` capability read by the case, then a decline
    /// through the `DeclineGroupInvite` capability.
    Capability,
}

impl Answering {
    /// The word the round's metrics begin with.
    const fn word(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Client => "client",
            Self::Capability => "cap",
        }
    }
}

/// Everything the legs share.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one thing the clean up has to undo"
)]
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids and names the metrics name.
    scene: Scene,
    /// How many texts the census has made.
    said: u32,
    /// Whether B is logged in.
    b_is_here: bool,
    /// Whether B is a member of the census group, as far as the census
    /// knows.
    b_is_member: bool,
    /// Whether B is banned from the census group, as far as the census knows.
    b_is_banned: bool,
    /// The role the census made, while it stands.
    role: Option<GroupRoleKey>,
    /// The groups the census founded and A has not left yet.
    founded: Vec<GroupKey>,
    /// A's folders.
    folders: Folders,
    /// What the census rezzed and made.
    residue: Residue,
    /// Whether this run founded the census group: it then measures in it
    /// and keeps it, and founds no other.
    bootstrapped: bool,
}

impl Census<'_> {
    /// A text of the census for `word`.
    fn text(&mut self, word: &str) -> String {
        self.said = self.said.saturating_add(1);
        format!("{TEXT_TAG}{}-{word}", self.said)
    }

    /// The census group.
    fn group(&self) -> GroupKey {
        GroupKey::from(self.scene.group)
    }

    /// Avatar B.
    fn b_key(&self) -> AgentKey {
        AgentKey::from(self.scene.b)
    }

    /// Has `who` send `commands` and returns what the two avatars were then
    /// sent: the actor listens until `done` says it has what the leg came for
    /// (or [`LISTEN`]), the other one for a moment after. An avatar that is
    /// away is sent nothing.
    async fn act_until(
        &mut self,
        who: Who,
        commands: Vec<Command>,
        done: impl FnMut(&Got) -> bool,
    ) -> Result<Brought, TestFailure> {
        let (actor, listener) = match who {
            Who::A => (&mut *self.a, &mut *self.b),
            Who::B => (&mut *self.b, &mut *self.a),
        };
        for command in commands {
            actor.send(command).await?;
        }
        let acted = watch_for(actor, ECHO_WINDOW, LISTEN, done).await?;
        let listened = if who == Who::B || self.b_is_here {
            watch(listener, DRAIN).await?
        } else {
            Got::default()
        };
        Ok(match who {
            Who::A => Brought {
                a: acted,
                b: listened,
            },
            Who::B => Brought {
                a: listened,
                b: acted,
            },
        })
    }

    /// Has `who` send `commands` and returns what the two avatars were then
    /// sent, each listening for [`LISTEN`].
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        self.act_until(who, commands, |_got| false).await
    }

    /// Records under `key` what each avatar was told: `{key}_told_a` and
    /// `{key}_told_b`.
    fn record(&self, metrics: &mut Metrics, key: &str, brought: &Brought, sent: Option<&str>) {
        metrics.set(
            &format!("{key}_told_a"),
            self.scene.everything(&brought.a, sent),
        );
        if self.b_is_here {
            metrics.set(
                &format!("{key}_told_b"),
                self.scene.everything(&brought.b, sent),
            );
        }
    }

    /// Has `who` ask with `command` and records under `key` what it was
    /// answered: the reply, or `nothing`.
    async fn ask(
        &mut self,
        metrics: &mut Metrics,
        who: Who,
        key: &str,
        command: Command,
        sent: Option<&str>,
    ) -> Result<(), TestFailure> {
        let session = match who {
            Who::A => &mut *self.a,
            Who::B => &mut *self.b,
        };
        session.send(command).await?;
        let got = watch(session, LISTEN).await?;
        metrics.set(key, self.scene.everything(&got, sent));
        Ok(())
    }

    /// A deed by the owner: A rezzes a cube, sets it to the group and deeds
    /// it; A and then B move it; A takes it away.
    async fn deed(&mut self, metrics: &mut Metrics, run: &mut Run) -> Result<(), TestFailure> {
        let group = self.group();
        let mut cube = rez_cube(self.a, run, &mut self.residue, 0).await?;
        let local_id = cube.scoped_id();
        self.a
            .send(Command::SetObjectGroup {
                local_ids: vec![local_id],
                group_id: group,
            })
            .await?;
        let heard = listen(self.a, STEP_TIMEOUT, run.pace.quiet, |_heard| false).await?;
        metrics.set("deed_set_group_alerts", joined(&heard.alerts));
        let record = record_of(self.a, run, &cube).await?;
        metrics.set(
            "deed_set_group_reads",
            record
                .group
                .map_or("none", |set| self.scene.id_text(set.uuid())),
        );

        // A deed of a cube not shared with the group: the reference offers
        // none.
        self.deed_once(metrics, run, &cube, "deed_unshared").await?;
        let record = record_of(self.a, run, &cube).await?;
        if matches!(record.owner, OwnerKey::Agent(_)) {
            // Shared with the group, as the reference asks before a deed.
            self.a
                .send(Command::SetObjectPermissions {
                    local_ids: vec![local_id],
                    field: PermissionField::Group,
                    set: true,
                    mask: Permissions::MOVE | Permissions::MODIFY | Permissions::COPY,
                })
                .await?;
            listen(self.a, STEP_TIMEOUT, run.pace.quiet, |_heard| false).await?;
            self.deed_once(metrics, run, &cube, "deed_shared").await?;
        } else {
            for key in [
                "deed_shared_alerts",
                "deed_shared_answered_with",
                "deed_shared_owner_reads",
                "deed_shared_group_reads",
                "deed_shared_last_owner_reads",
                "deed_shared_masks",
            ] {
                metrics.set(key, "deeded unshared already");
            }
        }

        // The owner moves it.
        reread(self.a, run, &mut cube).await?;
        let before = cube.motion.position.clone();
        self.a.send(moved(local_id, &lifted(&before, 0.5))).await?;
        let heard = listen(self.a, STEP_TIMEOUT, run.pace.quiet, |_heard| false).await?;
        reread(self.a, run, &mut cube).await?;
        metrics.set(
            "deed_owner_move_took",
            !near(&before, &cube.motion.position),
        );
        metrics.set("deed_owner_move_alerts", joined(&heard.alerts));

        // A member without the power to move the group's objects moves it.
        let before = cube.motion.position.clone();
        self.b.send(moved(local_id, &lifted(&before, 0.5))).await?;
        let got = watch(self.b, LISTEN).await?;
        reread(self.a, run, &mut cube).await?;
        metrics.set(
            "deed_member_move_took",
            !near(&before, &cube.motion.position),
        );
        metrics.set(
            "deed_member_move_alerts",
            if got.alerts.is_empty() {
                "none".to_owned()
            } else {
                got.alerts
                    .iter()
                    .map(|alert| self.scene.scrubbed(alert))
                    .collect::<Vec<_>>()
                    .join(" | ")
            },
        );

        // The owner takes it away, to its Trash, under the group.
        self.a
            .send(Command::DerezObjects {
                local_ids: vec![local_id],
                destination: DeRezDestination::Trash(self.folders.trash),
                transaction_id: TransactionId::from(Uuid::new_v4()),
                group_id: Some(group),
            })
            .await?;
        let heard = listen(self.a, STEP_TIMEOUT, run.pace.quiet, |heard| {
            heard.killed(local_id)
        })
        .await?;
        let gone = heard.killed(local_id);
        metrics.set("deed_delete_took", gone);
        metrics.set("deed_delete_alerts", joined(&heard.alerts));
        let items: Vec<String> = heard
            .items()
            .map(|item| {
                format!(
                    "item of type {} in {}, owned by {}",
                    item.item_type,
                    self.folders.word(item.folder_id),
                    match item.owner {
                        OwnerKey::Agent(agent) => self.scene.id_text(agent.uuid()),
                        OwnerKey::Group(owner) => self.scene.id_text(owner.uuid()),
                    }
                )
            })
            .collect();
        metrics.set("deed_delete_items", joined(&items));
        self.residue
            .items
            .extend(heard.items().map(|item| item.item_id));
        if gone {
            self.residue.roots.retain(|root| *root != local_id);
        }
        Ok(())
    }

    /// A deeds `cube` to the census group and records under `key` what it
    /// was told and what the cube's record then reads, a moment later.
    async fn deed_once(
        &mut self,
        metrics: &mut Metrics,
        run: &Run,
        cube: &Object,
        key: &str,
    ) -> Result<(), TestFailure> {
        self.a
            .send(Command::DeedObjectsToGroup {
                local_ids: vec![cube.scoped_id()],
                group_id: self.group(),
            })
            .await?;
        let heard = listen(self.a, STEP_TIMEOUT, run.pace.quiet, |_heard| false).await?;
        metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
        metrics.set(&format!("{key}_answered_with"), heard.brought(cube));
        // A deed may be carried out after its answer: read the record again
        // once the grid has had a moment.
        tokio::time::sleep(SETTLE).await;
        let record = record_of(self.a, run, cube).await?;
        metrics.set(
            &format!("{key}_owner_reads"),
            match record.owner {
                OwnerKey::Agent(agent) => self.scene.id_text(agent.uuid()),
                OwnerKey::Group(owner) => self.scene.id_text(owner.uuid()),
            },
        );
        metrics.set(
            &format!("{key}_group_reads"),
            record
                .group
                .map_or("none", |set| self.scene.id_text(set.uuid())),
        );
        metrics.set(
            &format!("{key}_last_owner_reads"),
            self.scene.id_text(record.last_owner_id),
        );
        record_masks(metrics, key, &record);
        Ok(())
    }

    /// Founds a group named `name`, and returns it where it was founded.
    async fn found(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        name: &str,
        charter: &str,
        what: &'static str,
    ) -> Result<Option<GroupKey>, TestFailure> {
        self.a.send(Command::RequestMoneyBalance).await?;
        let before = watch_for(self.a, ECHO_WINDOW, Duration::from_secs(1), |got| {
            !got.money.is_empty()
        })
        .await?;
        let before = before.money.last().map(|balance| balance.balance.0);
        let started = Instant::now();
        let brought = self
            .act_until(
                Who::A,
                vec![Command::CreateGroup(CreateGroupParams {
                    name: name.to_owned(),
                    charter: charter.to_owned(),
                    show_in_list: false,
                    insignia_id: None,
                    membership_fee: LindenAmount(0),
                    open_enrollment: true,
                    allow_publish: false,
                    mature_publish: false,
                })],
                |got| !got.created.is_empty(),
            )
            .await?;
        let founded = brought
            .a
            .created
            .iter()
            .find(|(_group, success, _message)| *success)
            .map(|(group, _success, _message)| GroupKey::from(*group));
        if founded.is_some() {
            metrics.set_timing(&secs_metric(key), started.elapsed().as_secs_f64());
        }
        if let Some(group) = founded {
            self.founded.push(group);
            self.scene.named.push((group.uuid(), what));
        }
        // What was said before the answer: the founding may take longer than
        // a moment on a grid that charges for it.
        let later = watch(
            self.a,
            if founded.is_some() {
                SETTLE
            } else {
                CREATE_WINDOW
            },
        )
        .await?;
        self.record(metrics, key, &brought, Some(charter));
        metrics.set(
            &format!("{key}_later_told_a"),
            self.scene.everything(&later, Some(charter)),
        );
        self.a.send(Command::RequestMoneyBalance).await?;
        let after = watch_for(self.a, ECHO_WINDOW, Duration::from_secs(1), |got| {
            !got.money.is_empty()
        })
        .await?;
        let after = after.money.last().map(|balance| balance.balance.0);
        metrics.set(
            &format!("{key}_charged"),
            match (before, after) {
                (Some(before), Some(after)) => i64::try_from(before)
                    .ok()
                    .zip(i64::try_from(after).ok())
                    .and_then(|(before, after)| before.checked_sub(after))
                    .unwrap_or(-1),
                _ => -1,
            },
        );
        Ok(founded)
    }

    /// A founds a fresh group and reads it.
    async fn founding(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let name = format!("gs fresh {}", random_word());
        self.scene
            .group_names
            .push((name.clone(), "the fresh group"));
        let charter = self.text("fresh-charter");
        let Some(fresh) = self
            .found(metrics, "found", &name, &charter, "the fresh group")
            .await?
        else {
            return Ok(());
        };
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::A,
            "fresh_profile",
            Command::RequestGroupProfile(fresh),
            Some(&charter),
        )
        .await?;
        let roles = self.roles_of(fresh).await?;
        for role in &roles {
            if let Some(id) = role.role_id {
                self.scene.named.push((id.uuid(), "a role the grid made"));
            }
        }
        let mut texts: Vec<String> = roles
            .iter()
            .map(|role| self.scene.role_text(role))
            .collect();
        texts.sort_unstable();
        metrics.set("fresh_roles", texts.join("; "));
        self.ask(
            metrics,
            Who::A,
            "fresh_members",
            Command::RequestGroupMembers(fresh),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::A,
            "fresh_role_members",
            Command::RequestGroupRoleMembers(fresh),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::A,
            "fresh_titles",
            Command::RequestGroupTitles(fresh),
            None,
        )
        .await
    }

    /// The roles of `group`, as A reads them.
    async fn roles_of(&mut self, group: GroupKey) -> Result<Vec<GroupRole>, TestFailure> {
        self.a.send(Command::RequestGroupRoles(group)).await?;
        Ok(watch(self.a, LISTEN).await?.roles)
    }

    /// B leaves the census group and reads it as an outsider; A edits the
    /// charter.
    async fn leaving(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let brought = self
            .act_until(Who::B, vec![Command::LeaveGroup(group)], |got| {
                !got.left.is_empty()
            })
            .await?;
        self.b_is_member = false;
        self.record(metrics, "leave", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::B,
            "outsider_profile",
            Command::RequestGroupProfile(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "outsider_roles",
            Command::RequestGroupRoles(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "outsider_members",
            Command::RequestGroupMembers(group),
            None,
        )
        .await?;

        let charter = self.text("charter");
        let brought = self
            .act(
                Who::A,
                vec![Command::UpdateGroupInfo(UpdateGroupInfoParams {
                    group_id: group,
                    charter: charter.clone(),
                    show_in_list: false,
                    insignia_id: None,
                    membership_fee: LindenAmount(0),
                    open_enrollment: true,
                    allow_publish: false,
                    mature_publish: false,
                })],
            )
            .await?;
        self.record(metrics, "charter", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::B,
            "outsider_profile_after_the_charter",
            Command::RequestGroupProfile(group),
            Some(&charter),
        )
        .await
    }

    /// B joins; the members over both roads; B's view as a member; B makes
    /// the group its active one and drops it.
    async fn joining(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let brought = self
            .act_until(Who::B, vec![Command::JoinGroup(group)], |got| {
                !got.joined.is_empty()
            })
            .await?;
        self.b_is_member = brought
            .b
            .joined
            .iter()
            .any(|(joined, success)| *joined == group.uuid() && *success);
        self.record(metrics, "join", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::A,
            "members_udp",
            Command::RequestGroupMembers(group),
            None,
        )
        .await?;
        metrics.set(
            "members_capability_granted",
            self.a.cap(GROUP_MEMBER_DATA).is_some(),
        );
        self.ask(
            metrics,
            Who::A,
            "members_capability",
            Command::FetchGroupMembers(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "member_profile",
            Command::RequestGroupProfile(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "member_titles",
            Command::RequestGroupTitles(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "member_roles",
            Command::RequestGroupRoles(group),
            None,
        )
        .await?;

        let brought = self
            .act_until(Who::B, vec![Command::ActivateGroup(Some(group))], |got| {
                !got.active.is_empty()
            })
            .await?;
        self.record(metrics, "activate", &brought, None);
        let brought = self
            .act_until(Who::B, vec![Command::ActivateGroup(None)], |got| {
                !got.active.is_empty()
            })
            .await?;
        self.record(metrics, "deactivate", &brought, None);
        Ok(())
    }

    /// A makes a role, puts B in it, B wears its title, A takes B out and
    /// deletes the role.
    async fn roles(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let chosen = Uuid::new_v4();
        self.scene.named.push((chosen, "the role's id as sent"));
        let name = self.text("role");
        let title = self.text("title");
        let edit = |update_type, powers| GroupRoleEdit {
            role_id: Some(GroupRoleKey::from(chosen)),
            name: name.clone(),
            description: "a role of the groups census".to_owned(),
            title: title.clone(),
            powers,
            update_type,
        };
        let brought = self
            .act(
                Who::A,
                vec![Command::UpdateGroupRoles {
                    group_id: group,
                    roles: vec![edit(
                        GroupRoleUpdateType::Create,
                        group_powers::NOTICES_SEND | group_powers::MEMBER_INVITE,
                    )],
                }],
            )
            .await?;
        self.record(metrics, "role_create", &brought, None);
        tokio::time::sleep(SETTLE).await;
        let roles = self.roles_of(group).await?;
        let made = roles
            .iter()
            .find(|role| role.name == name)
            .and_then(|role| role.role_id);
        if let Some(made) = made
            && made.uuid() != chosen
        {
            self.scene
                .named
                .push((made.uuid(), "the role the grid made"));
        }
        let mut texts: Vec<String> = roles
            .iter()
            .map(|role| self.scene.role_text(role))
            .collect();
        texts.sort_unstable();
        metrics.set("roles_with_the_new_role", texts.join("; "));
        let Some(role) = made else {
            return Ok(());
        };
        self.role = Some(role);

        let member_id = self.b_key();
        let change = move |change| Command::ChangeGroupRoleMembers {
            group_id: group,
            changes: vec![GroupRoleMemberChange {
                role_id: Some(role),
                member_id,
                change,
            }],
        };
        let brought = self.act(Who::A, vec![change(GroupRoleChange::Add)]).await?;
        self.record(metrics, "role_assign", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::A,
            "role_members_after_the_assignment",
            Command::RequestGroupRoleMembers(group),
            None,
        )
        .await?;
        self.ask(
            metrics,
            Who::B,
            "titles_in_the_role",
            Command::RequestGroupTitles(group),
            None,
        )
        .await?;
        let brought = self
            .act(
                Who::B,
                vec![Command::UpdateGroupTitle {
                    group_id: group,
                    title_role_id: role,
                }],
            )
            .await?;
        self.record(metrics, "title_worn", &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.ask(
            metrics,
            Who::A,
            "members_with_the_title",
            Command::RequestGroupMembers(group),
            None,
        )
        .await?;
        let brought = self
            .act(
                Who::A,
                vec![Command::UpdateGroupRoles {
                    group_id: group,
                    roles: vec![GroupRoleEdit {
                        role_id: Some(role),
                        ..edit(
                            GroupRoleUpdateType::UpdatePowers,
                            group_powers::NOTICES_SEND,
                        )
                    }],
                }],
            )
            .await?;
        self.record(metrics, "role_powers_changed", &brought, None);

        let brought = self
            .act(Who::A, vec![change(GroupRoleChange::Remove)])
            .await?;
        self.record(metrics, "role_unassign", &brought, None);
        tokio::time::sleep(SETTLE).await;
        let brought = self
            .act(
                Who::A,
                vec![Command::UpdateGroupRoles {
                    group_id: group,
                    roles: vec![GroupRoleEdit {
                        role_id: Some(role),
                        ..edit(GroupRoleUpdateType::Delete, 0)
                    }],
                }],
            )
            .await?;
        self.record(metrics, "role_delete", &brought, None);
        tokio::time::sleep(SETTLE).await;
        let roles = self.roles_of(group).await?;
        if !roles.iter().any(|listed| listed.role_id == Some(role)) {
            self.role = None;
        }
        let mut texts: Vec<String> = roles
            .iter()
            .map(|role| self.scene.role_text(role))
            .collect();
        texts.sort_unstable();
        metrics.set("roles_after_the_delete", texts.join("; "));
        Ok(())
    }

    /// A posts a notice, B asks for it again, B stops taking notices and A
    /// posts again, and A posts one with a notecard.
    async fn notices(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let notice = |subject: &str, attachment| Command::SendGroupNotice {
            group_id: group,
            subject: subject.to_owned(),
            message: "a notice of the groups census".to_owned(),
            attachment,
        };
        let subject = self.text("notice");
        let wire = format!("{subject}|a notice of the groups census");
        let brought = self
            .act_until(Who::A, vec![notice(&subject, None)], |got| {
                got.ims.iter().any(|im| im.dialog == ImDialog::GroupNotice)
            })
            .await?;
        let posted = brought
            .b
            .ims
            .iter()
            .chain(&brought.a.ims)
            .find(|im| im.dialog == ImDialog::GroupNotice && im.message == wire)
            .map(|im| im.id);
        if let Some(posted) = posted {
            self.scene.named.push((posted, "the notice's id"));
        }
        self.record(metrics, "notice", &brought, Some(&wire));
        let listed = self
            .notice_list(metrics, "notice_list", &subject, "the listed notice's id")
            .await?;
        if let Some(asked) = listed.or(posted) {
            let brought = self
                .act(
                    Who::B,
                    vec![Command::RequestGroupNotice(GroupNoticeKey::from(asked))],
                )
                .await?;
            self.record(metrics, "notice_asked_again", &brought, Some(&wire));
        } else {
            metrics.set("notice_asked_again_told_a", "no notice to ask for");
            metrics.set("notice_asked_again_told_b", "no notice to ask for");
        }

        let brought = self
            .act(
                Who::B,
                vec![Command::SetGroupAcceptNotices {
                    group_id: group,
                    accept_notices: false,
                    list_in_profile: true,
                }],
            )
            .await?;
        self.record(metrics, "notices_refused", &brought, None);
        tokio::time::sleep(SETTLE).await;
        let subject = self.text("unwanted-notice");
        let wire = format!("{subject}|a notice of the groups census");
        let brought = self
            .act_until(Who::A, vec![notice(&subject, None)], |got| {
                got.ims.iter().any(|im| im.dialog == ImDialog::GroupNotice)
            })
            .await?;
        self.record(metrics, "notice_while_refused", &brought, Some(&wire));
        let brought = self
            .act(
                Who::B,
                vec![Command::SetGroupAcceptNotices {
                    group_id: group,
                    accept_notices: true,
                    list_in_profile: true,
                }],
            )
            .await?;
        self.record(metrics, "notices_taken_again", &brought, None);
        tokio::time::sleep(SETTLE).await;

        let name = self.text("notecard");
        let item = create_item(
            self.a,
            &mut self.residue,
            &self.folders,
            AssetType::Notecard,
            InventoryType::Notecard,
            &name,
        )
        .await?;
        self.scene.named.push((item.item_id.uuid(), "the notecard"));
        let subject = self.text("notice-with-a-notecard");
        let wire = format!("{subject}|a notice of the groups census");
        let brought = self
            .act_until(
                Who::A,
                vec![notice(
                    &subject,
                    Some(GroupNoticeAttachment {
                        item_id: item.item_id,
                        owner_id: self.scene.a,
                    }),
                )],
                |got| got.ims.iter().any(|im| im.dialog == ImDialog::GroupNotice),
            )
            .await?;
        if let Some(posted) = brought
            .b
            .ims
            .iter()
            .chain(&brought.a.ims)
            .find(|im| im.dialog == ImDialog::GroupNotice && im.message == wire)
        {
            self.scene
                .named
                .push((posted.id, "the notice with the notecard's id"));
        }
        self.record(metrics, "notice_with_a_notecard", &brought, Some(&wire));
        self.notice_list(
            metrics,
            "notice_list_with_the_notecard",
            &subject,
            "the listed notice with the notecard's id",
        )
        .await?;
        Ok(())
    }

    /// A reads the census group's notice list a while after a notice was
    /// posted, names the newest entry with `subject` as `what`, records the
    /// list under `key`, and returns that entry's id.
    async fn notice_list(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        subject: &str,
        what: &'static str,
    ) -> Result<Option<Uuid>, TestFailure> {
        tokio::time::sleep(NOTICE_LIST_PAUSE).await;
        self.a
            .send(Command::RequestGroupNotices(self.group()))
            .await?;
        let got = watch(self.a, LISTEN).await?;
        let listed = got
            .notice_lists
            .iter()
            .flatten()
            .filter(|notice| notice.subject == subject)
            .max_by_key(|notice| notice.timestamp)
            .map(|notice| notice.notice_id.uuid());
        if let Some(listed) = listed
            && !self.scene.knows(listed)
        {
            self.scene.named.push((listed, what));
        }
        metrics.set(key, self.scene.everything(&got, Some(subject)));
        Ok(listed)
    }

    /// A asks for the accounts and the proposals.
    async fn accounts(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let request_id = || GroupRequestId::from(Uuid::new_v4());
        for (key, command) in [
            (
                "accounts_summary",
                Command::RequestGroupAccountSummary {
                    group_id: group,
                    request_id: request_id(),
                    interval_days: 7,
                    current_interval: 0,
                },
            ),
            (
                "accounts_details",
                Command::RequestGroupAccountDetails {
                    group_id: group,
                    request_id: request_id(),
                    interval_days: 7,
                    current_interval: 0,
                },
            ),
            (
                "accounts_transactions",
                Command::RequestGroupAccountTransactions {
                    group_id: group,
                    request_id: request_id(),
                    interval_days: 7,
                    current_interval: 0,
                },
            ),
            (
                "proposals_active",
                Command::RequestGroupActiveProposals {
                    group_id: group,
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                },
            ),
            (
                "proposals_history",
                Command::RequestGroupVoteHistory {
                    group_id: group,
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                },
            ),
        ] {
            self.ask(metrics, Who::A, key, command, None).await?;
        }
        Ok(())
    }

    /// A ejects B.
    async fn ejection(&mut self, metrics: &mut Metrics, key: &str) -> Result<(), TestFailure> {
        let group = self.group();
        let brought = self
            .act_until(
                Who::A,
                vec![Command::EjectGroupMembers {
                    group_id: group,
                    member_ids: vec![self.b_key()],
                }],
                |got| !got.ejected.is_empty(),
            )
            .await?;
        if brought
            .a
            .ejected
            .iter()
            .any(|(ejected, success)| *ejected == group.uuid() && *success)
        {
            self.b_is_member = false;
        }
        self.record(metrics, key, &brought, None);
        // Whether the ejected member is told it is out later than at once.
        let later = watch_for(self.b, ECHO_WINDOW, SETTLE, |got| {
            !got.dropped.is_empty() || !got.memberships.is_empty()
        })
        .await?;
        metrics.set(
            &format!("{key}_later_told_b"),
            self.scene.everything(&later, None),
        );
        Ok(())
    }

    /// The invitation A sends B.
    fn invite(&self) -> Command {
        Command::InviteToGroup {
            group_id: self.group(),
            invitees: vec![(self.b_key(), GroupRoleKey::from(Uuid::nil()))],
        }
    }

    /// The invitation to the census group among what `got` holds.
    fn the_invitation(got: &Got) -> Option<InstantMessage> {
        got.ims
            .iter()
            .rfind(|im| im.dialog == ImDialog::GroupInvitation)
            .cloned()
    }

    /// B's accept of `invitation` as the viewer makes it: the group the
    /// client read from it, over the capabilities where the client says so.
    fn client_accept(invitation: &InstantMessage) -> Option<Command> {
        let read = invitation.group_invitation()?;
        Some(Command::AcceptGroupInvitation {
            group_id: read.group_id,
            transaction_id: TransactionId::from(read.transaction_id),
            use_offline_cap: invitation.answered_over_capabilities(),
        })
    }

    /// B's answer to `invitation` over UDP.
    fn answer(&self, invitation: &InstantMessage, accept: bool) -> Command {
        let group_id = self.group();
        let transaction_id = TransactionId::from(invitation.id);
        if accept {
            Command::AcceptGroupInvitation {
                group_id,
                transaction_id,
                use_offline_cap: false,
            }
        } else {
            Command::DeclineGroupInvitation {
                group_id,
                transaction_id,
                use_offline_cap: false,
            }
        }
    }

    /// A invites B, B declines; A invites again, B accepts; A invites B, who
    /// is a member.
    async fn invitations(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group().uuid();
        for (key, accept) in [("invite_declined", false), ("invite_accepted", true)] {
            let brought = self.act(Who::A, vec![self.invite()]).await?;
            let invitation = Self::the_invitation(&brought.b);
            if let Some(invitation) = &invitation {
                self.scene
                    .named
                    .push((invitation.id, "the invitation's id"));
            }
            self.record(metrics, key, &brought, None);
            let Some(invitation) = invitation else {
                metrics.set(&format!("{key}_answer_told_a"), "no invitation to answer");
                metrics.set(&format!("{key}_answer_told_b"), "no invitation to answer");
                continue;
            };
            let brought = self
                .act(Who::B, vec![self.answer(&invitation, accept)])
                .await?;
            if accept {
                self.b_is_member = true;
            }
            self.record(metrics, &format!("{key}_answer"), &brought, None);
            tokio::time::sleep(SETTLE).await;
        }
        let brought = self.act(Who::A, vec![self.invite()]).await?;
        self.record(metrics, "invite_a_member", &brought, None);
        if let Some(invitation) = Self::the_invitation(&brought.b) {
            self.scene
                .named
                .push((invitation.id, "the invitation's id"));
            // Answer it, so that nothing is left pending.
            self.b.send(self.answer(&invitation, false)).await?;
            watch(self.b, SETTLE).await?;
        }
        self.b.send(Command::RequestAgentDataUpdate).await?;
        let got = watch(self.b, LISTEN).await?;
        self.b_is_member = got
            .memberships
            .last()
            .is_some_and(|groups| groups.iter().any(|entry| entry.group_id.uuid() == group));
        Ok(())
    }

    /// A bans B over `GroupAPIv1`, B tries to join and A invites it, A unbans
    /// B.
    async fn bans(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        let http = http_client()?;
        let Some(url) = self.a.cap(GROUP_API_V1) else {
            for key in [
                "ban_list_before",
                "ban_answer",
                "ban_list",
                "ban_told_a",
                "ban_told_b",
                "join_while_banned_told_a",
                "join_while_banned_told_b",
                "invite_while_banned_told_a",
                "invite_while_banned_told_b",
                "unban_answer",
                "unban_told_a",
                "unban_told_b",
                "ban_list_after",
            ] {
                metrics.set(key, NO_CAPABILITY);
            }
            return Ok(());
        };
        let url = format!("{url}?group_id={}", group.uuid());
        let list = async |http: &reqwest::Client| -> Result<(u16, String), TestFailure> {
            let response = http
                .get(&url)
                .header("Accept", "application/llsd+xml")
                .send()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("the GroupAPIv1 request failed: {error}"))
                })?;
            let status = response.status().as_u16();
            Ok((status, response.text().await.unwrap_or_default()))
        };
        let (status, text) = list(&http).await?;
        metrics.set("ban_list_before", self.scene.bans_text(status, &text));
        let b = self.scene.b;
        let ban_body = move |action: i32| {
            Llsd::Map(HashMap::from([
                ("ban_action".to_owned(), Llsd::Integer(action)),
                ("ban_ids".to_owned(), Llsd::Array(vec![Llsd::Uuid(b)])),
            ]))
            .to_llsd_xml()
        };
        let post = async |http: &reqwest::Client, body: String| -> Result<String, TestFailure> {
            let response = http
                .post(&url)
                .header("Content-Type", "application/llsd+xml")
                .header("Accept", "application/llsd+xml")
                .body(body)
                .send()
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("the GroupAPIv1 request failed: {error}"))
                })?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Ok(answer_text(status, &text))
        };
        let answer = post(&http, ban_body(1)).await?;
        self.b_is_banned = true;
        metrics.set("ban_answer", answer);
        let brought = self.act(Who::A, Vec::new()).await?;
        self.record(metrics, "ban", &brought, None);
        let (status, text) = list(&http).await?;
        metrics.set("ban_list", self.scene.bans_text(status, &text));

        let brought = self
            .act_until(Who::B, vec![Command::JoinGroup(group)], |got| {
                !got.joined.is_empty()
            })
            .await?;
        self.record(metrics, "join_while_banned", &brought, None);
        let brought = self.act(Who::A, vec![self.invite()]).await?;
        self.record(metrics, "invite_while_banned", &brought, None);
        if let Some(invitation) = Self::the_invitation(&brought.b) {
            self.b.send(self.answer(&invitation, false)).await?;
            watch(self.b, SETTLE).await?;
        }

        let answer = post(&http, ban_body(2)).await?;
        metrics.set("unban_answer", answer);
        let brought = self.act(Who::A, Vec::new()).await?;
        self.record(metrics, "unban", &brought, None);
        let (status, text) = list(&http).await?;
        if !text.contains(&self.scene.b.to_string()) {
            self.b_is_banned = false;
        }
        metrics.set("ban_list_after", self.scene.bans_text(status, &text));
        Ok(())
    }

    /// Makes sure B is no member of the census group.
    async fn b_out(&mut self) -> Result<(), TestFailure> {
        self.b.send(Command::RequestAgentDataUpdate).await?;
        let got = watch(self.b, LISTEN).await?;
        let group = self.scene.group;
        if let Some(groups) = got.memberships.last() {
            self.b_is_member = groups.iter().any(|entry| entry.group_id.uuid() == group);
        }
        if self.b_is_member {
            self.b.send(Command::LeaveGroup(self.group())).await?;
            watch(self.b, SETTLE).await?;
            self.b_is_member = false;
        }
        Ok(())
    }

    /// B comes back: what each is told while B is left alone, and what B was
    /// told before anything was asked.
    async fn b_comes(&mut self, metrics: &mut Metrics, key: &str) -> Result<Got, TestFailure> {
        self.b.relogin().await?;
        self.b_is_here = true;
        // Nothing waits for the region here: that wait would throw away what
        // the arrival brings.
        let unasked = watch(self.b, UNASKED_WINDOW).await?;
        let told_a = watch(self.a, DRAIN).await?;
        metrics.set(
            &format!("{key}_told_a"),
            self.scene.everything(&told_a, None),
        );
        Ok(unasked)
    }

    /// B logs out, A invites it; B comes back and answers as `answering`
    /// says.
    async fn away_round(
        &mut self,
        metrics: &mut Metrics,
        answering: Answering,
    ) -> Result<(), TestFailure> {
        let round = answering.word();
        self.b_out().await?;
        self.b.disconnect().await?;
        self.b_is_here = false;
        tokio::time::sleep(AWAY_SETTLE).await;
        let brought = self.act(Who::A, vec![self.invite()]).await?;
        self.record(
            metrics,
            &format!("{round}_invite_while_away"),
            &brought,
            None,
        );

        let unasked = self.b_comes(metrics, &format!("{round}_return")).await?;
        metrics.set(
            &format!("{round}_return_unasked_b"),
            self.scene.everything(&unasked, None),
        );
        let waited = Instant::now();
        while self.b.cap(READ_OFFLINE_MSGS).is_none() && waited.elapsed() < CAP_WAIT {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if answering == Answering::Capability {
            return self.capability_round(metrics, unasked).await;
        }
        self.b
            .send(if answering == Answering::Client {
                Command::RequestOfflineMessages
            } else {
                Command::RetrieveInstantMessages
            })
            .await?;
        let got = watch_for(self.b, FETCH_WINDOW, FETCH_QUIET, Got::any_kept).await?;
        let mut all = unasked;
        all.absorb(got);
        let invitation = Self::the_invitation(&all);
        if let Some(invitation) = &invitation {
            self.scene
                .named
                .push((invitation.id, "the invitation's id"));
        }
        metrics.set(
            &format!("{round}_kept_invitation"),
            invitation.as_ref().map_or_else(
                || "nothing".to_owned(),
                |invitation| self.scene.im_text(invitation, None),
            ),
        );
        let Some(invitation) = invitation else {
            return Ok(());
        };
        let answer = if answering == Answering::Client {
            Self::client_accept(&invitation)
        } else {
            Some(self.answer(&invitation, true))
        };
        let brought = self.act(Who::B, answer.into_iter().collect()).await?;
        self.record(metrics, &format!("{round}_accept"), &brought, None);
        tokio::time::sleep(SETTLE).await;
        self.b.send(Command::RequestAgentDataUpdate).await?;
        let got = watch(self.b, LISTEN).await?;
        let group = self.scene.group;
        self.b_is_member = got
            .memberships
            .last()
            .is_some_and(|groups| groups.iter().any(|entry| entry.group_id.uuid() == group));
        metrics.set(&format!("{round}_accept_made_a_member"), self.b_is_member);
        Ok(())
    }

    /// B reads the capability itself and declines through the capability.
    async fn capability_round(
        &mut self,
        metrics: &mut Metrics,
        unasked: Got,
    ) -> Result<(), TestFailure> {
        let http = http_client()?;
        let group = self.group();
        let records = match self.b.cap(READ_OFFLINE_MSGS) {
            Some(url) => {
                let answer = cap_get(&http, &url).await?;
                kept_records(answer.body.as_ref())
            }
            None => Vec::new(),
        };
        let record = records
            .iter()
            .find(|record| record.get("dialog").and_then(Llsd::as_i32) == Some(3));
        metrics.set(
            "cap_kept_invitation",
            match (self.b.cap(READ_OFFLINE_MSGS), record) {
                (None, _) => NO_CAPABILITY.to_owned(),
                (Some(_), None) => "nothing".to_owned(),
                (Some(_), Some(record)) => self.scene.record_text(record, ""),
            },
        );
        // Without the capability the invitation is read over UDP, so that its
        // decline below has an id to name.
        let udp_invitation = if self.b.cap(READ_OFFLINE_MSGS).is_none() {
            self.b.send(Command::RetrieveInstantMessages).await?;
            let mut got = watch_for(self.b, FETCH_WINDOW, FETCH_QUIET, Got::any_kept).await?;
            got.absorb(unasked);
            Self::the_invitation(&got)
        } else {
            None
        };
        let answer = match self.b.cap(DECLINE_GROUP_INVITE) {
            Some(url) => {
                let body = Llsd::Map(HashMap::from([(
                    "group".to_owned(),
                    Llsd::Uuid(group.uuid()),
                )]))
                .to_llsd_xml();
                let response = http
                    .post(&url)
                    .header("Content-Type", "application/llsd+xml")
                    .header("Accept", "application/llsd+xml")
                    .body(body)
                    .send()
                    .await
                    .map_err(|error| {
                        TestFailure::Assertion(format!(
                            "the DeclineGroupInvite request failed: {error}"
                        ))
                    })?;
                let status = response.status().as_u16();
                let text = response.text().await.unwrap_or_default();
                answer_text(status, &text)
            }
            None => {
                if let Some(invitation) = &udp_invitation {
                    self.b.send(self.answer(invitation, false)).await?;
                }
                NO_CAPABILITY.to_owned()
            }
        };
        metrics.set("cap_decline_answer", answer);
        let brought = self.act(Who::B, Vec::new()).await?;
        self.record(metrics, "cap_decline", &brought, None);
        let again = match self.b.cap(READ_OFFLINE_MSGS) {
            Some(url) => {
                let answer = cap_get(&http, &url).await?;
                format!("{} messages", kept_records(answer.body.as_ref()).len())
            }
            None => NO_CAPABILITY.to_owned(),
        };
        metrics.set("cap_kept_after_the_decline", again);
        Ok(())
    }

    /// A leaves the groups it founded, and founds one with a name one
    /// character too long for Second Life.
    async fn leaving_a_founded_group(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        if let Some(fresh) = self.founded.first().copied() {
            let brought = self
                .act_until(Who::A, vec![Command::LeaveGroup(fresh)], |got| {
                    !got.left.is_empty()
                })
                .await?;
            if brought
                .a
                .left
                .iter()
                .any(|(left, success)| *left == fresh.uuid() && *success)
            {
                self.founded.retain(|group| *group != fresh);
            }
            self.record(metrics, "founder_leaves", &brought, None);
        } else {
            metrics.set("founder_leaves_told_a", "no group was founded");
            metrics.set("founder_leaves_told_b", "no group was founded");
        }
        tokio::time::sleep(SECOND_FOUNDING_PAUSE).await;
        let mut name = format!("gs long {}", random_word());
        while name.len() < LONG_NAME_LENGTH {
            name.push('x');
        }
        name.truncate(LONG_NAME_LENGTH);
        self.scene
            .group_names
            .push((name.clone(), "the long-named group"));
        let truncated: String = name.chars().take(LONG_NAME_LENGTH - 1).collect();
        self.scene
            .group_names
            .push((truncated, "the long-named group cut to 35"));
        let charter = self.text("long-charter");
        if let Some(long) = self
            .found(
                metrics,
                "found_long",
                &name,
                &charter,
                "the long-named group",
            )
            .await?
        {
            self.ask(
                metrics,
                Who::A,
                "long_profile",
                Command::RequestGroupProfile(long),
                Some(&charter),
            )
            .await?;
        } else {
            metrics.set("long_profile", "no group was founded");
        }
        Ok(())
    }

    /// Leaves what the census made: B a member and unbanned, the role gone,
    /// the founded groups left, the cube and the notecard taken away.
    async fn clean_up(&mut self) -> Result<(), TestFailure> {
        let group = self.group();
        if !self.b_is_here {
            self.b.relogin().await?;
            self.b_is_here = true;
        }
        if self.b_is_banned
            && let Some(url) = self.a.cap(GROUP_API_V1)
        {
            let http = http_client()?;
            let body = Llsd::Map(HashMap::from([
                ("ban_action".to_owned(), Llsd::Integer(2)),
                (
                    "ban_ids".to_owned(),
                    Llsd::Array(vec![Llsd::Uuid(self.scene.b)]),
                ),
            ]))
            .to_llsd_xml();
            cap_post(&http, &format!("{url}?group_id={}", group.uuid()), body).await?;
        }
        if let Some(role) = self.role {
            self.a
                .send(Command::UpdateGroupRoles {
                    group_id: group,
                    roles: vec![GroupRoleEdit {
                        role_id: Some(role),
                        name: String::new(),
                        description: String::new(),
                        title: String::new(),
                        powers: 0,
                        update_type: GroupRoleUpdateType::Delete,
                    }],
                })
                .await?;
        }
        if !self.bootstrapped {
            for founded in std::mem::take(&mut self.founded) {
                self.a.send(Command::LeaveGroup(founded)).await?;
            }
        }
        // Second Life disbands a young group of fewer than two members: B
        // stays in the census group between runs.
        self.b.send(Command::RequestAgentDataUpdate).await?;
        let got = watch(self.b, LISTEN).await?;
        let member = got.memberships.last().is_some_and(|groups| {
            groups
                .iter()
                .any(|entry| entry.group_id.uuid() == group.uuid())
        });
        if !member {
            self.b.send(Command::JoinGroup(group)).await?;
            watch(self.b, SETTLE).await?;
        }
        watch(self.a, SETTLE).await?;
        clean_up(self.a, &self.residue, self.folders.trash).await
    }
}

/// Every way a grid writes the name of the avatar whose legacy name is
/// `legacy`: the legacy name, a Resident's first name, and the user name
/// (`first` for a Resident, `first.last` otherwise, in lower case).
fn names_of(legacy: &str) -> Vec<String> {
    let mut names = vec![legacy.to_owned()];
    let (first, last) = legacy.split_once(' ').unwrap_or((legacy, ""));
    if last.is_empty() || last.eq_ignore_ascii_case("Resident") {
        names.push(first.to_owned());
        names.push(first.to_lowercase());
    } else {
        names.push(format!("{first}.{last}").to_lowercase());
    }
    names
}

/// A word no other run uses.
fn random_word() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

/// A census of groups: founding, membership, roles, notices, invitations,
/// ejection, bans, an invitation kept for an avatar that is away, and a deed.
#[derive(Debug)]
pub struct Groups;

impl GridTest for Groups {
    fn name(&self) -> &'static str {
        "groups"
    }

    fn description(&self) -> &'static str {
        "What founding, joining, leaving, roles, notices, invitations, ejection and bans bring \
         both avatars, what is kept of an invitation for an avatar that is away, and what a deed \
         by an owner makes of an object"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(3000)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The census: both avatars in, the census group found or founded, the legs,
/// and the clean up whatever became of them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let census_group = ctx.census_group();
    let pace = Pace::of(grid);
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "groups needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let folders = login_folders(a).await?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) =
        settle_scene(a, grid, build_position, SETTLE_WINDOW, pace.settle_idle).await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let mut run = Run {
        folders,
        owner: a_id,
        pace,
        position: anchor,
        seen,
    };
    run.position.z += REZ_LIFT_M;
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_names: names_of(&a.avatar_legacy_name()),
        b_names: names_of(&b.avatar_legacy_name()),
        ..Scene::default()
    };
    let mut census = Census {
        a,
        b,
        scene,
        said: 0,
        b_is_here: true,
        b_is_member: false,
        b_is_banned: false,
        role: None,
        founded: Vec::new(),
        folders,
        residue: Residue::default(),
        bootstrapped: false,
    };
    let outcome = legs(&mut census, metrics, &mut run, census_group).await;
    let cleaned = census.clean_up().await;
    outcome.and(cleaned)
}

/// Settles which group the census measures in, and that B is a member of it.
async fn the_census_group(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    configured: Option<GroupKey>,
) -> Result<(), TestFailure> {
    let group = if let Some(group) = configured {
        metrics.set("census_group_is", "the fixtures'");
        group
    } else {
        // No group named: found one, measure in it and keep it.
        let name = format!("gs census {}", random_word());
        let charter = census.text("census-charter");
        let founded = census
            .found(metrics, "bootstrap_found", &name, &charter, "the group")
            .await?
            .ok_or_else(|| {
                TestFailure::Assertion("the census group could not be founded".to_owned())
            })?;
        tracing::warn!(
            "the groups census founded its group {founded}: name it as census_group in the \
             fixtures"
        );
        census.bootstrapped = true;
        metrics.set("census_group_is", "founded by this run");
        tokio::time::sleep(SECOND_FOUNDING_PAUSE).await;
        founded
    };
    census.scene.group = group.uuid();
    census.a.send(Command::RequestGroupProfile(group)).await?;
    let got = watch_for(census.a, ECHO_WINDOW, Duration::from_secs(1), |got| {
        !got.profiles.is_empty()
    })
    .await?;
    let profile = got
        .profiles
        .iter()
        .find(|profile| profile.group_id == group)
        .ok_or_else(|| TestFailure::Assertion("the census group has no profile".to_owned()))?;
    census.scene.group_name.clone_from(&profile.name);
    census.scene.owner_role = profile.owner_role.uuid();
    census.b.send(Command::RequestAgentDataUpdate).await?;
    let got = watch(census.b, LISTEN).await?;
    census.b_is_member = got.memberships.last().is_some_and(|groups| {
        groups
            .iter()
            .any(|entry| entry.group_id.uuid() == group.uuid())
    });
    metrics.set("b_was_a_member", census.b_is_member);
    if !census.b_is_member {
        census.b.send(Command::JoinGroup(group)).await?;
        let got = watch_for(census.b, ECHO_WINDOW, SETTLE, |got| !got.joined.is_empty()).await?;
        census.b_is_member = got
            .joined
            .iter()
            .any(|(joined, success)| *joined == group.uuid() && *success);
        if !census.b_is_member {
            return Err(TestFailure::Assertion(
                "B could not join the census group".to_owned(),
            ));
        }
    }
    let granted: Vec<&str> = [
        READ_OFFLINE_MSGS,
        ACCEPT_GROUP_INVITE,
        DECLINE_GROUP_INVITE,
        GROUP_MEMBER_DATA,
        GROUP_API_V1,
    ]
    .into_iter()
    .filter(|name| census.a.cap(name).is_some())
    .collect();
    metrics.set(
        "group_capabilities_granted",
        if granted.is_empty() {
            "none".to_owned()
        } else {
            granted.join(", ")
        },
    );
    Ok(())
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    run: &mut Run,
    configured: Option<GroupKey>,
) -> Result<(), TestFailure> {
    watch(census.b, DRAIN).await?;
    the_census_group(census, metrics, configured).await?;
    census.deed(metrics, run).await?;
    if !census.bootstrapped {
        census.founding(metrics).await?;
    }
    census.leaving(metrics).await?;
    census.joining(metrics).await?;
    census.roles(metrics).await?;
    census.notices(metrics).await?;
    census.accounts(metrics).await?;
    census.ejection(metrics, "eject").await?;
    census.invitations(metrics).await?;
    if census.b_is_member {
        census.ejection(metrics, "eject_again").await?;
    }
    census.bans(metrics).await?;
    for answering in [Answering::Udp, Answering::Client, Answering::Capability] {
        census.away_round(metrics, answering).await?;
    }
    if !census.bootstrapped {
        census.leaving_a_founded_group(metrics).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Scene, names_of, without_dates};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;

    #[test]
    fn a_resident_is_also_named_by_first_and_user_name() {
        assert_eq!(
            names_of("Some Body"),
            vec!["Some Body".to_owned(), "some.body".to_owned()]
        );
        assert_eq!(
            names_of("SomeBody Resident"),
            vec![
                "SomeBody Resident".to_owned(),
                "SomeBody".to_owned(),
                "somebody".to_owned()
            ]
        );
    }

    #[test]
    fn a_date_is_written_as_such() {
        assert_eq!(without_dates("10/14/2026"), "<date>");
        assert_eq!(without_dates("since 2026-10-14."), "since <date>.");
        assert_eq!(without_dates("Online"), "Online");
        assert_eq!(without_dates("a 1/2 share"), "a 1/2 share");
    }

    #[test]
    fn a_ban_list_names_who_is_banned() {
        let b = Uuid::from_u128(7);
        let scene = Scene {
            b,
            ..Scene::default()
        };
        let text = format!(
            "<?xml version=\"1.0\" ?><llsd><map><key>ban_list</key><map><key>{b}</key><map>\
             <key>ban_date</key><date>2026-10-14T00:00:00Z</date></map></map></map></llsd>"
        );
        assert_eq!(
            scene.bans_text(200, &text),
            "200, {ban_list=[avatar B {ban_date:date}]}"
        );
    }
}
