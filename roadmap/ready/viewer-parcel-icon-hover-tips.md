---
id: viewer-parcel-icon-hover-tips
title: Hover-tips over the parcel permission icons in the top bar
topic: viewer
status: ready
origin: user request (2026-07-23)
---

Context: [context/viewer.md](../context/viewer.md).

The parcel-permission icons on the top status bar (no-build, no-fly,
no-scripts, voice, damage, …) are glyphs only; add **hover tooltips**
naming each restriction, like the reference viewer's status-bar icon
tooltips (e.g. "Building/dropping objects is not allowed here"). Use the
reference's wording as the source strings and route them through Fluent
like the rest of the UI text (they are user-visible strings, so they are
translation targets too).

Shape: a small shared hover-tip affordance would serve other top-bar
elements as well (balance, time, FPS) — check whether the widget scaffold
already has a tooltip primitive or whether this task introduces it.
