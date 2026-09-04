---
id: viewer-audit-ui-widget-per-frame-writes
title: The colour picker writes unguarded every frame and defeats the layout gate
topic: viewer
status: done
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

## Outcome (2026-09-04): the picker reconciles, and it is no longer untested

`sync_color_picker_visual` now compares before it writes — the preview fill, the
original fill and each thumb's `inline_start` — and returns immediately when the
picker is closed, which is most of the process's life. The thumb-offset maths
came out of the loop as `thumb_offset(value, range)` so it can be checked at the
ends and past them without standing up a slider.

One correction to the finding, in its favour and against it. The dirty
`LogicalInset` did make `resolve_logical_boxes` re-run for three nodes every
frame, as filed — but it stopped there: that resolver writes `Node` only on a
real difference, so taffy was never re-entered. The waste was the resolver's own
pass plus two `BackgroundColor` dirties per frame, not a re-layout. Small, then,
but constant and permanent, and the widget was the one place in the crate that
broke the compare-then-write convention every other widget keeps.

The dropped requests are now explicit: `handle_open_color_picker` takes the
**first** `OpenColorPicker` of the frame rather than the last, and warns about
the rest. One shared floater can only answer one requester, so somebody must
lose; what it must not do is silently drop the earliest click in favour of the
latest. The four systems are `.chain()`ed, which also gets the open handler's
`SliderValue` commands applied before the visual sync reads them — previously
the thumbs sat a frame behind the colour the picker opened on.

Adjacent, as filed: `drive_pie_material` builds its `PieParams` and writes them
through only on a difference (`get_mut` on an `Assets` entry raises
`AssetEvent::Modified` regardless, which re-prepares the bind group), and
`apply_hud_fullbright` asks `get(…)` whether the face is already fullbright
before reaching for `get_mut` — its query fires on a *layer* change too, and by
then the material is usually already unlit.

The file had no tests; it has eleven. Pure: `byte` rounding and clamping,
`ColorPickerState::current`, `thumb_offset` (both ends, past both ends, and a
degenerate range). Driven, on a headless app with synthesised presses: a swatch
opens the picker on its own colour, a disabled swatch does not, the first of two
requests in a frame wins, a drag previews without committing and moves only its
own thumb, **OK** commits and **Cancel** hands back the original with
`final_pick: false`, and — the regression guard for this bug — an open picker
sitting still re-marks no inset and no fill across five frames.

Gotcha worth keeping: `ValueChange { value: 200.0, .. }` in a test infers
`ValueChange<f64>`, which matches no observer and fails silently. The literal
has to say `_f32`.
