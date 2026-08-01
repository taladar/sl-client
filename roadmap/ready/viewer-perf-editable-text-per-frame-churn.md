---
id: viewer-perf-editable-text-per-frame-churn
title: bevy_ui editable text re-measures every field every frame (upstream)
topic: viewer
status: ready
origin: SL_VIEWER_LOG_UI_DIRTY diagnosis during
  viewer-perf-ui-layout-per-frame-relayout (2026-08-01)
refs: [viewer-perf-ui-layout-per-frame-relayout]
---

Context: [context/viewer.md](../context/viewer.md).

Found with the `SL_VIEWER_LOG_UI_DIRTY=1` layout-gate diagnostic
(`ui_perf.rs`): **every `EditableText` field re-`set`s its `ContentSize`
every frame**, dirtying taffy and re-laying-out the field for nothing. The
viewer always has visible text inputs (the nearby-chat bar, the menu-bar
search), so this churn ran the full `ui_layout_system` walk every frame
— part of the pre-task per-frame floor, and it would have kept the new
layout gate from ever skipping.

Root cause, in bevy_ui 0.19.0 (`widget/text_input_layout.rs`):

- `update_editable_text_styles` and the input-field layout system iterate
  with `&mut EditableText` and dereference it for **every** field
  **every** frame (e.g. `editable_text.editor.get_scale()` through the
  `Mut`), so `Changed<EditableText>` is permanently true;
- `update_editable_text_content_size` triggers on
  `editable_text.is_changed()` and unconditionally
  `content_size.set(...)` — an **identical** measure each time (it
  derives only from `visible_lines` / `visible_width` / font metrics,
  never from the typed content).

Viewer-side mitigation (shipped with the layout gate): `ui_layout_dirty`
ignores `Changed<ContentSize>` on `With<EditableText>` nodes and watches
their real measure inputs (`TextFont` / `LineHeight` / `TextLayout`)
instead. The wasted per-field re-measure itself still happens whenever
the layout system runs.

Remaining work — fix it at the source, per the fork-upstream policy
(`sl-client-fork-upstream-for-upstream-bugs`):

- check bevy `main` first (the editable-text widget is new in 0.19 and
  actively worked on — the churn may already be fixed);
- otherwise patch bevy_ui: split the read-only path off the `&mut
  EditableText` queries (or use `bypass_change_detection` for the
  reads / same-value guards for the writes), and make
  `update_editable_text_content_size` compare the derived measure before
  `content_size.set(...)`;
- test with `SL_VIEWER_LOG_UI_DIRTY=1`: the steady-state `content-size`
  category must go quiet with text-input fields on screen and idle;
- submit upstream, then drop the `[patch.crates-io]` once released.
