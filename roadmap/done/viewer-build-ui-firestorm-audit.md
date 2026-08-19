---
id: viewer-build-ui-firestorm-audit
title: Systematic side-by-side audit of the whole build UI vs Firestorm
topic: viewer
status: done
origin: user request (2026-07-24) after the build-tool texture / face-selection
  work — many small behaviours only surfaced by direct comparison
refs: [viewer-object-edit-floater-shell, viewer-prim-parameter-editing,
  viewer-prim-texture-editing, viewer-edit-face-selection, viewer-face-materials-pbr]
---

Context: [context/viewer.md](../context/viewer.md).

Once the build tool's major surfaces exist, do a **methodical,
control-by-control comparison of the entire Build Tools floater against
Firestorm**, running both side by side, rather than reactively fixing what a
live test happens to trip on.
The face-selection / texture work showed how many behaviours (channel
precedence, live-preview lifecycle, selection edge cases, click-through,
revert-on-close) only surface by direct comparison.

Walk every tab and control and record each divergence as its own roadmap item:

- **Tool row / focus / grid**: Focus / Move / Edit / Create / Land radios and
  their sub-modes; the grid-options popup; snap / units; ruler modes; the
  coordinate read-outs and their edit behaviour.
- **General (`llpanelpermissions`)**: name / description, creator / owner /
  group, the permission matrix (next-owner / group / anyone), sale type & price,
  "for sale", "show in search", click-action, the "You can…" summary, deed.
- **Object (`llpanelobject`)**: every shape spinner and its S/T-flip semantics,
  the type / hollow-shape / sculpt-type combos, the physics / material / temp /
  phantom flags, position/size/rotation precision and clamping.
- **Features (`llpanelvolume`)**: flexible path, light + spotlight, reflection
  probe, animesh, the physics-shape-type / gravity / friction / density fields.
- **Texture (`llpanelface`)**: matmedia (Textures / PBR / Media), the map-type
  radio, all three legacy channels + the GLTF channels, align / flip /
  copy-paste / "select same", repeats-per-meter vs raw scale, the alpha-mode
  block — cross-check with [[viewer-face-materials-pbr]].
- **Content**: the prim inventory list, drag / drop, new-script, permissions.

For each: does our control exist, is it in the right place, does it read the
same value, does it commit the same message, does it clamp / gate / grey the
same way? File the gaps.

Reference (Firestorm, read-only): `floater_tools.xml` and the `llpanel*` /
`lltool*` sources it wires in.

## Discharged — Firestorm full-parity audit (2026-08-19)

The control-by-control source-level walk of `floater_tools.xml`,
`panel_tools_texture.xml`, `floater_build_options.xml` and the
edit-adjacent floaters was executed as part of the Firestorm full-parity
audit. Every gap became its own item: [[viewer-build-tool-row-parity]],
[[viewer-build-align-tool]], [[viewer-build-create-tool-options]],
[[viewer-build-general-sale-clickaction]], [[viewer-build-sculpt-controls]],
[[viewer-build-physics-params]], [[viewer-build-probe-animesh-controls]],
[[viewer-build-widget-parity-upgrades]], [[viewer-build-copy-paste-params]],
[[viewer-build-stretch-textures]], [[viewer-build-texture-tab-fs-extras]],
[[viewer-build-creation-defaults]], plus addenda on the open
grid-options / bulk-permissions / terrain-brush / media-prim tasks. The
remaining live behavioural side-by-side (previews, clamps,
revert-on-close under a real pointer) is exactly the scope of the
blocked interaction-test tasks [[viewer-build-floater-interaction-tests]],
[[viewer-edit-gizmo-interaction-tests]] and
[[viewer-edit-selection-interaction-tests]], so nothing of this
umbrella's scope remains untracked.
