---
id: viewer-build-tool-row-parity
title: Build floater tool row — Focus & grab tools, row parity
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-transform-gizmos, viewer-prim-linking,
       viewer-camera-focus-on-object, viewer-camera-controls-window]
---

Context: [context/viewer.md](../context/viewer.md).

The reference build floater's top row offers five tools; ours models
only the Edit sub-modes plus Create (`BUILD_TOOLS` in
`sl-client-bevy-viewer/src/edit_tool.rs`). Missing tools: the **Focus**
tool (`button focus` with the Zoom / Orbit / Pan radio group and the
zoom slider — it should drive the camera orbit/zoom code that already
exists from [[viewer-camera-focus-on-object]], surfaced as a build
tool), and the **Move (grab)** tool (`button move` with the Move /
Lift / Spin radio group: grab-drag an unlocked object without entering
full edit mode, via ObjectGrab / ObjectGrabUpdate / ObjectDeGrab — we
currently send only instantaneous grab/degrab for touch clicks in
`sl-client-bevy-viewer/src/hud_pick.rs`).

Row-parity extras on the Edit panel: in-floater **Link / Unlink**
buttons (`link_btn` / `unlink_btn` — the function exists via Ctrl+L /
Ctrl+Shift+L and the Build menu, `sl-client-bevy-viewer/src/
edit_link.rs`, but the buttons are absent), the FS **Edit axis at
root** pivot option (`FSBuildPrefs_ActualRoot` — rotate/stretch around
the root prim instead of the selection centre; we have no
pivot-at-root option), and optionally the FS collapse-to-tool-row
expander (`btnExpand`, cosmetic). Already implemented and out of
scope: the edit radios, EditLinkedParts, part prev/next, ScaleUniform,
snap + grid unit, selection summary, and the Ctrl / Ctrl+Shift held
chords.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L85-235,
317-330, 395-402, 3505), `indra/newview/lltoolfocus.cpp`,
`indra/newview/lltoolgrab.cpp`, `indra/newview/llfloatertools.cpp`.
