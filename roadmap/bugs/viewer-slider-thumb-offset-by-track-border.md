---
id: viewer-slider-thumb-offset-by-track-border
title: A slider's thumb is drawn one track border width off the value the pointer set
topic: viewer
status: bugs
origin: viewer-slider-click-mirrored-rtl (2026-11-10)
refs: [viewer-slider-click-mirrored-rtl]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The shared slider (`sl-viewer-ui-widgets/src/ui_slider.rs`) places its thumb
by an absolute `inline_start` inset of `fraction * travel`, where
`SliderStyle::travel` is the track's width less the thumb's. An absolute inset
is measured from the track's **padding box**, inside its border, so the thumb
at the minimum starts one border width in from the track's edge, and at the
maximum it overhangs the trailing border by the same width.

The core slider (`bevy_ui_widgets`) converts a pointer into a value against
the track's **border box** with the same travel, so the two agree on scale but
not on origin: after a snapping click the thumb's centre sits one border width
(2 px in the test style) toward the trailing end of the pointer, in either
direction. Measured by `a_track_click_sets_the_value_drawn_under_the_pointer`,
which allows for it.

## What to do

Make the thumb travel inside the track's interior and the pointer map onto the
same box: the travel becomes the track's width less both borders and the
thumb, and either the core slider measures the content box (a fork change,
upstreamable, since its doc already asks the stylist to keep the thumb inside
the track) or the widget offsets the value on the way in. Then tighten the
click test's tolerance to a pixel.

## Done when

A thumb never overhangs its track's border, and after a click it is centred
under the pointer to within a pixel, in both directions.
