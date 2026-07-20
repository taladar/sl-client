---
id: viewer-ui-status-bar-parcel-icons
title: Status-area parcel permission icon art
topic: viewer
status: ready
origin: split from viewer-ui-status-bar (2026-07-20)
blocked_by: [viewer-ui-status-bar]
refs: [viewer-ui-status-bar]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-ui-status-bar]] shipped the parcel-permission logic and read-outs, but
— lacking bundled icon art — draws each permission as a single always-visible
**letter placeholder** (`V F P B S A D`) that brightens when the permission is
in force. This task replaces those placeholders with the real **icon**
presentation the reference viewer uses, matching
`LLStatusBar::updateParcelIcons` / `panel_status_bar.xml` (`Parcel_VoiceNo`,
`Parcel_FlyNo`, `Parcel_PushNo`, `Parcel_BuildNo`, `Parcel_ScriptsNo`,
`Parcel_SeeAVsOff`, `Parcel_Health`, …).

The wiring is already done in `src/status_bar.rs`: the per-icon state comes from
`ParcelIcons::shown`, and each icon is a `ParcelIcon`-tagged node updated by
`update_parcel_icons`. The work here is the **display swap**:

- Source or draw the seven icon glyphs (import the reference TGA art, or author
  equivalents), respecting the skin / theme system rather than a fixed colour.
- Show an icon only when its permission is in force (the reference behaviour),
  rather than the always-visible muted/bright letter, and keep the fixed-width /
  non-jitter layout the letters have now.
- Consider restoring the reference's show-when-restricted semantics vs. the
  interim always-visible block, and whether a tooltip names each icon.

Deferred siblings that could join or split further: the pathfinding-dirty /
-disabled icons (need SL navmesh state the viewer does not track yet), the
damage-% text, and the media / audio controls + bandwidth graph the status-area
task also left out.

Reference (Firestorm, read-only): `indra/newview/llstatusbar.{h,cpp}`
(`updateParcelIcons` / `layoutParcelIcons` / `EParcelIcon`);
`newview/skins/default/xui/en/panel_status_bar.xml`; the `Parcel_*` textures in
`newview/skins/default/textures/`.
