---
id: idiomatic-p1-01
title: New Permissions bitflags type (the SL PERM_* set:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 1 — Permission & flag bitflags (low invasiveness, high ROI)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

Caller-facing bitfields are currently raw integers. Follow the existing
`sl-wire/src/parcel_flags.rs` / `control_flags.rs` pattern
(`union`/`contains`/`from_bits`/`bits`; no external `bitflags` crate).

New `Permissions` bitflags type (the SL `PERM_*` set:
MODIFY/COPY/TRANSFER/MOVE/…) plus a `Permissions5 { base, owner, group,
everyone, next_owner }` grouping struct (both in `sl-wire/src/permissions.rs`,
following the `parcel_flags`/`control_flags` pattern). The five raw `u32`
masks (`base_mask`/`owner_mask`/`group_mask`/`everyone_mask`/
`next_owner_mask`) are replaced by one `permissions: Permissions5` field in
`ObjectProperties`, `InventoryItem`, `RestoreItem` (the `ObjectOwnershipData`
the roadmap named), and `ObjectPropertiesFamily` (the condensed sibling of
`ObjectProperties`, which carries the identical five-mask block). Codec sites
wrap/unwrap at the boundary so the wire bytes are byte-identical. The
partial-grant masks that are *not* a full `LLPermissions` block —
`NewInventoryItem.next_owner_mask`, `NotecardRez`'s three masks,
`Command::UploadAsset`'s three masks, and the `i32` `ObjectPermMasks`
preferences block — were intentionally left raw (a different concept; not
five-mask blocks).
