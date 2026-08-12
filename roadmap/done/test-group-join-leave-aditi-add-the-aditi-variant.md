---
id: test-group-join-leave-aditi
title: Group join/leave — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `group-join-leave` case
(`[[test-group-join-leave]]`, already green `[opensim]`): **green on aditi
live** (2026-08-12, Phase Z batch) after two Second Life group findings
(both now handled in `support`):

- **SL group names are hard-capped at 35 characters**
  (`DB_GROUP_NAME_STR_LEN`) and a `CreateGroupRequest` over the limit is
  **silently discarded** — no `CreateGroupReply` at all. The cases now use
  short `"slc <tag> <millis>"` names, and `membership_group` retries a
  create with a per-attempt name (SL also silently drops back-to-back
  creates by one agent; orphan single-member groups purge on SL in
  ~24-48 h).
- **SL sends no `AgentDropGroup` for a voluntary leave.** The membership
  drop is confirmed per grid by the new `support::confirm_group_departure`
  helper — it accepts either the OpenSim `AgentDropGroup`
  ([`Event::DroppedFromGroup`]) or, the reference viewer's approach, a
  re-requested membership list ([`Command::RequestAgentDataUpdate`] →
  [`Event::GroupMemberships`]) that no longer contains the group.
