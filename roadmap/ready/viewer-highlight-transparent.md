---
id: viewer-highlight-transparent
title: Highlight Transparent view mode
topic: viewer
status: ready
origin: user request (2026-07-22), during the Vintage-parity coverage audit
blocked_by: [viewer-input-action-map]
refs: [viewer-render-type-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

View → Highlight Transparent (Ctrl+Alt+T): tint every transparent surface
translucent red so builders can *see* alpha faces — invisible walls,
oversized alpha planes, the face you are about to fail to click. The
reference tints any face whose render pass is alpha (including fully
transparent faces, which start rendering while the mode is on).

Scope: the global toggle (menu + keybind via
[[viewer-input-action-map]]), a material override path that renders
alpha-blend / alpha-masked / fully-invisible faces with the red tint while
active (including faces normally skipped as fully transparent), and
restoring cleanly on toggle-off. Distinct from the render-type masks
([[viewer-render-type-toggles]]) — this recolours rather than hides.

Reference (Firestorm, read-only): `LLDrawPoolAlpha::sShowDebugAlpha`,
`menu_viewer.xml` (Highlight Transparent).

Builds on: the material/mesh pipeline (`materials.rs`, alpha handling).
