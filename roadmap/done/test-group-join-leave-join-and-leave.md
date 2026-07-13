---
id: test-group-join-leave
title: join and leave
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-join-leave` — join and leave. `2av`. Plain membership churn,
the complement to `group-create-activate`'s founder lifecycle: the primary
owns a throwaway open-enrollment group while the **secondary** does the
join/leave the case actually tests. It must be the secondary, not the
primary: the founder is the group's last owner and a grid will not let the
last owner drop the group out from under it, so `2av` is intrinsic, not
incidental. Both ends are observable on OpenSim: a join replies
`JoinGroupReply` (`Event::JoinGroupResult { success }`); a leave is a
two-event transition — `GroupsModule.LeaveGroupRequest` sends
`LeaveGroupReply` (`Event::LeaveGroupResult { success }`) *and then*
`AgentDropGroup` (`Event::DroppedFromGroup`), the membership-list update
that proves the agent is genuinely out, not merely acked. The case asserts
both so the leave is a real transition rather than a bare reply. Green on
OpenSim (local secondary `avatar2`, Groups V2 enabled): create
≈ 0.20 s, join ≈ 0.12 s, leave ≈ 0.11 s loopback. **Pre-made-group reuse
(new support):** group creation on Second Life costs **L$100**, an emptied
group purges only ~48 h after dropping below two members, and the founder
holds a group slot per created group — so creating per run on SL spends L$
and marches the founder toward the ~42-group cap. The group cases that do not
themselves test creation therefore take their group(s) from
`support::membership_group`, which reuses pre-made groups listed (by
position) in a gitignored `fixtures.<grid>.toml` when present (the SL path)
and otherwise creates throwaways (the OpenSim default, free and disposable).
`group-join-leave` and `group-session-message` use the first fixture group;
`chat-invite-accept-decline` uses the first two (it needs two distinct pending
sessions). Each leaves any group it joined, so a reused fixture is restored to
its founder-only state for the next run (a fresh join is also what makes the
invitation case fire). The reuse path was verified end to end against two
primary-owned OpenSim groups, confirming join/leave/cleanup leave the
fixtures clean. Aditi deferred to Phase Z pending a second Aditi avatar and
configured pre-made groups.
