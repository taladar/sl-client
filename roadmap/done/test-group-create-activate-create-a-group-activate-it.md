---
id: test-group-create-activate
title: create a group, activate it
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 6 — Groups `[both]`
---

Context: [context/test.md](../context/test.md).

OpenSim requires Groups V2 enabled (see appendix).

`group-create-activate` — create a group, activate it. `1av`. The group
lifecycle entry point: `CreateGroup` makes the primary the founder/owner,
then `ActivateGroup` sets the agent's active group. OpenSim auto-activates a
group at creation time (`GroupsService.CreateGroup` stamps the founder's
principal record with the new group as active), so a bare "activate then check
active == group" would not exercise the command — creation already left it
active. To make the activation a genuine, observable transition, the case
first *clears* the active group with `ActivateGroup(None)` and confirms the
grid reports no active group, then activates the new group and confirms it is
reported active (`Event::ActiveGroupChanged`) with the founder's non-zero
powers and the group's name. This also drove an idiomatic API change:
`Command::ActivateGroup` / `Session::activate_group` now take an
`Option<GroupKey>` (`None` clears, sent as the nil group id on the wire),
mirroring the read side where `ActiveGroup::active_group_id` is already an
`Option`; the REPL gained an `Args::opt_uuid` so `activate_group` with no
argument clears. Green on OpenSim: create ≈ 0.39 s, clear ≈ 0.05 s, activate
≈ 0.06 s loopback; owner powers `0x000ffffffffffffe`. `[both]`; the Aditi run
is deferred with the batch (no aditi record this session).
