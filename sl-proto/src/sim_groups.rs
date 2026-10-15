//! The simulator side of groups: the replies a [`SimSession`] sends to the
//! group requests it decodes (`ServerEvent::CreateGroup`,
//! `ServerEvent::RequestGroupProfile`, …), each the inverse of the client's
//! decoding of the same message, and the bodies of the three group
//! capabilities a driver answers (`GroupMemberData`, `GroupAPIv1`, and the
//! answers to a kept invitation).
//!
//! Like the rest of [`SimSession`], nothing here keeps groups: what a group
//! holds, and who is told what, is the driver's.

use std::collections::HashMap;
use std::time::Instant;

use sl_types::key::{AgentKey, GroupKey};
use sl_wire::Llsd;
use sl_wire::messages::{
    AgentDropGroup, AgentDropGroupAgentDataBlock, AgentGroupDataUpdate,
    AgentGroupDataUpdateAgentDataBlock, AgentGroupDataUpdateGroupDataBlock, AnyMessage,
    CreateGroupReply, CreateGroupReplyAgentDataBlock, CreateGroupReplyReplyDataBlock,
    EjectGroupMemberReply, EjectGroupMemberReplyAgentDataBlock,
    EjectGroupMemberReplyEjectDataBlock, EjectGroupMemberReplyGroupDataBlock, GroupMembersReply,
    GroupMembersReplyAgentDataBlock, GroupMembersReplyGroupDataBlock,
    GroupMembersReplyMemberDataBlock, GroupNoticesListReply, GroupNoticesListReplyAgentDataBlock,
    GroupNoticesListReplyDataBlock, GroupProfileReply, GroupProfileReplyAgentDataBlock,
    GroupProfileReplyGroupDataBlock, GroupRoleDataReply, GroupRoleDataReplyAgentDataBlock,
    GroupRoleDataReplyGroupDataBlock, GroupRoleDataReplyRoleDataBlock, GroupRoleMembersReply,
    GroupRoleMembersReplyAgentDataBlock, GroupRoleMembersReplyMemberDataBlock, GroupTitlesReply,
    GroupTitlesReplyAgentDataBlock, GroupTitlesReplyGroupDataBlock, JoinGroupReply,
    JoinGroupReplyAgentDataBlock, JoinGroupReplyGroupDataBlock, LeaveGroupReply,
    LeaveGroupReplyAgentDataBlock, LeaveGroupReplyGroupDataBlock,
};
use uuid::Uuid;

use crate::types::{
    GroupBan, GroupBanAction, GroupMember, GroupMembership, GroupNotice, GroupProfile, GroupRole,
    GroupRoleMember, GroupTitle, Reliability,
};
use crate::{Error, SimSession};

/// Encodes a string as NUL-terminated UTF-8 bytes, as a simulator sends a
/// variable string field.
fn with_nul(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the group replies own their `impl SimSession` block, apart from the session's \
              canonical impl in `sim_session.rs`"
)]
impl SimSession {
    /// This circuit's agent id, nil before the circuit opened.
    fn agent_uuid(&self) -> Uuid {
        self.agent_id().map_or_else(Uuid::nil, |agent| agent.uuid())
    }

    /// Sends `message` reliably.
    fn push_reliable(&mut self, message: &AnyMessage, now: Instant) -> Result<(), Error> {
        self.push(message, Reliability::Reliable, now)
    }

