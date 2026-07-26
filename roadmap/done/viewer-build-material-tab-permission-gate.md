---
id: viewer-build-material-tab-permission-gate
title: Build tools — gate the Material tab on modify permission (grey + notice)
topic: viewer
status: done
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

## Done

Both halves shipped; 736 viewer unit tests green, clippy clean.

**Modify gate (`edit_material.rs`):**

- New `MatControl` marker on every interactive material control (the legacy
  normal / specular / colour swatches, the PBR render-material / base / metallic
  / emissive / normal swatches + tints, the alpha / PBR-alpha combos, all legacy
  and PBR numeric / scalar fields, the double-sided toggle and the New / Save
  buttons). New `gate_material_controls` system pointer-disables them
  (`InteractionDisabled` + `Pickable::IGNORE`, and the shared
  `reflect_disabled_text_color` greys a disabled field's font) on the same gate
  the Texture tab uses — `representative_face(..).is_some()` AND
  `ObjectState::agent_can_modify(primary)` — applied on the enabled/disabled
  transition. The **row labels / values** already grey through the shared page
  walk (`grey_texture_tab`), which the Texture tab drives on the same condition,
  so `MatControl` owns only the interaction-disable.
- Every object-modifying commit path is gated behind a new
  `material_edit_allowed` helper (`EditPerm::Modify` on the agent-relative
  `update_flags`, posting the shared `perm_notice` / `chat::LocalChatNotice`):
  `commit_legacy_fields`, `apply_alpha_mode_change`,
  `apply_normal_specular_picked`, `apply_spec_color_picked`,
  `apply_pbr_material_picked`, `commit_pbr_fields`, `commit_pbr_scalars`,
  `apply_pbr_texture_picked`, `apply_pbr_tint_picked`, `apply_pbr_alpha_change`,
  `handle_double_sided_press`, `handle_pbr_new_press`. Belt-and-braces behind
  the greying (a disabled control cannot fire), for the edge where a
  picker/field outlives a selection change to a non-modifiable object.
  `handle_pbr_save_press` (an inventory upload, not an object modify) keeps no
  notice — greying disables it.
- The object's Blinn-Phong material still **renders** on a non-modifiable object
  (that rendering is perm-independent and untouched); only editing is gated
  (user decision, 2026-07-26).

**Blinn-Phong / PBR mode switch → tab strip:** the `matmedia` combo is now a
`crate::ui_tab` `spawn_tab_strip` (`TabPlacement::BlockStart`), reading like the
build floater's aspect tabs. Its `TabStrip::active` replaces the combo's index
in `read_material_mode` / `auto_select_material_mode` (which now writes the
strip's `active` directly). Because the tab widget only reconciled its highlight
on the click / arrow path, a new `ui_tab::apply_programmatic_tab_selection`
system reconciles `Checked` / highlight / panel visibility whenever
`TabStrip::active` is set programmatically (the shared `reconcile_tab_selection`
extracted from `on_tab_value_change`) — the enabler the material auto-select
needs. The strip carries **no** `TexControl`: switching mode to *view* an
object's values is navigation like the shell tabs, so it stays usable on a
non-modifiable object (user decision, 2026-07-26). The Blinn-Phong live-preview
mechanics (`drive_legacy_preview` / `revert_legacy_preview`, keyed off
`mode.is_material()`) survive the conversion unchanged.

Not live-verified this pass (mechanical UI gate + widget swap; deferred to a
build-window live session).
