---
id: viewer-input-rebinding-persistence
title: Persist per-context key-binding overrides
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-action-map, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

Persist and reload the user's **per-context binding overrides**, layered over
the default profiles from [[viewer-input-action-map]] and stored in
[[viewer-ui-settings-store]]. A user rebinds within a context; only the deltas
from the defaults are saved, and defaults can evolve without clobbering user
edits.

Reference (Firestorm, read-only): the user `keys.xml` override load/save in
`llviewerinput`.
