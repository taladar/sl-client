---
id: viewer-opensim-region-extras-limits
title: Consume the remaining OpenSimExtras region features
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-opensim-region-settings-panel, viewer-login-screen,
viewer-os-slurl-handler-linux, viewer-edit-permission-gating,
viewer-prim-parameter-editing]
---

Context: [context/viewer.md](../context/viewer.md).

Our `SimulatorFeatures` decode already carries the whole `OpenSimExtras`
bag (`sl-wire/src/sim_features.rs`), and the chat ranges and map/search
URL overrides are consumed, but three OpenSim-grid affordances still have
no consumer. (1) Export permission: when `ExportSupported` is true the
reference (`lfsimfeaturehandler.cpp`) shows an Export checkbox next to
Copy/Modify/Transfer in the permissions UI and honours `PERM_EXPORT`
(bit 1<<16, which `sl-wire/src/permissions.rs` already defines) on items
and objects — gating ties into [[viewer-edit-permission-gating]].
(2) Prim scale limits: `MinPrimScale` / `MaxPrimScale` /
`MaxPhysPrimScale` replace the SL constants (0.01–64 m) as the clamp for
build-tool size spinners and stretch gizmos on grids that raise them
(an Aurora-derived extension; our clamps live in the
[[viewer-prim-parameter-editing]] surface). (3) HyperGrid SLURL base:
`GridURL` (or the gatekeeper) yields the `hop://grid/region/x/y/z` prefix
used when composing shareable location links on OpenSim, instead of a
secondlife:// SLURL — [[viewer-os-slurl-handler-linux]] only registers us
as a handler, and grid identity comes from the
[[viewer-login-screen]] grid manager. All three are OpenSim-only; SL
behaviour is unchanged. Region-limit display belongs with
[[viewer-opensim-region-settings-panel]].

Reference (Firestorm, read-only): `indra/newview/lfsimfeaturehandler.cpp`,
`indra/newview/lfsimfeaturehandler.h`.
