---
id: viewer-slider-editable-value-field
title: A slider's readout a user can type into, as the reference's LLSliderCtrl
topic: viewer
status: ideas
origin: viewer-sliders-show-no-value (2026-11-09)
refs: [viewer-sliders-show-no-value]
---

Context: [context/viewer.md](../context/viewer.md).

Every slider now shows its value, bounds and step
([[viewer-sliders-show-no-value]]), but the readout is read-only. The
reference's `LLSliderCtrl` pairs the track with an **editable** value field:
type a number, press Enter, and the slider (and its setting) take it, clamped
to the range and snapped to the step.

Phototools already shows the way for the rows the reference gives a spinner
(`SliderLabels::bounds_only` beside a bound spinner). The general version
would make the `beside` readout a numeric text field bound to the same value —
a `SliderReadout` variant spawning a `TextInputKind::Float` field, written by
`label_sliders` when it is not focused and committing through the slider's
`ValueChange` path when it is — so every caller gets it at once, as they got
the readout.
