---
id: viewer-hud-menu-reorder-when-implemented
title: Re-lay the HUD pie by meaning once most actions are implemented
topic: viewer
status: deferred
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-hud-context-menu]
refs: [viewer-ui-radial-menu, viewer-avatar-menu-reorder-when-implemented, viewer-attachment-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-hud-context-menu]] lays the HUD pie at the **reference's** compass
positions (the reference shows the *attachment-self* pie for a HUD pick)
while most slices are greyed placeholders. Once most of its actions are
real, **re-lay the pie by meaning** — dropping the attachment-self entries
that never make sense for a screen-space HUD (Sit Here, Go-to-style world
actions) instead of keeping them greyed forever — the same deliberate,
one-shot muscle-memory reset as
[[viewer-avatar-menu-reorder-when-implemented]]: a single reviewed commit
that also updates the committed address table (`…keeps_every_address`), per
the [[viewer-ui-radial-menu]] angular-stability rule. Keep it consistent
with [[viewer-attachment-menu-reorder-when-implemented]], since the two
start from the same reference pie. Fold in the line-menu presentation of
the same entry tree at that point.
