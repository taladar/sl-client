---
id: protocol-audit-legacy-material-date-codec
title: The legacy-material binary LLSD codec writes Date one way and reads it another
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 2
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

Scope: delete the duplicate codec and route `legacy.rs` through `sl-llsd`, or —
if the legacy quirks are load-bearing — fix `Date` and add the round-trip test
whose absence let this survive.
