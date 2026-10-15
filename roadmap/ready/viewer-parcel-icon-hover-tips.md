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

Audit 2026-10-15: still open. The icons are spawned by `spawn_parcel_icon`
in `sl-client-bevy-viewer/src/status_bar.rs` with no tip. There is no
UI-widget tooltip primitive yet: the only tooltip is the in-world hover
tip (`sl-viewer-world-view/src/hover_tooltip.rs`), already skinned through
the `.sk-tooltip` roles ([[viewer-skin-tooltip-roles]]) — reuse its look
for the new widget-level tip.
