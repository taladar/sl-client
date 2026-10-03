---
id: idiomatic-opaque-bool-parameters
title: Replace call-site-opaque and correlated bools with named types
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What (readability; none misused today)

Correlated bools that admit invalid states: `RlvWearMask { add, replace }`
(replace-without-add), `GizmoDrag { duplicate, duplicate_sent }`
("sent but not armed"), `Accelerator { ctrl, alt, shift }`, CLI
`Click { right, double }`, `MeshHeader.not_found` folding 404 and
"version too new". Swappable adjacent bools: `cast_ray(…, solid, solid_only)`
(callers pass `false, false` / `true, false`), modifier keys in opposite
orders (`select(index, ctrl, shift)` vs `resolve_volume(base, shift, ctrl)`),
`apply_owner_say(…, issuer: Uuid, agent: Uuid)`. Two-variant enums with a
name: inventory vs library (`library: bool` in five functions), wear add vs
replace (immediately becomes `AttachmentMode`), sort by date, rights
direction (`Right(false, RightKind::SeeOnline)`), bake mask combine
(`multiply_blend`), prim cap (`top: bool`), key / mouse button state
(`down: bool`), `FetchChunk.whole`, `translate_label: bool` beside a label
string (~109 sites; `UiLabel { Key, Literal, Glyph }` exists),
`spawn_action_button(.., write: bool, ..)` (19 bare `true,`), paging
`forward: bool`, result-shaped events with `success: bool` plus an error
string (`ChatSessionStarted`, `ServerAppearanceUpdate`, `CreateGroupResult`),
`duplicate_objects_on_ray` with four bools, `set_object_position /
rotation / scale(…, group, uniform)`, `set_object_permissions(set)`.
Also: `RlvHeldCommand.keyword: String` re-resolved later where the
`&'static RlvEntry` is already known.

## How

Small enums / bitflags per site (`Modifiers`, `InventoryTree`, `LinkScope`,
`Grant` / `Revoke`, `ButtonState`, `Cap`, `ShiftCopy { Off, Armed, Sent }`,
Result-style event enums). Sweep crate by crate.
