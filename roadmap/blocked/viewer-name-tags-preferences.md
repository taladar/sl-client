---
id: viewer-name-tags-preferences
title: Name tags — preference toggles
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-name-tags
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Expose the name-tag **preferences** in the preferences floater
([[viewer-preferences-floater]]): show tags, show own tag, show display names,
and the distance limit. These bind the toggles the renderer and decorations
already honour (fade / hide-beyond-N cut-off, display-name-vs-legacy choice) to
persisted settings, rather than the enabled-by-default / env-gated behaviour
they ship with.

Reference (Firestorm, read-only): the name-tag section of the reference
preferences (`llfloaterpreference`).

Deps: [[viewer-preferences-floater]] (the settings shell the toggles live in).
