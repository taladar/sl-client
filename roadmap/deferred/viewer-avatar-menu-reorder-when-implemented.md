---
id: viewer-avatar-menu-reorder-when-implemented
title: Re-lay the avatar pie by meaning once most actions are implemented
topic: viewer
status: deferred
origin: viewer-avatar-context-menu review (2026-07)
blocked_by: [viewer-avatar-context-menu]
refs: [viewer-ui-radial-menu, viewer-object-context-menu, viewer-hud-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-avatar-context-menu]] laid the two avatar pies (self / other) at the
**reference viewer's** compass positions, so the muscle memory matches Firestorm
today. But most of those slices are disabled placeholders, and the reference
layout is itself compromised — it leans on nameless `More >` overflow that our
pie doctrine forbids, and it scatters related actions (Call / Share / Invite sit
three levels deep in the other-avatar `More >`) purely because the reference ran
out of its own eight slots.

Once most of the avatar actions are actually implemented — profile, pay, report,
call, share (give inventory), the moderation powers, outfit / appearance editing
— **re-lay both pies by meaning**, not by the reference's accidents:

- Promote the genuinely-common actions the reference buries (Call, Share) to the
  top ring; demote the debug entries (Textures / Dump XML / Reset) into a single
  honestly-named `Debug >` grouping or out of the avatar pie entirely.
- Reconsider the `More >`-in-reference-position slice (other-avatar south): name
  it for what it groups, per the no-overflow rule.
- Bring in the deep enumerations deferred the first time round only where they
  are real: the per-attachment-point `Detach >` / `HUD >` lists become runtime
  lists of what is actually worn (not static leaves); the clothing-layer
  `Take Off >` becomes the real worn-layer set.

**This is a deliberate, one-shot muscle-memory reset**, so it must be a single
reviewed commit that also updates the committed address tables
(`{self,other}_avatar_pie_keeps_every_address`) in `src/avatar_menu.rs` — moving
an entry is a conscious edit to those tables, never a silent side effect
([[viewer-ui-radial-menu]]'s angular-stability rule). Do it **once**, when the
menu is mostly real, rather than nudging positions every time one action lands.

Also fold in at that point: the **line-menu** presentation of the same entry
trees (both widgets should reach one entry model), and opening the avatar menu
from a radar / People-list row, not only the world name tag / body.
