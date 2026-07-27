---
id: viewer-appearance-editor-clothing
title: Clothing-layer editors — params, fabric textures, tint
topic: viewer
status: done
origin: user request (2026-07-22) — no task covered editing wearables
blocked_by: [viewer-appearance-editor-shell]
refs: [viewer-appearance-editor-bodyparts]
---

Context: [context/viewer.md](../context/viewer.md).

The **clothing layer** editors on the appearance shell, one per slot:
shirt, pants, shoes, socks, jacket, skirt, gloves, undershirt,
underpants, alpha (the five alpha-mask texture pickers), tattoo,
universal (its eleven layer textures) and physics (the softbody
params). Each is the slot's `avatar_lad` params plus its **fabric
texture picker(s)** and **tint colour swatch**, previewing live
through the client composite like the shell's sliders, saving back as
a layered wearable (layers stack, unlike body parts).

Reference (Firestorm, read-only): `llpaneleditwearable.cpp`,
`llwearabletype.cpp` (per-type texture/param tables).

**Shipped 2026-07-27** via the shared `edit_wearable.rs` floater
([[viewer-appearance-editor-shell]]): every clothing slot's `avatar_lad` param
sliders, its fabric texture picker(s), and a tint colour swatch driving the
slot's three colour params, previewed live through the client composite.
Verified live on OpenSim. Known limits: a layered same-type stack previews its
lead layer, and a fit / mask (shape-driven) param re-masks the bake on the next
texture / tint edit or Save rather than mid-drag.
