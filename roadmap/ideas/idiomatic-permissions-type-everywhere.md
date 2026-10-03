---
id: idiomatic-permissions-type-everywhere
title: Use the typed Permissions for every permission mask
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  viewer-default-next-owner-mask-diverges]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`sl_wire::Permissions` exists (and item properties use it), but permission
masks are raw `u32` in `sl-proto` (`types/editing.rs`, `Command` next-owner
masks), `sl-object-asset` (`LegacyPermissions`, task-inventory perms,
`perms_mask: i32`), `sl-avatar` (`WearablePermissions`) and `sl-notecard`
(`PermissionMask(pub u32)` re-declaring the bits). All values match
`llpermissionsflags.h` today; a mask can still be passed in the wrong slot
(base vs next-owner) or position unnoticed. Group powers are a bare `u64`
tested by hand (`has_power(powers, power)` reads a combined mask as "any").

## How

`Permissions` (round-trips raw bits) as the field type, `GroupPowers`
bitflags with `contains`; encode to the wire at the boundary.
