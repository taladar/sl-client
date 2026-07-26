---
id: viewer-build-material-tab-permission-gate
title: Build tools — gate the Material tab on modify permission (grey + notice)
topic: viewer
status: ready
origin: split from viewer-build-tool-modify-permission-gate (2026-07-26)
refs: [viewer-build-tool-modify-permission-gate, viewer-prim-material-editing]
---

Context: [context/viewer.md](../context/viewer.md).

Extend the build-window modify-permission gate
([[viewer-build-tool-modify-permission-gate]]) to the **Material** tab
(`edit_material.rs`), which was left out because — unlike the Object / Features
and Texture tabs — it has **no control-disable / greying infrastructure** (it
does not grey even on an empty selection) and drives edits through several
separate commit systems (`commit_legacy_fields`, `apply_alpha_mode_change`, the
PBR field / scalar commits, the texture-picker applies).

Wanted:

- Grey every Material-tab control (still showing values) when the primary
  selection is not modifiable — first the tab needs a control-disable gate at
  all (a `MatControl`-style marker walk, like the Texture tab's
  `grey_texture_tab`), then AND `ObjectState::agent_can_modify` into it.
- Block each Material commit path on no-modify and post the shared
  `Build Tools: you do not have permission to modify …` notice
  (`gizmos::perm_notice` / `EditPerm::Modify`).

Everything needed already exists: the `update_flags` permission helpers on
`ObjectState`, the `EditPerm` enum, `perm_notice`, and the
`chat::LocalChatNotice` overlay message.

While in the Material tab, also **turn the Blinn-Phong / PBR mode select box
into a tab widget**: the material-type mode switch (`MatModeState`, legacy
Blinn-Phong vs PBR) is currently a combo / select box, but it should read as a
tab strip (the reference viewer presents the material type as tabs, and it
matches the rest of the build window's tabbed shell). Reuse the existing
`crate::ui_tab` `spawn_tab_container` / `TabStrip` widget the build floater
already uses for its aspect tabs.
