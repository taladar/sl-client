---
id: viewer-audit-ui-widget-per-frame-writes
title: The colour picker writes unguarded every frame and defeats the layout gate
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-ui-widgets/src/ui_color_picker.rs:559`, `:562`, `:578` —
`sync_color_picker_visual` runs every frame in `Update` (registered
unconditionally at `:244`, no run condition, no `state.is_changed()` guard) and
writes `preview.0`, `original.0` and `inset.0.inline_start` with **no
`if != want` guard**, unlike every other widget in the crate.

`LogicalInset` is exactly what `ui.rs:538 ChangedLogicalBoxes` filters on, whose
doc says it exists "so an unchanged UI does not re-trigger layout every frame".
So three slider thumbs re-resolve their logical boxes on every frame of the
process, picker open or closed.

Same file, two more: `:523` — `let Some(open) = opens.read().last()` silently
**drops** every `OpenColorPicker` but the last in a frame, so two swatches
requesting in one frame leaves one requester with no answer; and `:243-247` —
the picker's four systems are an unordered tuple with no `.chain()` /
`.after()`, while every other module here orders explicitly
(`settings_binding.rs:194`, `ui_table.rs:1439`).

Adjacent, same class: `sl-viewer-ui-pie-menu/src/pie_menu.rs:1918-1921` —
`drive_pie_material` calls `materials.get_mut(&node.0)` and writes
`inner_radius` / `outer_radius` / `slot_states` / `highlighted` unconditionally
each frame while a pie exists, forcing a bind-group re-prepare. Bounded (a pie
is open briefly) but unguarded; and `sl-viewer-world-view/src/hud.rs:302` calls
`materials.get_mut` *before* its `!material.base.unlit` guard, so the guard
prevents the write but not the `AssetEvent::Modified`. `sky.rs:1456` has the
correct compare-then-`get_mut` idiom.

`ui_color_picker.rs` is 656 lines with **zero** tests; `byte()` (`:646`), the
slider-fraction-to-thumb-offset math (`:569-575`) and
`ColorPickerState::current()` (`:191`) are pure, and the
Cancel-emits-`final_pick:false` contract (`:629`) is asserted only by a doc
comment.
