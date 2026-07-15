---
id: viewer-permission-active-grants
title: Active permission grants (review / revoke)
topic: viewer
status: blocked
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
