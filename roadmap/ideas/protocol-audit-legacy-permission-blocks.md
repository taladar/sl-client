---
id: protocol-audit-legacy-permission-blocks
title: The legacy permissions block has three writers and two readers
topic: protocol
status: ideas
origin: noticed doing test-assets-object-asset-codec (2026-09-06)
points: 2
refs: [test-assets-object-asset-codec, protocol-audit-notecard-fidelity]
---

`LLPermissions`' and `LLSaleInfo`'s legacy `{ base_mask … }` blocks appear in
three asset formats this workspace handles, and each crate carries its own
copy:

- `sl-notecard` parses **and** writes them (`decode::parse_permissions`,
  `encode::write_permissions`) with its own `Permissions` / `SaleInfo` types;
- `sl-avatar` **writes** them for a wearable (`WearableAsset::to_text` plus
  `WearablePermissions` / `SaleType`) and deliberately parses past them,
  because a bake does not need them;
- `sl-object-asset` parses and writes them again, with a third pair of types.

Nothing is wrong today — each copy is small and pinned by its own tests — but
the blocks are one format, so a fix in one is a fix nobody applies to the other
two. Worth deciding whether they belong in a shared crate (a `sl-legacy-blocks`
alongside the format crates), and worth doing before a fourth format needs
them.

The awkward part is that the three crates depend on different things:
`sl-notecard` deliberately depends on `sl-types` alone, `sl-avatar` on
`sl-proto`. A shared crate has to sit at the `sl-types` level to be usable by
all three, which means its own permission-mask and sale-type types rather than
`sl_wire::Permissions5`.

Acceptance: one home for the two blocks, or a recorded decision that three
copies is the lesser cost with the reason.
