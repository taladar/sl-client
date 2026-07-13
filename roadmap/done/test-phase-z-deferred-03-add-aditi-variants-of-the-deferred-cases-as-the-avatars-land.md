---
id: test-phase-z-deferred-03
title: Add [aditi] variants of the deferred cases as the avatars land
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

Done as a planning task: the avatars have landed (see
[[test-phase-z-deferred-01]] / [[test-phase-z-deferred-02]]), so this umbrella
was decomposed into one `ready` task per deferred case —
`test-<case>-aditi` (e.g. [[test-im-1to1-aditi]], [[test-group-admin-aditi]]).
Each flips its case's `grids()` to include `Grid::Aditi` and runs it live.

Add `[aditi]` variants of the deferred cases as the avatars land.

Single-avatar SL-behaviour blocker (not multi-avatar, parked separately in
[[test-phase-z-deferred-04]]): `script-upload` / `script-running` stay
`[opensim]`-only pending a viewer packet capture, not an avatar.
