---
id: viewer-sliders-show-no-value
title: A slider shows no value, and no bounds or step either
topic: viewer
status: bugs
origin: live check of viewer-render-resolution-divisor (2026-09-20)
refs: [viewer-render-resolution-divisor, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Found while sliding the new `RenderResolutionDivisor` control: **no slider in
this viewer displays the value it holds.** There is a track, a thumb and a
translated label, and nothing that says `2`, `0.7` or `512 m`. The user is
left reading the thumb's position against an unlabelled track and guessing —
which for an integer setting whose whole meaning is *which* integer (a
resolution divisor, a shadow-cascade count, a blur-iteration count) is not a
readable control at all.

Nor is the value the only thing missing: a slider shows **none of the numbers
that would let a user place the thumb** — not its **minimum**, not its
**maximum**, and not its **step**. The range and the increment are declared
(`SliderRange`, `SliderStep`) and consumed only by the drag arithmetic, so
"how far does this go, and what will one notch move it by" is answerable from
the source and from nowhere in the interface. An unlabelled track between two
unlabelled ends means even the *relative* reading — "about a third of the way
up" — resolves to no quantity.

This is not specific to the divisor row. `spawn_slider`
(`sl-viewer-ui-widgets/src/ui_slider.rs`) spawns a track and a thumb and
nothing else, and `place_slider_thumbs` moves the thumb; every caller —
`spawn_pref_slider` across all the preferences tabs, the quick-preferences
panel, phototools — inherits the gap.

The reference viewer's `slider` control carries its value in a text field beside
it (`LLSlider` inside `LLSliderCtrl`, which owns the label *and* an editable
value field), so this is a parity gap as well as a usability one.

## Scope

A readout belongs in the widget, not in each caller, so that the forty-odd
existing sliders get it at once:

- a text node spawned beside the track by `spawn_slider`, and a system that
  writes `SliderValue` into it in step with `place_slider_thumbs`;
- **the bounds and the step too** — end labels at the track's two ends from
  `SliderRange`, and the step wherever it is not obvious from them (a tooltip
  on the track, or as part of the readout: `4 (1–16, step 1)`). All three
  numbers are already declared on the slider's bundle; none of them reaches the
  eye. Whether the ends are always drawn or only for a slider whose range is
  not self-evident is a judgement to make while looking at a full tab, not
  here;
- **formatting is per-row, not global**: an integer setting wants `4`, a factor
  wants `0.70`, a distance wants `512 m`. That argues for a small format enum
  (or a formatter fn) on the slider's bundle, defaulting to something sane,
  rather than one `{value}` for everything — and the same formatter has to
  render the end labels, or the ends and the value disagree about units;
- the reference's editable field is a second step and need not come with this
  one; read-only is already the whole of the fix.

Per [context/parallel-work-plan.md](../context/parallel-work-plan.md) this is
**B**'s (the UI shell owns `sl-viewer-ui-widgets` and the preferences tabs).
