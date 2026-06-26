# Generic Messages

Not every feature earns a dedicated message in
[`message_template.msg`](messages.md). Linden Lab adds protocol surface
constantly, and minting a new template message — with its own blocks, fields,
frequency id, and generated struct — for every small, loosely-coupled feature
would bloat the template and force a lock-step template update on every client.
So the protocol reserves a handful of **generic** envelopes: a method selector
plus an opaque parameter payload, where the *method name* picks the feature and
the *payload layout* is whatever that feature defines. New features can ride an
existing generic message without ever touching the template.

This chapter covers the three generic envelopes, the contract they impose on
consumers, and how this workspace surfaces them.

## The method-envelope shape

Every generic message is the same idea: **a method selector, an optional
correlation id, and one or more opaque payload blobs.** Nothing in the envelope
tells you how to interpret the payload — that is entirely a function of the
method. A decoder reads the envelope, hands the consumer the method and the raw
bytes, and stops there.

The selector comes in two forms. The two UDP-style envelopes
(`GenericMessage`, `LargeGenericMessage`) use a **string** method name
(`"emptymutelist"`, `"GrantUserRights"`, …) plus an `Invoice` UUID that acts as
a feature-specific correlation id pairing a reply with the request that
provoked it. The streaming envelope (`GenericStreamingMessage`) uses a
**numeric** method id and carries no invoice.

## The three flavours

| Message | Id | Selector | Payload | Notes |
|---------|----|----------|---------|-------|
| `GenericMessage` | Low 261 | string `Method` + `Invoice` | a list of byte blobs (`Parameter`, one count byte each) | the workhorse |
| `LargeGenericMessage` | Low 430 | string `Method` + `Invoice` | same, but two count bytes per parameter | larger per-param limit; HTTP transport on real grids (`UDPDeprecated`) |
| `GenericStreamingMessage` | High 31 | numeric `Method` (`U16`) | a single byte blob (`Data`, two count bytes) | optimised for streaming; avoid payloads over ~7 KB |

`GenericMessage` and `LargeGenericMessage` are structurally identical — an
`AgentData` block (agent/session/transaction ids), a `MethodData` block (the
method name and invoice), and a `Variable` `ParamList` of `Parameter` blobs.
They differ only in the per-parameter length prefix: `GenericMessage` writes one
count byte (so each parameter is at most 255 bytes), while
`LargeGenericMessage` writes two (so each parameter can be far larger). On real
grids the large form rides HTTP rather than UDP, which is why the template marks
it `UDPDeprecated`.

`GenericStreamingMessage` is a leaner, simulator-to-viewer (`Trusted`) variant:
a numeric method id and a single opaque `Data` blob, with no agent block and no
invoice. It exists for payloads that don't fit the small-parameter-list mould —
typically notation- or binary-encoded LLSD streamed to the viewer.

## Worked examples

**`emptymutelist` (`GenericMessage`).** When a viewer's mute list is empty, the
simulator sends a `GenericMessage` whose method is `emptymutelist` and whose
parameter list is empty — a degenerate use of the envelope as a pure signal. No
payload parsing is needed; the method name *is* the message.

**`GrantUserRights` (`GenericMessage`).** Granting another agent rights (e.g.
over an object) is a method name plus a small parameter list of stringified
values. The feature defines the order and meaning of those parameters; the
envelope only guarantees they arrive in order.

**The `0x4175` GLTF material override (`GenericStreamingMessage`).** A PBR
(GLTF) material override for an object is streamed with the numeric method id
`0x4175` and a single `Data` blob of encoded LLSD describing the per-face
overrides. The blob is large and structured, which is exactly what the
streaming envelope is for.

## The parsing contract

The defining property of every generic message is that **parameter parsing is
the feature handler's job, not the envelope's.** The envelope decoder knows the
method name (or numeric id) and the raw bytes, and nothing more. It cannot
validate the payload, because it doesn't know the payload's schema — that schema
belongs to the method.

In this workspace the session honours that contract by surfacing the method and
the verbatim bytes and leaving interpretation to the consumer. The session does
intercept a few methods it understands itself — `emptymutelist` becomes an
`Event::MuteList`, and the `0x4175` streaming method is
decoded into a typed `Event::GltfMaterialOverride` — but everything else is
handed up unparsed as a `GenericMessage` / `LargeGenericMessage` /
`GenericStreamingMessage` event. A consumer matches on the method, then decodes
the parameters however that method requires (in practice each string parameter
is usually NUL-terminated UTF-8, but the bytes are preserved exactly so any
encoding survives).

---

> **In this codebase**
>
> - Types are in `sl-proto/src/types/generic.rs`: `GenericMessage` (the
>   `method` / `invoice` / `params` envelope shared by the small and large
>   forms) and `GenericStreamingMessage` (numeric `method` + opaque `data`). The
>   string-method envelopes keep their correlation id as the
>   [`InvoiceId`](messages.md) newtype (`sl-proto/src/bookkeeping_ids.rs`); the
>   GLTF method constant is `sl_wire::GLTF_MATERIAL_OVERRIDE_METHOD` (`0x4175`).
> - Events are `Event::GenericMessage`, `Event::LargeGenericMessage`, and
>   `Event::GenericStreamingMessage` in `sl-proto/src/types/event.rs`; they are
>   emitted by the decode arms in `sl-proto/src/session/methods.rs`, which match
>   the handled methods (`emptymutelist`, `0x4175`) first and fall through to
>   the verbatim envelope events for everything else.
> - Server events: `SimSession::send_generic_message`,
>   `send_large_generic_message`, and `send_generic_streaming_message`
>   (`sl-proto/src/sim_session.rs`) are the inverse encoders; the client-side
>   outbound path is `Session`'s `send_generic_message` circuit helper
>   (`sl-proto/src/session/circuit.rs`), used internally by feature commands
>   such as autopilot and the avatar picks/notes requests.
> - REPL: the three events render as `generic_message`,
>   `large_generic_message`, and `generic_streaming_message`
>   (`sl-repl/src/format.rs`).
