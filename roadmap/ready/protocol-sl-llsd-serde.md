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
