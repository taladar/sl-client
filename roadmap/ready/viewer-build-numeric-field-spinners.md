---
id: viewer-build-numeric-field-spinners
title: Build-window numeric fields — up/down arrow spinners
topic: viewer
status: ready
origin: user request (2026-07-24) while reviewing the build-tool numeric fields
refs: [viewer-prim-parameter-editing, viewer-prim-texture-editing,
  viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

Every numeric field in the Build Tools floater (the reference's `LLSpinner`)
carries a pair of small **up / down arrow buttons** that step the value by a
per-field increment, holding to repeat. Ours are plain text inputs today — a
value only changes by typing. Add spinner arrows to each numeric build field:

- **Where**: the transform rows (position / rotation / size X-Y-Z), the grid
  unit, every Object-tab shape spinner (cut / hollow / twist / taper / shear /
  radius / revolutions / skew …), the Features-tab flexi / light / spot fields,
  and the Texture-tab transparency / glow / repeats / offset / rotation fields.
- **Step**: the reference's per-field increment (`LLSpinner` `increment`), e.g.
  0.01 m for position / size, 1° for rotation, the shape fields' own steps.
  Shift / Ctrl modifiers step by a coarser / finer amount as the reference does.
- **Behaviour**: click steps once; press-and-hold auto-repeats after an initial
  delay. Each step commits exactly as an Enter would (the same
  `MultipleObjectUpdate` / `ObjectImage` / feature send), and clamps to the
  field's min / max. The arrows grey / disable with their field (they share the
  field's gate — see the no-selection disabling already wired for the transform
  and Texture-tab controls).

Best done as a **reusable spinner widget** wrapping `spawn_text_input`
(mirroring the reusable combo / radio / colour-picker widgets), so every build
field and any future numeric field gets arrows for free.

Reference (Firestorm, read-only): `llspinctrl.cpp` / `llspinctrl.h`
(`LLSpinCtrl` — the arrow buttons, increment, hold-to-repeat, clamp).
