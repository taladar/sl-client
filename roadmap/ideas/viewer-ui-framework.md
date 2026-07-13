---
id: viewer-ui-framework
title: In-viewer UI / floater-panel-menu framework
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The shell every other panel needs: draggable / dockable windows (floaters),
panels, menus, buttons, lists, tree views, text input, tab containers, a
notifications host, and theming / skins. Today the viewer only has fixed
`bevy_ui` overlays (`chat.rs`, `diagnostics.rs`) — there is no real widget
framework, no windowing, and no text input.

This is the **pivotal architectural choice** the other UI stubs depend on:
decide the tech (`bevy_ui`, `bevy_egui` / `egui` overlay, or another Rust UI
layer) weighing immediate-mode vs retained, input focus/capture, world-vs-UI
event routing, and performance under many open panels.

**Skinning is an explicit open question for this stub:** evaluate whether we can
offer skin flexibility roughly comparable to the reference viewer — Firestorm's
XUI is data-driven XML layouts plus fully swappable themes/skins (default,
firestorm, vintage, starlight) — and decide whether that is feasible with our
chosen UI tech or too much work. Land on a concrete target: full data-driven
layouts + hot-swappable themes, vs. just a colour/font theme layer over
code-defined layouts, vs. no user skinning.

Reference (Firestorm, read-only): `indra/llui/` (239 files: `llfloater`,
`llpanel`, `llmenugl`, `lllayoutstack`, `lldockablefloater`, `llfolderview`),
`llfloaterreg`, and the XUI layouts + skins under `newview/skins/*/xui/` and
`newview/skins/*/themes/`.

Builds on: the current `bevy_ui` overlays. Supersedes the MVP "no non-quit UI"
non-goal.