    /// Sends a `CreateGroupReply` — the answer to
    /// [`ServerEvent::CreateGroup`](crate::ServerEvent::CreateGroup) (the
    /// client's [`Event::CreateGroupResult`](crate::Event::CreateGroupResult)).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_create_group_reply(
        &mut self,
        group_id: GroupKey,
        success: bool,
        message: &str,
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::CreateGroupReply(CreateGroupReply {
            agent_data: CreateGroupReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            reply_data: CreateGroupReplyReplyDataBlock {
                group_id: group_id.uuid(),
                success,
                message: with_nul(message),
            },
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `JoinGroupReply` — the answer to
    /// [`ServerEvent::JoinGroup`](crate::ServerEvent::JoinGroup).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_join_group_reply(
        &mut self,
        group_id: GroupKey,
        success: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::JoinGroupReply(JoinGroupReply {
            agent_data: JoinGroupReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: JoinGroupReplyGroupDataBlock {
                group_id: group_id.uuid(),
                success,
            },
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `LeaveGroupReply` — the answer to
    /// [`ServerEvent::LeaveGroup`](crate::ServerEvent::LeaveGroup).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_leave_group_reply(
        &mut self,
        group_id: GroupKey,
        success: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::LeaveGroupReply(LeaveGroupReply {
            agent_data: LeaveGroupReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: LeaveGroupReplyGroupDataBlock {
                group_id: group_id.uuid(),
                success,
            },
        });
        self.push_reliable(&reply, now)
    }

    /// Sends an `EjectGroupMemberReply` — the answer to
    /// [`ServerEvent::EjectGroupMembers`](crate::ServerEvent::EjectGroupMembers).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_eject_group_member_reply(
        &mut self,
        group_id: GroupKey,
        success: bool,
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::EjectGroupMemberReply(EjectGroupMemberReply {
            agent_data: EjectGroupMemberReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: EjectGroupMemberReplyGroupDataBlock {
                group_id: group_id.uuid(),
            },
            eject_data: EjectGroupMemberReplyEjectDataBlock { success },
        });
        self.push_reliable(&reply, now)
    }

    /// Sends an `AgentDropGroup` over UDP — the agent is no longer in `group`
    /// (the client's [`Event::DroppedFromGroup`](crate::Event::DroppedFromGroup);
    /// [`enqueue_agent_drop_group`](Self::enqueue_agent_drop_group) is the event
    /// queue's form).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_agent_drop_group(&mut self, group: GroupKey, now: Instant) -> Result<(), Error> {
        let message = AnyMessage::AgentDropGroup(AgentDropGroup {
            agent_data: AgentDropGroupAgentDataBlock {
                agent_id: self.agent_uuid(),
                group_id: group.uuid(),
            },
        });
        self.push_reliable(&message, now)
    }

    /// Sends an `AgentGroupDataUpdate` — every group the agent is in (the
    /// client's [`Event::GroupMemberships`](crate::Event::GroupMemberships)).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if a contribution exceeds the wire's range or the message fails to
    /// encode.
    pub fn send_agent_group_data_update(
        &mut self,
        memberships: &[GroupMembership],
        now: Instant,
    ) -> Result<(), Error> {
        let group_data = memberships
            .iter()
            .map(|membership| {
                Ok(AgentGroupDataUpdateGroupDataBlock {
                    group_id: membership.group_id.uuid(),
                    group_powers: membership.group_powers,
                    accept_notices: membership.accept_notices,
                    group_insignia_id: membership.group_insignia_id.uuid(),
                    contribution: crate::types::land_area_to_wire(
                        "Contribution",
                        &membership.contribution,
                    )?,
                    group_name: with_nul(&membership.group_name),
                })
            })
            .collect::<Result<Vec<_>, sl_wire::WireError>>()?;
        let message = AnyMessage::AgentGroupDataUpdate(AgentGroupDataUpdate {
            agent_data: AgentGroupDataUpdateAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data,
        });
        self.push_reliable(&message, now)
    }

    /// Sends a `GroupProfileReply` — the answer to
    /// [`ServerEvent::RequestGroupProfile`](crate::ServerEvent::RequestGroupProfile).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the fee is out of the wire's range or the message fails to encode.
    pub fn send_group_profile_reply(
        &mut self,
        profile: &GroupProfile,
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::GroupProfileReply(GroupProfileReply {
            agent_data: GroupProfileReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: GroupProfileReplyGroupDataBlock {
                group_id: profile.group_id.uuid(),
                name: with_nul(&profile.name),
                charter: with_nul(&profile.charter),
                show_in_list: profile.show_in_list,
                member_title: with_nul(&profile.member_title),
                powers_mask: profile.powers,
                insignia_id: profile
                    .insignia_id
                    .map_or_else(Uuid::nil, |insignia| insignia.uuid()),
                founder_id: profile.founder_id.uuid(),
                membership_fee: crate::types::linden_to_wire(
                    "MembershipFee",
                    &profile.membership_fee,
                )?,
                open_enrollment: profile.open_enrollment,
                money: profile.money,
                group_membership_count: profile.member_count,
                group_roles_count: profile.role_count,
                allow_publish: profile.allow_publish,
                mature_publish: profile.mature_publish,
                owner_role: profile.owner_role.uuid(),
            },
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `GroupMembersReply` of `members`, stating `member_count` — the
    /// answer to
    /// [`ServerEvent::RequestGroupMembers`](crate::ServerEvent::RequestGroupMembers).
    /// The count is the caller's: a grid may state one the blocks do not hold.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if a contribution exceeds the wire's range or the message fails to
    /// encode.
    pub fn send_group_members_reply(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        member_count: i32,
        members: &[GroupMember],
        now: Instant,
    ) -> Result<(), Error> {
        let member_data = members
            .iter()
            .map(|member| {
                Ok(GroupMembersReplyMemberDataBlock {
                    agent_id: member.agent_id.uuid(),
                    contribution: crate::types::land_area_to_wire(
                        "Contribution",
                        &member.contribution,
                    )?,
                    online_status: with_nul(&member.online_status),
                    agent_powers: member.agent_powers,
                    title: with_nul(&member.title),
                    is_owner: member.is_owner,
                })
            })
            .collect::<Result<Vec<_>, sl_wire::WireError>>()?;
        let reply = AnyMessage::GroupMembersReply(GroupMembersReply {
            agent_data: GroupMembersReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: GroupMembersReplyGroupDataBlock {
                group_id: group_id.uuid(),
                request_id,
                member_count,
            },
            member_data,
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `GroupRoleDataReply` of `roles`, stating `role_count` — the
    /// answer to
    /// [`ServerEvent::RequestGroupRoles`](crate::ServerEvent::RequestGroupRoles).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_group_role_data_reply(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        role_count: i32,
        roles: &[GroupRole],
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::GroupRoleDataReply(GroupRoleDataReply {
            agent_data: GroupRoleDataReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
            },
            group_data: GroupRoleDataReplyGroupDataBlock {
                group_id: group_id.uuid(),
                request_id,
                role_count,
            },
            role_data: roles
                .iter()
                .map(|role| GroupRoleDataReplyRoleDataBlock {
                    role_id: role.role_id.map_or_else(Uuid::nil, |role| role.uuid()),
                    name: with_nul(&role.name),
                    title: with_nul(&role.title),
                    description: with_nul(&role.description),
                    powers: role.powers,
                    members: role.members,
                })
                .collect(),
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `GroupRoleMembersReply` of `pairs` — the answer to
    /// [`ServerEvent::RequestGroupRoleMembers`](crate::ServerEvent::RequestGroupRoleMembers).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_group_role_members_reply(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        pairs: &[GroupRoleMember],
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::GroupRoleMembersReply(GroupRoleMembersReply {
            agent_data: GroupRoleMembersReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
                group_id: group_id.uuid(),
                request_id,
                total_pairs: u32::try_from(pairs.len()).unwrap_or(u32::MAX),
            },
            member_data: pairs
                .iter()
                .map(|pair| GroupRoleMembersReplyMemberDataBlock {
                    role_id: pair.role_id.map_or_else(Uuid::nil, |role| role.uuid()),
                    member_id: pair.member_id.uuid(),
                })
                .collect(),
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `GroupTitlesReply` of `titles` — the answer to
    /// [`ServerEvent::RequestGroupTitles`](crate::ServerEvent::RequestGroupTitles).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_group_titles_reply(
        &mut self,
        group_id: GroupKey,
        request_id: Uuid,
        titles: &[GroupTitle],
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::GroupTitlesReply(GroupTitlesReply {
            agent_data: GroupTitlesReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
                group_id: group_id.uuid(),
                request_id,
            },
            group_data: titles
                .iter()
                .map(|title| GroupTitlesReplyGroupDataBlock {
                    title: with_nul(&title.title),
                    role_id: title.role_id.map_or_else(Uuid::nil, |role| role.uuid()),
                    selected: title.selected,
                })
                .collect(),
        });
        self.push_reliable(&reply, now)
    }

    /// Sends a `GroupNoticesListReply` of `notices` — the answer to
    /// [`ServerEvent::RequestGroupNotices`](crate::ServerEvent::RequestGroupNotices).
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCircuit`] if the circuit is not open, or a wire error
    /// if the message fails to encode.
    pub fn send_group_notices_list_reply(
        &mut self,
        group_id: GroupKey,
        notices: &[GroupNotice],
        now: Instant,
    ) -> Result<(), Error> {
        let reply = AnyMessage::GroupNoticesListReply(GroupNoticesListReply {
            agent_data: GroupNoticesListReplyAgentDataBlock {
                agent_id: self.agent_uuid(),
                group_id: group_id.uuid(),
            },
            data: notices
                .iter()
                .map(|notice| GroupNoticesListReplyDataBlock {
                    notice_id: notice.notice_id.uuid(),
                    timestamp: notice.timestamp,
                    from_name: with_nul(&notice.from_name),
                    subject: with_nul(&notice.subject),
                    has_attachment: notice.has_attachment,
                    asset_type: notice.asset_type,
                })
                .collect(),
        });
        self.push_reliable(&reply, now)
    }

    /// Routes the answer to a kept group invitation received over the
    /// `AcceptGroupInvite` / `DeclineGroupInvite` capability to the driver as
    /// [`ServerEvent::KeptGroupInvitationAnswered`](crate::ServerEvent::KeptGroupInvitationAnswered).
    pub(crate) fn push_kept_group_invitation_answer(&mut self, group_id: GroupKey, accept: bool) {
        self.push_server_event(crate::ServerEvent::KeptGroupInvitationAnswered {
            group_id,
            accept,
        });
    }
}

/// The body of a `GroupMemberData` request: the group it asks for. `None` for
/// a body that names none.
#[must_use]
pub fn group_member_data_request_from_llsd(body: &Llsd) -> Option<GroupKey> {
    match body.get("group_id")? {
        Llsd::Uuid(id) => Some(GroupKey::from(*id)),
        Llsd::String(text) => Uuid::parse_str(text).ok().map(GroupKey::from),
        _ => None,
    }
}

/// The body of a `GroupAPIv1` POST: what it does and to whom (inverse of
/// [`group_ban_request_body`](crate::group_ban_request_body)). `None` for a
/// body that is no ban request.
#[must_use]
pub fn group_ban_request_from_llsd(body: &Llsd) -> Option<(GroupBanAction, Vec<AgentKey>)> {
    let action = match body.get("ban_action").and_then(Llsd::as_i32)? {
        1 => GroupBanAction::Ban,
        2 => GroupBanAction::Unban,
        _ => return None,
    };
    let agents = body
        .get("ban_ids")
        .and_then(Llsd::as_array)?
        .iter()
        .filter_map(|id| match id {
            Llsd::Uuid(id) => Some(AgentKey::from(*id)),
            Llsd::String(text) => Uuid::parse_str(text).ok().map(AgentKey::from),
            _ => None,
        })
        .collect();
    Some((action, agents))
}

/// A `GroupAPIv1` ban list as Second Life answers it — `{ group_id, ban_list:
/// { <agent>: { ban_date } } }`, the `group_id` a string — the inverse of the
/// client's reading of it.
#[must_use]
pub fn group_bans_to_llsd(group_id: GroupKey, bans: &[GroupBan]) -> Llsd {
    let list = bans
        .iter()
        .map(|ban| {
            let mut entry = HashMap::new();
            if let Some(date) = &ban.ban_date {
                entry.insert("ban_date".to_owned(), Llsd::Date(date.clone()));
            }
            (ban.agent_id.to_string(), Llsd::Map(entry))
        })
        .collect();
    Llsd::Map(HashMap::from([
        ("group_id".to_owned(), Llsd::String(group_id.to_string())),
        ("ban_list".to_owned(), Llsd::Map(list)),
    ]))
}

/// The body of an `AcceptGroupInvite` / `DeclineGroupInvite` POST: the group
/// whose invitation it answers (inverse of
/// [`group_invite_response_body`](crate::group_invite_response_body)).
#[must_use]
pub fn group_invite_answer_from_llsd(body: &Llsd) -> Option<GroupKey> {
    match body.get("group")? {
        Llsd::Uuid(id) => Some(GroupKey::from(*id)),
        Llsd::String(text) => Uuid::parse_str(text).ok().map(GroupKey::from),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        group_ban_request_from_llsd, group_bans_to_llsd, group_invite_answer_from_llsd,
        group_member_data_request_from_llsd,
    };
    use crate::types::{GroupBan, GroupBanAction};
    use crate::{Event, group_ban_request_body, group_invite_response_body};
    use pretty_assertions::assert_eq;
    use sl_types::key::{AgentKey, GroupKey};
    use sl_wire::parse_llsd_xml;
    use uuid::Uuid;

    #[test]
    fn a_ban_request_reads_back_as_written() -> Result<(), String> {
        let banned = [
            AgentKey::from(Uuid::from_u128(7)),
            AgentKey::from(Uuid::from_u128(8)),
        ];
        for action in [GroupBanAction::Ban, GroupBanAction::Unban] {
            let body = parse_llsd_xml(&group_ban_request_body(action, &banned))
                .map_err(|error| error.to_string())?;
            assert_eq!(
                group_ban_request_from_llsd(&body),
                Some((action, banned.to_vec()))
            );
        }
        Ok(())
    }

    #[test]
    fn a_ban_list_reads_back_as_written() {
        let group = GroupKey::from(Uuid::from_u128(1));
        let bans = vec![GroupBan {
            agent_id: AgentKey::from(Uuid::from_u128(7)),
            ban_date: Some("2026-10-14T00:00:00Z".to_owned()),
        }];
        assert_eq!(
            crate::session::conversions::group_bans_from_caps_llsd(&group_bans_to_llsd(
                group, &bans
            )),
            Some(Event::GroupBans {
                group_id: group,
                bans
            })
        );
    }

    #[test]
    fn the_group_capability_bodies_read_back_as_written() -> Result<(), String> {
        let group = GroupKey::from(Uuid::from_u128(1));
        let invite = parse_llsd_xml(&group_invite_response_body(group))
            .map_err(|error| error.to_string())?;
        assert_eq!(group_invite_answer_from_llsd(&invite), Some(group));
        let members = parse_llsd_xml(&sl_wire::build_group_member_data_request(group.uuid()))
            .map_err(|error| error.to_string())?;
        assert_eq!(group_member_data_request_from_llsd(&members), Some(group));
        Ok(())
    }
}
