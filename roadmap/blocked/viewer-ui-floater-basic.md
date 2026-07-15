---
id: viewer-ui-floater-basic
title: Floater window manager (basic)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The floater window manager, basic tier: a draggable title-bar window with drag,
z-order, focus, and close. Nothing upstream has a floater manager — every SL
viewer hand-writes one — so this is ours to build on top of `bevy_ui`.

Per the cross-cutting layout convention, a floater **sizes to its content**
(clamped to min-content) and **reflows on a font-size or locale change** — no
fixed pixel rects — so a longer translated label or a larger UI font never
overflows or clips. Resize / minimize / dock / tear-off are the follow-on
[[viewer-ui-floater-resize-dock]].

Reference (Firestorm, read-only): `indra/llui/llfloater`, `llfloaterreg`.
Every panel/floater task ([[viewer-chat-history-panel]],
[[viewer-emoji-picker-floater]], the repointed panel ideas) builds on this.
