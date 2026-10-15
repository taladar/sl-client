---
id: protocol-sl-llsd-serde
title: serde Serialize/Deserialize derives for sl-llsd (Llsd) types
topic: protocol
status: ready
origin: split from the sl-proto/sl-wire serde pass (2026-08-06)
refs: [viewer-avatar-state-dump-replay, protocol-sl-lsl-serde]
---

**Scope: `#[derive(serde::Serialize, serde::Deserialize)]` on the `Llsd` value
type** (and any sub-types) so an `Llsd` value can be serialized *by* serde (to
JSON etc.). This is NOT about implementing sl-llsd as a serde **data format** —
i.e. no `serde::Serializer`/`Deserializer` that encodes arbitrary serde types to
or from LLSD-on-the-wire. Just make the LLSD types themselves
serde-serializable.

Currently `sl-llsd` has no serde, which is the last blocker for a few wire types
that otherwise gained serde in the sl-proto/sl-wire pass:

- `sl-wire`: `EventQueueEvent` and `EventQueueResponse` (both hold `Llsd`).

Follow the same approach the sl-proto/sl-wire pass used: fully-qualified
`serde::Serialize`/`Deserialize` derives (no `use serde`), feature-gate if the
crate should stay serde-free by default (match the crate's existing convention),
and skip only genuinely non-serde members (borrowed lifetimes, `Instant`,
foreign non-serde types). `Llsd` is a JSON-like recursive value, so a serde
Serialize/Deserialize should map cleanly (mind the binary/real/uuid/date
variants — pick a stable representation and add a round-trip test). Once done,
re-derive serde on the two sl-wire EventQueue types and enable the sl-llsd serde
feature in sl-wire's dependency on it.

## Audit 2026-10-15: the derive is in, the consumers are not

`Llsd` (`sl-llsd/src/value.rs`) now derives `serde::Serialize` /
`serde::Deserialize` behind the crate's default-off `serde` feature, and
`sl-wire`'s own `serde` feature turns on `sl-llsd/serde` (added with the
login-surface work, `07da0d69`). What remains:

- derive serde on `sl-wire`'s `EventQueueEvent` and `EventQueueResponse`
  (`sl-wire/src/llsd.rs`), which still derive only `Debug, Clone, PartialEq`;
- a serde round-trip test of `Llsd` itself — none exists — that pins the
  representation chosen for the binary, real, uuid and date variants (the
  derive's default externally tagged form is what is in use today).
