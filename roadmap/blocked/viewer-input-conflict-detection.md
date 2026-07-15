---
id: viewer-input-conflict-detection
title: Key-binding conflict detection
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-rebinding-persistence]
---

Context: [context/viewer.md](../context/viewer.md).

Detect and report key-binding conflicts. A **conflict** is one key bound to
**two actions in the same context**. Multiple keys on one action is explicitly
**allowed** and is *not* a conflict (see the many-to-one rule in
[[viewer-input-action-map]]). Conflicts across different contexts are also not
conflicts, since only one context's profile is live at a time.

Surfaced to the user by [[viewer-input-rebinding-ui]] (the warn-before-overwrite
flow reads this).

Reference (Firestorm, read-only): `llkeyconflict`.
