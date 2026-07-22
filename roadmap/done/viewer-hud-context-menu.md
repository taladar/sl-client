---
id: viewer-hud-context-menu
title: HUD context / pie menu entries
topic: viewer
status: done
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-radial-menu]
refs: [viewer-ui-context-menu, viewer-object-context-menu, viewer-attachment-context-menu, viewer-hud-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

The **entries** offered when a **HUD attachment** is the pick target: Touch,
Edit, Detach (to inventory), and the rest — distinct from the in-world object
menu ([[viewer-object-context-menu]]) because a HUD is screen-space and already
attached. HUD picking / clicking already exists; this task is the menu entries
and their dispatch.

Correction (2026-07-21): the reference has **no separate HUD menu** — a
right-click on any *own* attachment, HUD included, shows the
**attachment-self** pie (`lltoolpie.cpp`: `isAttachment()` →
`gPieMenuAttachmentSelf`; there is no `menu_hud.xml`). So the entry set here
is `menu_pie_attachment_self.xml` (shared with
[[viewer-attachment-context-menu]] — build it once), at the reference compass
positions, with not-yet-implemented entries declared greyed (the
`UNIMPLEMENTED` pattern from `src/avatar_menu.rs`) and the simple ones —
**Detach**, **Touch** — wired. The pie XMLs are shared by every skin (Vintage
overrides none), so `default/xui/en/` is authoritative. Ship the committed
address-table test (`…keeps_every_address`) in the same commit, per the
[[viewer-ui-radial-menu]] angular-stability rule.
[[viewer-hud-menu-reorder-when-implemented]] later re-lays it by meaning
(dropping the world-only entries a HUD can never use).

Rendered by either the radial ([[viewer-ui-radial-menu]]) or line
([[viewer-ui-context-menu]]) widget.

Reference (Firestorm, read-only): `menu_pie_attachment_self.xml`,
`lltoolpie.cpp` (dispatch), `llviewermenu.cpp` (handlers).

## Done (2026-07-22)

Shipped with [[viewer-attachment-context-menu]] as one attachment-self pie
(`sl-client-bevy-viewer/src/attachment_menu.rs`), exactly as the corrected
scope says. The right-click resolver no longer swallows a click over a HUD:
the same orthographic HUD-camera ray the left-click touch uses
(`ObjectPicker::pick_hud`, restricted to the HUD layer, shown geometry only
— so only *own* HUDs, which is what makes it always the self pie) resolves
the picked prim + surface and opens the attachment-self pie. **Drop** is
disabled on the HUD path (`TARGET_DROPPABLE` withheld — a HUD has no world
position to drop at); **Detach** and **Touch** work as on any own
attachment. Address table pinned by
`attachment_self_pie_keeps_every_address`.
