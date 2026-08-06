---
id: protocol-sl-lsl-serde
title: serde support for sl-lsl (LslSyntax) types
topic: protocol
status: ready
origin: split from the sl-proto/sl-wire serde pass (2026-08-06)
refs: [viewer-avatar-state-dump-replay, protocol-sl-llsd-serde]
---

Add `serde::Serialize` + `serde::Deserialize` support to the public types in the
local `sl-lsl` crate — chiefly `LslSyntax` (and any sub-types). Currently
`sl-lsl` has no serde, which is the last blocker for one central sl-proto type
that otherwise gained serde in the sl-proto/sl-wire pass:

- `sl-proto`: `Event` (`SlSessionEvent`) — its `LslSyntax(Box<LslSyntax>)`
  variant is the only thing keeping the whole event enum non-serde.

Follow the same approach the sl-proto/sl-wire pass used: fully-qualified
`serde::Serialize`/`Deserialize` derives (no `use serde`), feature-gate if the
crate should stay serde-free by default (match the crate's existing convention),
skip only genuinely non-serde members, and add a round-trip test. Once done,
re-derive serde on sl-proto's `Event` (and `Diagnostic`/`Error` if their other
blocker, `sl_wire::WireError`, is also addressed) and enable the sl-lsl serde
feature in sl-proto's dependency on it.

Note: the avatar dump/replay feature does **not** need this — it serializes
`Object` + `AvatarAppearance` and reconstructs the `SlEvent` at replay rather
than deserializing `Event`. This task is for API completeness / other tooling.
