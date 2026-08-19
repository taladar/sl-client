---
id: viewer-build-widget-parity-upgrades
title: Build tabs — retire the stale widget deviations
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-ui-combo-widget,
       viewer-ui-color-picker, viewer-ui-texture-picker]
---

Context: [context/viewer.md](../context/viewer.md).

The build parameter tabs still carry deviations recorded when the
widget library was younger (module doc of
`sl-client-bevy-viewer/src/edit_params.rs`), and every original
blocker is done now:

- Replace the prim-type, hollow-shape, and material **cycle buttons**
  with real combos matching the reference's `comboBaseType`, hole-shape
  combo, and `material` combo — [[viewer-ui-combo-widget]] is done and
  the Texture tab already uses it.
- Replace the light colour's three numeric sRGB fields
  (`build-light-red/green/blue`) with the reference's **colour swatch**
  — [[viewer-ui-color-picker]] is done and the Texture tab's face
  colour already uses it.
- Add the spotlight **projector texture picker** (create or clear a
  projector, not just edit an existing projector's FOV / Focus /
  Ambiance, which we already support) — [[viewer-ui-texture-picker]]
  is done.
- Open the **group picker** for the General tab's "set group" instead
  of cycling through the agent's groups.

No new protocol work: all four commit through the existing
ObjectShape / ObjectExtraParams / ObjectMaterial / ObjectGroup spines
of [[viewer-prim-parameter-editing]].

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (comboBaseType /
hole / material combos, colorswatch + projector texture_picker
L2964-2977, button set group L1143), `indra/newview/llpanelobject.cpp`,
`indra/newview/llpanelvolume.cpp`.
