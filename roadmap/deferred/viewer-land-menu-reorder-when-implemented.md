---
id: viewer-land-menu-reorder-when-implemented
title: Re-lay the land pie by meaning once most actions are implemented
topic: viewer
status: deferred
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-land-context-menu]
refs: [viewer-ui-radial-menu, viewer-avatar-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-land-context-menu]] lays the land pie at the **reference's** compass
positions while most slices are greyed placeholders. Once most land actions
are real (about-land, buy flows, terraforming, build), **re-lay the pie by
meaning** rather than the reference's accidents — the same deliberate,
one-shot muscle-memory reset as
[[viewer-avatar-menu-reorder-when-implemented]]: a single reviewed commit
that also updates the committed address table
(`…keeps_every_address`), per the [[viewer-ui-radial-menu]]
angular-stability rule. Fold in the line-menu presentation of the same
entry tree at that point.
