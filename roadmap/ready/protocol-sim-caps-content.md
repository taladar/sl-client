---
id: protocol-sim-caps-content
title: Server-side content upload/update caps, materials and MOAP
topic: protocol
status: ready
origin: user request (2026-07) — complete simulator protocol surface
points: 8
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The upload/update flows, server side:

- `NewFileAgentInventory`, `UploadBakedTexture`,
  `UpdateAvatarAppearance`;
- the `Update{Gesture,Notecard,Script,Settings,Material}` ×
  `{Agent,Task}Inventory` caps — the two-stage upload flow (grant an
  uploader URL → receive the body → return the new asset id) as a
  reusable server-side state machine;
- `RenderMaterials` / `ModifyMaterialParams` /
  `UpdateMaterialAgentInventory`;
- `ObjectMedia` / `ObjectMediaNavigate` (MOAP).

Inverse-pairing per the convention; the two-stage uploader is verified by
driving the client `Session`'s own upload flow against it in-memory.
