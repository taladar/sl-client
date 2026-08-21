---
id: viewer-avatar-moderation-actions
title: Per-avatar parcel / estate moderation — the shared action layer
topic: viewer
status: blocked
origin: gap found (2026-08-21) while building viewer-radar-multi-select
blocked_by: [viewer-region-options-estate]
refs:
  [
    viewer-radar-multi-select,
    viewer-minimap-menu-avatar-actions,
    viewer-avatar-context-menu,
    viewer-god-tools,
    api-g17,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Every reference avatar menu — the pie, the radar's two row menus, the
minimap's More Options, the profile, the People panel — carries the same
five moderation entries pointed at whoever the menu is about:

- **Freeze** / **Parcel Eject** (`Avatar.Freeze` / `Avatar.Eject`, shown
  under the reference's `Avatar.VisibleFreezeEject`: you manage the
  parcel they are standing on).
- **Estate Kick** / **Estate Teleport Home** / **Estate Ban**
  (`Avatar.Kick` / `Avatar.TeleportHome` / `Avatar.EstateBan`, shown
  under `Avatar.VisibleKickTeleportHome`: you are an estate manager or a
  god).

**None of them is wired anywhere per avatar today.** The protocol half
exists — `Command::{FreezeUser, EjectUser, TeleportHomeUser, GodKickUser}`
are implemented in `sl-client-bevy`, and the estate ban / kick / teleport-home
paths are driven from the About Region estate tab
([[viewer-region-options-estate]]) against a name the *user typed*. What is
missing is the per-avatar layer every menu wants:

- **The two visibility predicates**, computed once and offered as menu
  conditions: "I can moderate the parcel this avatar is on" (parcel owner /
  group officer, from `SlAgentParcel` powers) and "I can moderate this
  estate" (estate manager or god). The reference recomputes these per menu
  open; ours should be one helper both the pie's condition set and the
  radar's `radar_menu_conditions` call.
- **One guarded request channel per action**, in the shape the rest of the
  avatar actions already use (`RequestBlock` / `RequestDerender` /
  `RequestRenderException`): the menus write it, one system applies it, and
  the confirmation notifications the catalogue already carries
  (`FreezeUser`, `UnFreezeUser`, `KickUser`, `EstateKickUser`,
  `EstateBanUser`, `EstateBanUserMultiple`, `EstateTeleportHomeUser`,
  `EstateTeleportHomeMultiple`) are asked before it is sent. Note the two
  *Multiple* notifications: the reference already expects these actions to
  arrive with a **list** of avatars, which is what the radar's
  multi-selection menu ([[viewer-radar-multi-select]]) hands it.

Then light the entries up **everywhere at once**, through that layer, never
per menu:

- the avatar pie, which carries Freeze today as a disabled placeholder
  (`avatar_menu.rs`, `UNIMPLEMENTED`);
- both radar row menus (`radar.rs` — the single-row and multi-selection
  shapes, whose module docs name this absence);
- the minimap's More Options ([[viewer-minimap-menu-avatar-actions]], whose
  Freeze / Eject / Estate bullets are this task);
- the profile floater and the People panel, wherever they offer avatar
  actions.

Reference (Firestorm, read-only): `llviewermenu.cpp`
(`handle_avatar_freeze` / `handle_avatar_eject` and the
`enable_freeze_eject` predicate), `llfloaterregioninfo.cpp` (the estate
side), `menu_fs_radar.xml`, `menu_fs_radar_multiselect.xml`,
`menu_pie_avatar_other.xml`.
