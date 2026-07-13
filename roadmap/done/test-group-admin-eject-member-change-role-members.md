---
id: test-group-admin
title: eject member / change role members
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-admin` — eject member / change role members. `2av`. The
**admin** side of a group, complementing `group-join-leave`'s self-churn and
`group-roster`'s read side: the **primary** owns the group and the
**secondary** — having joined the open-enrollment group — is the member it
acts on. `2av` is intrinsic: an owner cannot eject itself (it leaves instead),
and a self role-change would not exercise the cross-member path. Two halves,
each asserted against the grid's authoritative state rather than the
optimistic local cache. **Role change** (`Command::ChangeGroupRoleMembers`, a
`GroupRoleChanges`) draws no direct reply, so after assigning the secondary to
a non-owner assignable role — the stock "Officers" role, found as the role
whose id is neither the nil "Everyone" role nor the profile's owner role — the
case re-requests the role↔member pairings (`Event::GroupRoleMembers`) and
polls until the new pairing appears, then removes the assignment and polls
until it is gone (proving a real transition, not a one-way add). **Ejection**
(`Command::EjectGroupMembers`, an `EjectGroupMemberRequest`) is a two-event
transition like a voluntary leave: OpenSim replies to the ejector with
`EjectGroupMemberReply` (`Event::EjectGroupMemberResult { success }`) *and*
sends the ejectee `AgentDropGroup` (`Event::DroppedFromGroup`), the
membership-list update proving the member is genuinely out; the case asserts
both. The ejection also restores a reused pre-made group to its founder-only
state for the next run. The group comes from `support::membership_group`
(index 0): a throwaway created per run on OpenSim (the primary becomes
founder/owner, holding the `RoleAssignMember` + `MemberEject` powers), or a
reused pre-made group on Second Life. Green on OpenSim (local secondary
`avatar2`, Groups V2 enabled): create ≈ 0.43 s, role-add ≈ 48 ms,
role-remove ≈ 68 ms, eject ≈ 82 ms loopback. `[opensim]` only; the Aditi
variant — and a multi-member role/roster assertion that wants a **`3av`**
third avatar — is deferred to Phase Z pending more Aditi avatars (and a
configured pre-made group).
