---
id: viewer-teleport-flow-progress
title: Teleport flow — progress screen & arrival
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-teleport-flow
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The user-facing teleport experience: **initiate** a teleport (from the map, a
landmark, a SLURL, or "teleport home"), the teleport **progress** screen with
its state messages and cancel, and the **arrival** hand-off when the destination
region comes up.

The teleport **protocol** is already done and tested (see the Phase-12 teleport
conformance cases — local, cross-region, failed, and offer / accept); this task
is the viewer flow + progress UI on top.

Note: the incoming teleport-**offer / lure** dialog is a case of the
notifications system and lives in [[viewer-dialog-offers-invites]], not here;
this task owns the progress UX only.

Reference (Firestorm, read-only): `llagent` (teleport request / state),
`llviewermessage` (`TeleportLocal` / `TeleportFinish` / lure handling),
`llstartup` (progress screen).

Builds on: the existing teleport protocol (Phase 12).

Deps: [[viewer-ui-notification-host]].

## Done (2026-08-07)

New `teleport_progress.rs`: a centred progress overlay driven by the teleport
events, showing the live phase, elapsed time, destination label, and the
simulator's progress messages. Beyond the reference (per the user's "never
hang" ask): a **soft watchdog** (18 s → "taking longer, you can cancel") and a
**hard watchdog** (38 s → force-fail *and* send `CancelTeleport` so the session
never sticks in `Teleporting`), plus **Cancel / Dismiss / Retry** buttons and a
prominent failure reason + `AlertInfo` detail. A shared `issue_teleport` backend
(+ a `BeginTeleportFlow` message) that the double-click / minimap / world-map
surfaces all route through. Verified live on OpenSim.

Emergent teleport-protocol fixes surfaced by live testing and landed here (all
sl-proto, unit-tested):

- **`teleport_to` supersedes an in-progress teleport** — cancels the old
  (`TeleportCancel`) then requests the new, instead of rejecting the second
  (the reference's "a new teleport replaces the current one"); fixes a rapid
  double-click-teleport stalling on an earlier pending one.
- **`begin_handover` promotes an existing child circuit** for a teleport to an
  **already-connected neighbour** (`CompleteAgentMovement` on it, no fresh
  `UseCircuitCode`) — the teleport counterpart of `promote_child_to_root`. This
  is what makes **cross-region teleport work at all**: previously it minted a
  fresh circuit the sim rejected → `HandshakeFailed` disconnect. A distant
  teleport still mints fresh.
- **`TeleportStart` / `TeleportProgress` guarded on `Teleporting`** — a seamless
  region **crossing** is a teleport under the hood and OpenSim sends those
  messages for it, but it must stay silent; guarding them (like `Finish` /
  `Local` / `Failed` already are) keeps a crossing from popping the progress
  window, mirroring the reference (which only shows the screen for a teleport it
  initiated). Verified live: crossings no longer show the overlay.
