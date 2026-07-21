---
id: viewer-attachment-context-menu
title: Attachment context / pie menu entries (worn on self + on others)
topic: viewer
status: ready
origin: user request while reviewing the pie-menu cluster (2026-07-21)
blocked_by: [viewer-avatar-context-menu]
refs: [viewer-object-context-menu, viewer-hud-context-menu, viewer-avatar-mesh-accurate-pick, viewer-attachment-menu-reorder-when-implemented]
---

Context: [context/viewer.md](../context/viewer.md).

The pie menus offered when a **worn attachment** is the pick target — one for
an attachment worn by **ourselves**, one for an attachment worn by **another
avatar** — following the [[viewer-avatar-context-menu]] pattern: the
**reference's** entry sets at the reference compass positions, unimplemented
entries declared greyed (`UNIMPLEMENTED` condition), the simple ones wired.

Reference entry sets (the pie XMLs are shared by every skin — the Vintage
skin overrides none — so `default/xui/en/` is authoritative):

- `menu_pie_attachment_self.xml`: a superset of the self-avatar pie — its
  top ring adds **Touch**, **Edit**, **Detach**, **Drop**, **Sit Here /
  Stand Up**, alongside Profile / Gestures / Appearance / Edit Outfit and
  the debug tails.
- `menu_pie_attachment_other.xml`: a superset of the other-avatar pie
  (Profile / Mute / Add / Pay / IM / More…) plus the object-ish extras
  (Inspect, Derender, Textures, …).

Wire now: **Detach** (the detach wire path already backs the self pie's
detach entries), **Drop**, and **Touch** where the touch path exists; the
avatar-derived slices reuse their existing avatar-pie dispatch. The rest
start greyed.

Picking: today a click on a worn **rigged** submesh resolves to the wearer's
avatar pie ([[viewer-avatar-mesh-accurate-pick]] tags submeshes with
`AvatarPickTarget`), and **rigid** attachments are not picked at all. This
task refines that dispatch to the reference's: an attachment pick resolves
the worn *object* (submesh → worn object → wearer), rigid attachments become
pickable, and the attachment pies (not the plain avatar pies) open — self vs
other decided by the wearer, as `lltoolpie.cpp` does (`isAttachment()` +
ownership → `gPieMenuAttachmentSelf` / `gMenuAttachmentOther`).

**Pin every entry's position** (the [[viewer-ui-radial-menu]]
angular-stability rule): ship the committed address-table tests (one per
pie, `…keeps_every_address`) in the same commit.

Follow-up: [[viewer-attachment-menu-reorder-when-implemented]] re-lays both
pies by meaning once most entries are real.

Reference (Firestorm, read-only): `menu_pie_attachment_self.xml`,
`menu_pie_attachment_other.xml`, `lltoolpie.cpp` (dispatch),
`llviewermenu.cpp` (handlers).
