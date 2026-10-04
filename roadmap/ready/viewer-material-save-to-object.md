---
id: viewer-material-save-to-object
title: Save a PBR material into an object's inventory
topic: viewer
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, viewer-pbr-material-editor]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life grants `UpdateMaterialTaskInventory` (OpenSim grants neither
material capability). Firestorm saves a material held in an object through the
two-step upload with `{task_id, item_id}`, and enables material inventory at all
only when both this and `UpdateMaterialAgentInventory` are granted.

The protocol mapping exists (`UpdatableAssetType::task_cap`); our material
editor only saves to agent inventory. Request the capability, add the
save-back-to-object path, and disable material editing where the grid grants
neither capability.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
