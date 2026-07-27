---
id: viewer-appearance-editor-bodyparts
title: Edit Shape / Skin / Hair / Eyes — body-part editors
topic: viewer
status: done
origin: user request (2026-07-22) — no task covered editing wearables
blocked_by: [viewer-appearance-editor-shell]
refs: [viewer-appearance-editor-clothing]
---

Context: [context/viewer.md](../context/viewer.md).

The four **body part** editors on the appearance shell: **Shape** (the
big one — the reference's Body / Head / Eyes / Ears / Nose / Mouth /
Chin / Torso / Legs sub-tabs of ~80 sliders, plus the height read-out
and gender toggle), **Skin** (tone params + the three skin textures),
**Hair** (params + hair texture) and **Eyes** (iris param + texture).
Each is the shell's slider list filtered by the slot's `avatar_lad`
params (already parsed) plus the slot's texture pickers; body parts
save-replace (never layer).

Reference (Firestorm, read-only): `llpaneleditwearable.cpp` (the
per-type subpart tables), `avatar_lad.xml`.

**Shipped 2026-07-27** via the shared `edit_wearable.rs` floater
([[viewer-appearance-editor-shell]]): Shape (the full slider list plus a live
height read-out and a Female/Male gender radio group), and Skin / Hair / Eyes
(their tone / iris param sliders + texture pickers). Body parts save-replace.
Verified live on OpenSim.
