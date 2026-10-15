---
id: viewer-permission-active-grants
title: Active permission grants (review / revoke)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-permission-request-dialog]
---

Context: [context/viewer.md](../context/viewer.md).

Track which permissions are currently granted to which object, and let the user
**review and revoke** them — the release side of take-controls / camera grants.
This is the surface for releasing a script's controls or camera hold, and
addresses the known permission-clearing follow-up (permissions should not linger
invisibly after the granting object is gone or a teleport clears state).

Reads the grants registered by [[viewer-permission-request-dialog]] (including
the auto-grants).

Reference (Firestorm, read-only): the script-info / granted-permissions surface,
`llagent` control-flag release.

Audit 2026-10-15: the protocol half exists — `Session::granted_permissions`
mirrors each in-world grant and `Command::RevokeScriptPermissions` sends the
`RevokePermissions` release (exercised by `sl-repl` and the conformance case
`script_permissions`). No viewer surface reads or revokes them yet;
`sl-viewer-notices/src/script_permission.rs` points here.
