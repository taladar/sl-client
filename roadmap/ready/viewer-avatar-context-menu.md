---
id: viewer-avatar-context-menu
title: Avatar context / pie menu entries (self + others)
topic: viewer
status: ready
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-radial-menu]
refs: [viewer-ui-context-menu, viewer-object-context-menu, viewer-social-profiles, viewer-avatar-radar]
---

Context: [context/viewer.md](../context/viewer.md).

The **entries** offered when an avatar is the pick target (right-click an
avatar, its name tag, or a radar / people-list row), and the dispatch of each.
The two entry trees **differ**:

- **Own avatar:** Stand up / Sit, Appearance / outfit, My Profile, Groups /
  Friends, Gestures, Take off / detach, … (self-directed actions).
- **Another avatar:** Profile ([[viewer-social-profiles]]), IM / Call, Add
  friend, Pay, **Share** (give an inventory item — the wire path is done), Block
  / mute, Report, and the moderation actions where permitted.

Rendered by either the radial ([[viewer-ui-radial-menu]]) or the line
([[viewer-ui-context-menu]]) widget, mirroring [[viewer-object-context-menu]];
this task is the entry tree and its dispatch, reading the pick / selected
avatar.

Reference (Firestorm, read-only): `menu_attachment_self.xml` /
`menu_attachment_other.xml`, `menu_avatar_self.xml` / `menu_avatar_other.xml`.
