---
id: viewer-ui-rows-shorter-than-their-text
title: Rows shorter than their text, and ten slider thumbs hanging out of their tracks
topic: viewer
status: done
origin: surfaced by tightening OVERFLOW_EPSILON after the text measure was fixed
  (2026-09-14)
refs: [viewer-ui-test-harness, viewer-text-node-padding-measure]
---

Context: [context/viewer.md](../context/viewer.md).

`OVERFLOW_EPSILON` in `sl-viewer-testkit` was **6 logical px**, wide enough to
absorb the upstream measure error of [[viewer-text-node-padding-measure]]. With
that fixed, dropping it to **2** — what the remaining rounding actually
justifies — made the element and floater sweeps report 21 cells of small, real
overflows the allowance had been covering. They are fixed here, and the constant
is now 2.

## 1. A row shorter than one line of its own text

At 22 px UI text, three rows of the avatar radar were 22 px tall holding text
that lays out 27 px — 5 logical px of descender outside the row, at every scale
factor, plus nine cells escaping their rows with it.

The row height was a pixel constant while the text followed the swept font. It
is now a function of that font (`radar::row_height`), in the proportion the
table already had (22 px at its 13 px cell font), so a row is sized for the line
it holds. The live table's font is a constant today, so nothing moves there.

## 2. Ten slider thumbs hanging out of ten tracks

One node per cell in `preferences`, `quick-preferences` and `phototools`, at
`UiScale` 1.25 and 2 and at scale factors 1.5 and 2: a child the *same height*
as its parent, offset 3–4 physical px down.

It was every slider in the viewer. A thumb is absolutely positioned inside its
track, so it is placed within the track's **border**; each one had been given
the track's own height, which is its *border box*. Every thumb therefore
overhung the bottom of its track by exactly the border — 1 px on most, 2 px on
the preference panels, which is why only those three panels crossed a 2 px
threshold and only at fractional scales.

Ten copies, in seven crates, because **there was no slider widget**:
`bevy_ui_widgets` provides the behaviour and says drawing it is the stylist's
job, so every panel drew its own track, thumb, marker component and
`fraction * (track - thumb)` placement system. One defect, ten places, and it
survived because no single place looked wrong.

So the fix is the widget the workspace was missing:
`sl_viewer_ui_widgets::ui_slider` — `SliderStyle` (the geometry and colours),
`slider_track` / `slider_thumb` / `spawn_slider`, and one `place_slider_thumbs`
system in `PostUpdate` that puts every thumb where its value says. The thumb has
**no height**: both block insets are zero, so it stretches to the track's
interior, and there is one place for that to be true.

All ten call sites are ported — `preferences`, `quick_preferences`,
`phototools`, `volume_panel`, `parcel_audio`, `media_controls`, `edit_wearable`,
`edit_material_asset`, `environment::rows`, `land_environment` and the settings-
binding demo — each losing its thumb marker and its placement system. What a
panel still owns is what only it knows: the readout text, the observer, what the
value means.

`environment::rows::spawn_slider` became `spawn_slider_row`, which is what it
builds (caption + track + readout) and leaves the plain name to the widget.
