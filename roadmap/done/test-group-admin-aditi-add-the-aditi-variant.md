---
id: test-group-admin-aditi
title: Group admin — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `group-admin` case (`[[test-group-admin]]`,
already green `[opensim]`): **green on aditi live** (2026-08-12, Phase Z
batch). Role create/assign/unassign round-trips work as on OpenSim; the
ejection's membership-drop confirmation uses the same per-grid
`support::confirm_group_departure` helper as `group-join-leave` (Second
Life sends no `AgentDropGroup`, so the refreshed membership list is the
signal). Shares the short-group-name and creation-retry fixes documented
in [[test-group-join-leave-aditi]].
