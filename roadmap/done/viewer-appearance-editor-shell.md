---
id: viewer-appearance-editor-shell
title: Appearance editor shell — wearable editing mode, params, save
topic: viewer
status: done
origin: user request (2026-07-22) — no task covered editing wearables
  (the item context menu's "Edit" entry ships greyed)
refs: [viewer-inventory-context-actions, viewer-inventory-new-wearables]
---

Context: [context/viewer.md](../context/viewer.md).

The **appearance editing mode** every wearable editor sits in (the
Vintage skin's `LLFloaterCustomize` shape): enter edit-appearance on a
worn wearable (the inventory context menu's greyed **"Edit"** entry,
the avatar pie's Appearance), camera framed on the avatar, a **visual
param slider** infrastructure driving the **live local preview**
through the existing morph / bake pipeline (`sl-avatar` VisualParams +
the client composite), and the **save / save-as / revert** plumbing —
writing the edited `.wearable` asset back (the upload path the
new-wearable creation already exercises) and updating the item /
COF. Per-slot editors are their own tasks:
[[viewer-appearance-editor-bodyparts]],
[[viewer-appearance-editor-clothing]].

Reference (Firestorm, read-only): `llfloatercustomize.cpp` (vintage) /
`llpaneleditwearable.cpp`, `llagentwearables.cpp` (saveWearable),
`llviewerwearable.cpp`.

**Shipped 2026-07-27.** The appearance-editor floater (`edit_wearable.rs`) is
the editing mode for every worn wearable: per-slot visual-param sliders, texture
pickers, a clothing tint swatch, live preview through the existing morph/bake
pipeline, and Save (in-place) / Save As / Revert. Save writes the edited
`.wearable` back onto the same item over the re-added legacy UDP transaction
upload (`Command::SaveInventoryAsset` → `AssetUploadRequest` inline / Xfer for
large assets, plus a bound `UpdateInventoryItem`); `sl-avatar` gained a
`WearableAsset::to_text` serializer. Wired to the inventory "Edit" action
(un-greyed for worn wearables). Verified live on OpenSim.
