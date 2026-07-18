---
id: viewer-input-rebinding-persistence
title: Persist per-context key-binding overrides
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-action-map, viewer-ui-settings-store]
refs: [viewer-settings-account-scope-persist, viewer-ui-floater-persist-geometry]
---

Context: [context/viewer.md](../context/viewer.md).

Persist and reload the user's **per-context binding overrides**, layered over
the default profiles from [[viewer-input-action-map]] and stored in
[[viewer-ui-settings-store]]. A user rebinds within a context; only the deltas
from the defaults are saved, and defaults can evolve without clobbering user
edits.

**Per user = per (grid, avatar name).** Store the overrides in the `Account`
scope ([[viewer-settings-account-scope-persist]]), not the global one — the same
choice [[viewer-ui-floater-persist-geometry]] made for floater geometry, so each
character keeps its own bindings. (The reference keeps `keys.xml` per install;
we deviate, matching the rest of our per-avatar state.) Today the bindings all
use the built-in defaults and there is no editor UI yet
([[viewer-input-rebinding-ui]]), so nothing writes overrides — this task is the
load/save layer those edits will land in.

Reference (Firestorm, read-only): the user `keys.xml` override load/save in
`llviewerinput`.
