---
id: viewer-object-menu-reorder-when-implemented
title: Re-lay the object pie by meaning once most actions are implemented
topic: viewer
status: deferred
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-object-context-menu]
refs: [viewer-ui-radial-menu, viewer-avatar-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-object-context-menu]] lays the object pie at the **reference's**
compass positions while most slices are greyed placeholders (the reference
object pie is also the worst `More >`-overflow offender — take/buy/pay/edit
plus deep script/pathfinding/derender tails). Once most object actions are
real, **re-lay the pie by meaning** — the same deliberate, one-shot
muscle-memory reset as [[viewer-avatar-menu-reorder-when-implemented]]: a
single reviewed commit that also updates the committed address table
(`…keeps_every_address`), per the [[viewer-ui-radial-menu]]
angular-stability rule. Fold in the line-menu presentation of the same
entry tree at that point, and turn the attach-point enumerations into
runtime lists rather than static leaves.
