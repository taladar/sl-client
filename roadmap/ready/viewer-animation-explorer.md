---
id: viewer-animation-explorer
title: Animation explorer — what is animating whom
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's animation explorer: a live list of the animations currently
playing on the own avatar (and, in the reference, recently triggered ones) —
each row naming the animation asset, its priority, and the *object / agent
that triggered it* — with actions to **stop** a selected animation and to
**revoke** the triggering object's animation permission. The data all exists:
`AvatarAnimation` decode (`protocol-40` physical/event lists), the animation
registry in `animations.rs`, and the permission registry from the permission
roadmap (revoke path).

Useful against griefing (stop an unwanted animation and revoke its source)
and for creators (see priorities fight in real time).

Reference (Firestorm, read-only): `fsfloateranimationexplorer` /
`floater_animation_explorer.xml`.

Builds on: the animation registry + `protocol-40` decode + the permission
revoke command.
