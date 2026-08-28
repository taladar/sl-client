---
id: protocol-audit-legacy-material-date-codec
title: The legacy-material binary LLSD codec writes Date one way and reads it another
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 2
refs: [protocol-audit-llsd-recursion-depth-cap, protocol-audit-asset-decoder-allocation-caps]
---

Context: [context/protocol.md](../context/protocol.md).

`sl-wire/src/material/legacy.rs` carries a **second, independent binary-LLSD
codec** duplicating `sl-llsd/src/binary.rs`, and the two have drifted:

- write (`:305`): `Llsd::Date(text) => write_binary_string(b'd', text, out)` —
  marker, 4-byte BE length, UTF-8 text;
- read (`:379`):
  `b'd' => { reader.take_array::<8>().ok()?; Some(Llsd::Date(String::new())) }`
  — eight raw bytes, discarded.

Any `Date` written and read back **desynchronises every subsequent value in the
stream**. Note `sl-llsd` itself is correct: its native-endian 8-byte `Date`
faithfully mirrors the reference, which writes raw host bytes
(`llsdserialize.cpp:1621`) and reads them the same way (`:1134`). Only this file
diverges.

The two codecs also disagree on strictness — `read_binary_array` /
`read_binary_map` do `reader.u8().ok();` (a missing or wrong terminator is
accepted) where `sl-llsd` returns `MissingBinaryTerminator`, and
`read_binary_map` never checks the key tag is `k`.

A third divergence, found while fixing
[[protocol-audit-asset-decoder-allocation-caps]]: `read_binary_value` recurses
through arrays and maps with **no depth bound**. `sl-llsd` gained
`MAX_NESTING_DEPTH` in [[protocol-audit-llsd-recursion-depth-cap]]; this copy
never did. The inflate ceiling that fix added (64 MiB) bounds the *input* but
not the nesting — one byte per level is enough to overflow the stack well
inside it — so the exposure stands until the codec is replaced.

Scope: delete the duplicate codec and route `legacy.rs` through `sl-llsd`,
which closes `Date`, the terminator strictness and the nesting depth at once —
or, if the legacy quirks turn out to be load-bearing, fix `Date`, add a depth
guard, and add the round-trip test whose absence let this survive.

## Fixed (2026-08-28)

The duplicate codec is gone. `sl-wire/src/material/legacy.rs` now writes with
`Llsd::to_llsd_binary` and reads with `sl_llsd::parse_llsd_binary`, so the
`RenderMaterials` bodies and the inventory cache share one codec — which
closes `Date`, the terminator strictness, the unchecked `k` key tag and the
unbounded nesting in one move, because every one of them was a property of the
copy rather than of the format.

Nothing was load-bearing about the legacy quirks. The relevant question was
whether the stricter reader would refuse a real payload, and it does not:
OpenSim's `MaterialsModule` serialises through `ZCompressOSD(osd, useHeader:
false)`, i.e. libomv's header-less binary LLSD, which writes the mandatory `]`
/ `}` terminators and the `k`-tagged map keys `sl-llsd` requires. The one
encoding change on the wire is that map entries now come out sorted by key
rather than in `HashMap` order; the format does not carry order, and
determinism is the better default.

Three lines of duplication went with it: all three builders wrapped their
value in the `{ "Zipped": … }` envelope by hand, and now share a `zipped_body`
helper; `parse_render_materials_response` re-implemented `parse_zipped_body`
and now calls it. `sl-wire`'s `endian` module lost `i32_from_be` / `i32_to_be`
/ `f64_from_be` / `f64_to_be`, which existed only for the deleted codec.

Three tests, each of which fails (or aborts) without the fix:

- a `Date` sorted ahead of the `Norm*` / `Spec*` keys inside a `Material` map,
  which under the old codec desynchronised the whole rest of the material and
  decoded it as defaults;
- a payload whose closing `]` is removed, which the old reader accepted;
- a payload nested 100_000 deep, which the old reader followed until the stack
  overflowed. It is cheap despite its size, because the parse bails at
  `MAX_NESTING_DEPTH` without reading the rest.
