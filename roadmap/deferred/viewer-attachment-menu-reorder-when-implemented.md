---
id: viewer-attachment-menu-reorder-when-implemented
title: Re-lay the attachment pies by meaning once most actions are implemented
topic: viewer
status: deferred
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-attachment-context-menu]
refs: [viewer-ui-radial-menu, viewer-avatar-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-attachment-context-menu]] lays the two attachment pies (worn on
self / on another avatar) at the **reference's** compass positions while
most slices are greyed placeholders. Once most attachment and
avatar-derived actions are real, **re-lay both pies by meaning** — the same
deliberate, one-shot muscle-memory reset as
[[viewer-avatar-menu-reorder-when-implemented]] (ideally in the same sweep,
since the attachment pies are supersets of the avatar pies and should stay
consistent with them): a single reviewed commit that also updates the
committed address tables (`…keeps_every_address`), per the
[[viewer-ui-radial-menu]] angular-stability rule. Fold in the line-menu
presentation of the same entry trees at that point.
