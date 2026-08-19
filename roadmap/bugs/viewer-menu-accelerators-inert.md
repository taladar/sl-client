---
id: viewer-menu-accelerators-inert
title: Menu accelerators are drawn but dead (Ctrl+P / Ctrl+T / Ctrl+F / Ctrl+U)
topic: viewer
status: bugs
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-input-modifier-chords, viewer-ui-menu-bar, viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

The menu bar draws accelerator labels via `MenuCommand::accel`, but the
rendering in `sl-client-bevy-viewer/src/menu.rs` is display-only ("The
accelerator drawn against the entry"); nothing dispatches a pressed chord
to the menu command it is drawn on. Chords that do work each have a
bespoke keyboard system elsewhere. The result: Ctrl+P (Preferences…,
`sl-client-bevy-viewer/src/menu_bar.rs` around line 163), Ctrl+T
(Conversations, line 187) and Ctrl+F (Search…, line 381) are drawn
against live menu commands with no keyboard handler at all — the label
promises a shortcut that does nothing. Ctrl+U (Upload ▸ Image…,
`sl-client-bevy-viewer/src/inventory_actions.rs` line 237) sits on a
greyed UNIMPLEMENTED entry today and will lie the same way once
[[viewer-image-upload]] lands.

Fix the class, not the instances: add a generic accelerator→menu-command
dispatcher that routes a pressed chord to the command its `.accel()` is
drawn against, honouring `enabled_when` and the world-keyboard focus
gate, so a drawn accelerator can never disagree with the keyboard again.
Every future `.accel()` then becomes live automatically, and the
existing bespoke chord handlers (Ctrl+Q, Ctrl+I, Ctrl+M, …) can collapse
onto it — [[viewer-input-modifier-chords]] is the action-map-level
version of the same consolidation.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_viewer.xml` (shortcut=),
`indra/llui/llmenugl.cpp` (accelerator dispatch).
