---
id: viewer-menu-bar-fill-implemented-entries
title: Top menu bar — add entries for already-implemented features
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-menu-bar, viewer-snapshot-floater, viewer-social-profiles,
  viewer-inventory-worn-actions, viewer-sit-target-and-stand-button,
  viewer-camera-focus-on-object, viewer-chat-history-panel,
  viewer-social-people-panel, viewer-inventory-outfit-tab]
---

Context: [context/viewer.md](../context/viewer.md).

Our top menu bar (`sl-client-bevy-viewer/src/menu_bar.rs`,
[[viewer-ui-menu-bar]] done) deliberately shipped "names now, entries
as they land", each future entry to be wired by its owning feature
task. A batch of features are now DONE without any task owning their
menu entry — the feature works but has no reference-parity menu path.
Add the reference entries whose backing feature already exists:

- Avatar ▸ Profile (`ShowAgentProfile`) and Picks
  (`ShowAgentProfilePicks`) → `avatar_profile.rs`
  ([[viewer-social-profiles]] done).
- Avatar ▸ Now wearing… (`NowWearing`) → the inventory worn tab
  ([[viewer-inventory-outfit-tab]] done).
- Avatar ▸ Take off ▸ Clothes (13 wearable types + all,
  `Edit.TakeOff`) → `inventory_actions.rs` take-off
  ([[viewer-inventory-worn-actions]] done).
- Avatar ▸ Movement ▸ Sit Down / Stand Up (`Self.SitDown` /
  `Self.StandUp`) → `avatar_menu.rs` + `stand_stop_button.rs`
  ([[viewer-sit-target-and-stand-button]] done); Fly / Stop flying
  (`Agent.toggleFlying`) → `movement.rs`.
- Avatar ▸ Snapshot (`Floater.Show(snapshot)`) →
  `snapshot_floater.rs` ([[viewer-snapshot-floater]] done).
- Comm ▸ Chat… (nearby-chat floater) → `chat.rs` / `chat_input.rs` /
  `nearby_chat_bar.rs` ([[viewer-chat-history-panel]] done); Comm ▸
  People → `people.rs` ([[viewer-social-people-panel]] done).
- World ▸ Nearby Avatars → the people panel's nearby tab.
- Build ▸ Focus on Selection / Zoom to Selection
  (`Tools.LookAtSelection(focus|zoom)`) → the camera focus/alt-zoom
  machinery ([[viewer-camera-focus-on-object]] done).
- Build ▸ Object ▸ Duplicate (`Object.Duplicate`) → the shift-drag
  duplicate command path; Object ▸ Return (`Object.Return`) →
  `object_menu.rs` "return" (DerezObjects ReturnToOwner).

Each entry is a MenuItemDef plus an arm in `handle_top_menu_actions`
routing to the existing module — no new features, matching the
reference's menu layout (`menu_viewer.xml` L6–2508) and enable gates
(e.g. Take off enabled only when something is worn, Sit/Stand by
sit state, selection-dependent Build entries by selection).

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` L6–2508,
`indra/newview/llviewermenu.cpp` (the per-entry enable callbacks).
