---
id: viewer-ui-radio-widget
title: Reusable radio-button widget (grouping container)
topic: viewer
status: done
origin: the Build Tools floater's Move / Rotate / Stretch mode switch was
  hand-rolled buttons; the reference uses a radio group, and no reusable radio
  widget existed yet (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-combo-widget, viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

A reusable **radio-button group**: a grouping container holding a set of
mutually-exclusive labelled options, exactly one selected, that emits the chosen
one. This is the reference viewer's `LLRadioGroup` — the control its build /
tool floaters use for a mode switch (`move` / `rotate` / `stretch`, the
focus / land radios) and preferences use for a small closed choice.

Assemble it from the `bevy_ui_widgets` primitives rather than from scratch, the
same way the tab widget does: a [`RadioGroup`] grouping container is the single
focus stop, each option a [`RadioButton`] child, and arrow keys move the
selection within the group (the WAI-ARIA radiogroup pattern). Follow the
scaffold conventions ([[viewer-ui-widget-scaffold]]) — content-driven
auto-layout so a long translated option label grows the group instead of
clipping, both a row and a column layout named by axis (the inline row mirrors
under RTL for free), and full `Tab` / arrow-key reachability.

The widget's value is a **selected index** (a stable key the caller maps to its
own enum), not a bare `Entity`, kept on a `RadioSelection` component that is the
single source of truth — the `Checked` flags and the filled-dot indicators are
derived from it, so nothing drifts, and a consumer that sets it from outside
(the build tool syncing its `EditTool`) drives the visuals through the same
path. Register it in the UI element registry ([[viewer-ui-test-harness]]) so it
inherits the whole check matrix.

Adopt it in the **Build Tools floater** ([[viewer-object-edit-floater-shell]]),
whose Move / Rotate / Stretch tool switch was three hand-rolled highlight
buttons — replace them with the radio group, which is what the reference does.

Reference (Firestorm, read-only): `indra/llui/llradiogroup.{h,cpp}`
(`LLRadioGroup`, `LLRadioCtrl`), `floater_tools.xml`'s `move_radio_group` /
`edit_radio_group`.

## Done

`src/ui_radio.rs` — `spawn_radio_group` builds a `bevy_ui_widgets::RadioGroup`
grouping container (the single `TabIndex` focus stop) with one `RadioButton`
per option, each a row of a `◉` / `○` indicator glyph and its label.
`RadioSelection { element, active }` on the group is the single source of truth;
`on_radio_value_change` (an observer per group, like the tab strip's) moves it
on a click or arrow key and emits a `UiAction`, and `apply_radio_selection`
(the `RadioWidgetPlugin`, on `Changed<RadioSelection>`) reconciles every item's
`Checked` marker and indicator glyph/colour — so an external writer sets
`active` and the visuals follow, no separate path. `RadioLayout::{Row,Column}`
picks the axis; `translate_labels` binds each label to its Fluent key like the
tab widget. Two gallery/registry elements (`radio-group-row`,
`radio-group-column`) sweep both layouts across the whole script / direction /
scale / font matrix.

The Build Tools floater (`src/edit_tool.rs`) now builds its Move / Rotate /
Stretch switch with `spawn_radio_group` (labels the existing `build-tool-*`
Fluent keys), marked `BuildToolRadio`; `sync_build_tool_from_radio` writes the
picked index into `EditToolState::tool` and `sync_radio_from_build_tool` writes
it back if the tool changes elsewhere. The old `BuildToolButton` /
`spawn_tool_button` / `update_tool_button_visuals` and their two tint colours
are gone, and the `build-tools` specimen uses the radio group too so the swept
shape matches the live floater. The transient `Ctrl` / `Ctrl+Shift` chord
preview stays a `held_override`, so the radio keeps showing the resting tool.
