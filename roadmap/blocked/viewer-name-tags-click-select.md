---
id: viewer-name-tags-click-select
title: Name tags — click a tag to select the avatar
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-name-tags
blocked_by: [viewer-name-tags-billboard-render, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

Make the name tag a **click target** that selects the avatar it labels, wiring
the tag into the object-selection foundation ([[viewer-object-selection-core]]).
A click on the tag selects the corresponding avatar exactly as clicking the
avatar's body would, so the selection acts as the anchor for profile / IM /
track actions elsewhere.

Reference (Firestorm, read-only): `llhudnametag` (the tag's pick target),
`llvoavatar`.

Deps: [[viewer-name-tags-billboard-render]] (the tag to click) and
[[viewer-object-selection-core]] (what a click selects).
