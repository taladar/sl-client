---
id: api-g11
title: Gestures (active-state)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G11 — Gestures (active-state)

`ActivateGestures`/`DeactivateGestures` (the gesture asset is already uploadable
via `UpdateInventoryAsset`; this toggles which are active). OpenSim-testable.

- [x] G11 gesture activate/deactivate. New type `GestureActivation`
  (`item_id`/`asset_id`) in `sl-proto/src/types/inventory.rs`. Commands
  `ActivateGestures { gestures: Vec<GestureActivation> }` and
  `DeactivateGestures { item_ids: Vec<Uuid> }`; circuit encoders
  (`send_activate_gestures`/`send_deactivate_gestures`, both `flags`/
  `gesture_flags` fixed at 0 as the viewer always sends) + `Session` methods
  (`activate_gestures`/`deactivate_gestures`). Both are fire-and-forget (no
  reply, hence no client `Event`). Server: each surfaces as a matching
  `ServerEvent::ActivateGestures`/`DeactivateGestures` in the `SimSession`
  dispatch. Wired through both runtimes + REPL (2 commands `activate_gestures`/
  `deactivate_gestures` + `format.rs` names). Tests: 2 lifecycle client (each
  request packs), 1 loopback round-trip (both surface server events), 2 REPL
  registry. Book: `content/appearance.md` gained a "Gestures" section + "In this
  codebase" entry. OpenSim-testable but NOT live-tested this session (loopback +
  lifecycle cover both directions).
