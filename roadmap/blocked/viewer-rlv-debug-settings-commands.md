---
id: viewer-rlv-debug-settings-commands
title: RLV — @setdebug_*/@getdebug_* allowlist and @setrot
topic: viewer
status: blocked
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-preferences-debug-settings-editor]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

The RlvExtGetSet extension family (`rlvextensions.cpp`):
`@setdebug_<setting>:<value>=force` and `@getdebug_<setting>=<channel>`
expose a small allowlisted set of debug settings to scripts — AvatarSex
(read/write, pseudo-setting), AspectRatio (read, pseudo-setting),
RenderResolutionDivisor (read/write), plus read-only
RestrainedLoveForbidGiveToRLV, RestrainedLoveNoSetEnv and
WindLightUseAtmosShaders — while the `@setdebug=n` gate locks user edits
of those settings. The same module implements `@setrot:<radians>=force`,
which rotates the avatar to face a heading.

Like the environment family ([[viewer-rlv-environment-commands]]), these
are extension-prefix commands outside the behaviour dictionary: Firestorm
routes them through the `RlvExtCommandHandler` fallback, and our parser
yields `RlvBehaviour::Unknown` with the raw keyword kept. We parse the
`setdebug` gate itself but have no prefix recognition, no allowlist, no
application to our settings store, and no `@setrot`. Scope: prefix
dispatch on Unknown keywords, the exact allowlist plus the pseudo-setting
semantics, wiring reads/writes into the debug-settings registry built by
[[viewer-preferences-debug-settings-editor]], replying on the requested
channel, and the forced-rotation action.

Reference (Firestorm, read-only): `indra/newview/rlvextensions.cpp`,
`indra/newview/rlvextensions.h`.
