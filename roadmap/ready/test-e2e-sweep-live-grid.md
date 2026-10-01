---
id: test-e2e-sweep-live-grid
title: End-to-end tests for what only a live grid has
topic: test
status: ready
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 13
refs: [test-e2e-live-verify-sweep, test-e2e-live-grids]
---

Context: [context/automation.md](../context/automation.md).

Checks that need an estate owner, SL-only features, real content or real
failures. Each becomes a stage test that declares what it needs
(`Need::…`) and skips elsewhere, run by hand with
`SL_E2E_GRID=opensim|aditi` under nextest's `live` profile.

OpenSim, as the estate owner:

- [[viewer-region-options-estate]] and [[viewer-region-options-terrain]]:
  the write paths.
- [[viewer-terrain-edit-brushes]] and [[viewer-terrain-edit-bake-revert]]:
  a stroke moves ground, drag-select snaps, bake → raise → revert.
- [[viewer-parcel-join-split]]: subdivide and join.
- [[viewer-avatar-ground-from-collision-plane]]: flat ground, a slope, a
  prim floor, a ramp (both grids).
- The `api-g*` batches ([[api-g6]] and
  their siblings) — each "not live-tested", most testable on OpenSim with
  XEngine, Groups V2 and a money module.

aditi:

- [[viewer-conference-start-ui]]: an ad-hoc conference whose peers get the
  invitation and a line crosses.
- [[chat-group-history-server-side]]: the history fetch, and whether
  conferences return one.
- [[viewer-seated-region-crossing]]: a scripted vehicle over a border.
- [[viewer-experience-permission-dialog]] and
  [[protocol-27]]: an experience `ScriptQuestion`.
- [[viewer-inventory-new-wearables]]: `NewFileAgentInventory` for a
  wearable.
- [[viewer-environment-settings-index]]: the library's Sunrise preset and
  day cycle resolve different assets.
- [[viewer-region-environment-panel]]: the per-track `trackno` path.
- [[viewer-teleport-never-resets-the-world]]: a distant teleport purges, a
  neighbour step keeps.
- [[protocol-7]]: the SL-only capability events.
- [[test-group-notice]]: a group notice
  between two aditi avatars.
- [[test-e2e-live-grids]]: the MFA challenge path, when one is issued.

Either grid, from real objects and real failures:

- [[viewer-rlv-command-intake]] and the RLV family: a worn object's `@`
  commands, a collar's `@version` handshake, the expiry pass on a derezzed
  prim.
- [[viewer-p27-1]]: real PBR content.
- [[viewer-ecs-idiom-audit]]: particle emitters that must not flicker.
- [[viewer-perf-custom-static-raycast-index]]: two colliding physical
  prims' sound.
- [[viewer-asset-retry-counter-stuck]]: a fetch that fails transiently,
  retries and then succeeds — needs a real 503; watch any aditi run's log for
  `scheduling retry` without a `gave up` for that id.

Not automatable here and left with a person:
[[viewer-audit-audio-mute-and-device]]'s audio hot-plug,
[[viewer-ui-text-ime-verification]] (needs an input method),
[[viewer-automation-accesskit-bridge]]'s Orca pass,
[[viewer-avatar-face-bone-shape-brow-spike]] (blocked on Reset Skeleton),
[[viewer-avatar-state-dump-replay]]'s full capture run,
[[test-firestorm-harness-skin-selection]] and [[test-kick-user]]'s second
concurrent login.
