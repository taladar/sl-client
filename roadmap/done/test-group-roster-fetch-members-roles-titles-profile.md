---
id: test-group-roster
title: fetch members / roles / titles / profile
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

`group-roster` — fetch members / roles / titles / profile. `1av`. The
**read** side of a group, complementing `group-create-activate`'s lifecycle
and `group-join-leave`'s membership churn: the five roster queries a viewer
issues on opening a group's profile floater — `RequestGroupProfile`
(`Event::GroupProfileReceived`), `RequestGroupMembers`
(`Event::GroupMembers`), `RequestGroupRoles` (`Event::GroupRoleData`),
`RequestGroupRoleMembers` (`Event::GroupRoleMembers`), and
`RequestGroupTitles` (`Event::GroupTitles`).
Rather than assert each reply in isolation, the case cross-checks them so the
run proves they describe the *same*, self-consistent group: the profile names
a founder and an owner role; the member roster must then carry that founder
flagged as an owner; the role list must contain that owner role; and the
role↔member pairings must pair the founder with the owner role — catching a
stale or mismatched roster, not merely an empty one. One title is the agent's
currently selected title. The group comes from `support::membership_group`
(index 0): a throwaway created per run on OpenSim (the primary becomes
founder/owner), or a reused pre-made group on Second Life (avoiding the
per-run L$100 and a founder slot); the case only reads the group, leaving it
exactly as found. On the created path the founder is the primary itself, so
the case also pins the reported founder to the primary's own agent id. Green
on OpenSim: 1 member, 3 default roles (Everyone/Officers/Owners), 2
role-member pairs, 1 selected title; profile ≈ 23 ms, members ≈ 15 ms, roles
≈ 51 ms, role-members ≈ 10 ms, titles ≈ 15 ms loopback. `[both]`; the Aditi
run is deferred with the batch (needs a configured pre-made group to avoid the
L$ cost; no aditi record this session).
