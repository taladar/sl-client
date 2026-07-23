---
id: viewer-minimap-click-radar-select
title: Minimap single-click selects the avatar in the radar
topic: viewer
status: blocked
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: [viewer-avatar-radar]
refs: [viewer-minimap-avatar-dots]
---

Context: [context/viewer.md](../context/viewer.md).

The reference leaves single-left-click on a minimap dot as a TODO
("select the avatar in the nearby avatar list") — a candidate for us to
do better once [[viewer-avatar-radar]] exists: clicking a dot selects
that avatar in the radar (and the radar highlights the hovered /
selected dot back on the map — the selection-ring cue the dots task
deferred). The minimap's hover hit-testing (`MinimapState::dots`,
pick-radius resolution) is already in place to source the click target.
