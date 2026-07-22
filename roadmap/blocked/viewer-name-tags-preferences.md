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
([[viewer-preferences-floater]]) — the full toggle set from the 2026-07-22
nametag survey: the tag-mode selector (off / on / small), show own tag,
display-name vs username vs legacy-name line choices, group title on/off,
typing-state line, the distance / complexity display toggles
([[viewer-name-tags-complexity-distance]] consumes them), fade time +
fade duration, the legacy fixed-position option and the Z-offset
correction. These bind the toggles the renderer and decorations already
honour to persisted settings, rather than the enabled-by-default /
env-gated behaviour they ship with.

Reference (Firestorm, read-only): the name-tag section of the reference
preferences (`llfloaterpreference`).

Deps: [[viewer-preferences-floater]] (the settings shell the toggles live in).
