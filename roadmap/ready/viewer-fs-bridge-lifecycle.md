---
id: viewer-fs-bridge-lifecycle
title: Firestorm LSL bridge — create, attach, version and repair it
topic: viewer
status: ready
origin: user request (2026-07)
refs: [viewer-fs-bridge-protocol]
---

Context: [context/viewer.md](../context/viewer.md).

The **Firestorm LSL bridge** is a small scripted attachment the *viewer itself*
creates in the user's inventory and wears, so that it can ask the simulator
things only an in-world script can answer (and do things only a script can do).
It is plumbing, not content — but plumbing Firestorm builds, uploads, attaches
and version-manages entirely on its own, and every bridge-backed feature
([[viewer-fs-bridge-protocol]]) is dead without it.

This is **opt-in**: creating and wearing an object on the user's behalf is a
side effect a library must not perform unasked, so the whole lifecycle stays
behind a setting, off by default (the reference gates it behind a setting too).

Its lifecycle, from `fslslbridge.cpp`:

- **Where it lives.** Inventory folder `#LSL Bridge` (created if absent),
  holding an object named `#Firestorm LSL Bridge v<major>.<minor>` — currently
  `v2.29` (`FS_BRIDGE_NAME` + `FS_BRIDGE_MAJOR_VERSION` /
  `FS_BRIDGE_MINOR_VERSION`).
- **How it is made.** The viewer rezzes a prim in-world, uploads the bridge
  script into it from a `.lsltxt` shipped with the viewer, takes the object into
  `#LSL Bridge`, and attaches it to **attachment point 31** — `HUD Center 2`, a
  HUD point, so it is invisible and out of the way (a nice interaction with the
  Phase 35 HUD work: a HUD attachment we already classify and route). It cleans
  up the rezzed prim / older versions (`FSLSLBridge::cleanUpBridgeFolder`).
- **Version management.** On attach it reads the object's name / the version the
  script reports and **recreates the bridge from scratch** when it does not
  match the version the viewer expects, so an upgraded viewer silently rebuilds
  it.
  Older `v1.x` / lower-minor copies are detached and removed
  (`FS_MAX_MINOR_VERSION`, the major/minor `for` loops in `fslslbridge.cpp`).
- **Failure modes to respect.** Rez permission denied on the parcel (it needs
  somewhere it may rez); script-upload failures; a grid that does not support
  `llRequestURL` (the handshake depends on it — see
  [[viewer-fs-bridge-protocol]]); OpenSim, where Firestorm guards the whole path
  behind `#if OPENSIM`; and the bridge being detached by the user or by an RLV
  `@detach` restriction ([[viewer-rlv-enforce-send-side]] — Firestorm keeps
  `mAllowDetach` state for exactly this).

Every piece this needs already exists in the workspace protocol side — inventory
folder/item creation, script asset upload, `rez_object`, `attach_object`, take
to inventory — so this is mostly **orchestration plus a state machine**, and a
strong end-to-end exercise of them. It must also be usable **headless**: a bot
on `sl-client` wanting bridge-backed data (avatar Z-offsets, script info) needs
the same lifecycle without a window.

Sensibly staged: (1) detect an existing, current bridge and adopt it; (2) create
one when absent; (3) version-upgrade / repair; (4) settings and diagnostics
around it. Do (1) first — it is the common case and it is what makes the
protocol task testable.

Reference (Firestorm, read-only): `fslslbridge.cpp` / `fslslbridge.h`
(`FSLSLBridge`, `cleanUpBridgeFolder`, the version constants and loops).
