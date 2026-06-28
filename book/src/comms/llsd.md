# LLSD

**LLSD** (Linden Lab Structured Data) is the protocol's general-purpose data
format. It is to the [CAPS](caps.md) / HTTP side of the protocol what the
[message template](messages.md) is to the UDP side: nearly everything that
travels over HTTP — seed responses, event-queue bodies, inventory, materials,
voice, map data — is LLSD.

## The value model

LLSD describes a small, JSON-like tree of typed values. There are scalars, two
containers, and an explicit "undefined":

| LLSD type | Meaning |
|-----------|---------|
| **Undefined** | The null / absent value. |
| **Boolean** | `true` / `false`. |
| **Integer** | A 32-bit signed integer. |
| **Real** | A double-precision float. |
| **String** | UTF-8 text. |
| **UUID** | A 16-byte identifier. |
| **Date** | An ISO-8601 timestamp. |
| **URI** | A URI string. |
| **Binary** | Raw bytes. |
| **Array** | An ordered list of values. |
| **Map** | A string-keyed dictionary of values. |

That UUID, Date, Binary, and URI are *first-class scalar types* — not just
strings — is the main thing that distinguishes LLSD from JSON, and it matters
because the protocol leans on UUIDs everywhere.

## Three serializations

The same value model has three on-the-wire encodings:

- **LLSD-XML** — the verbose XML form, and the one used almost everywhere over
  HTTP. Each value is an element: `<integer>`, `<real>`, `<uuid>`, `<binary>`
  (base64-encoded), `<map>`/`<key>`, `<array>`, etc., all wrapped in a top-level
  `<llsd>`. The `Content-Type` is `application/llsd+xml`.
- **Binary LLSD** — a compact binary encoding, used in a few
  performance-sensitive places. Integers and reals are **big-endian** here (a
  contrast with LLUDP field payloads — see the
  [byte-order trap](lludp-transport.md#byte-order-a-recurring-trap)).
- **Notation** — a human-readable, JSON-ish text form, used mostly in tooling
  and specs rather than on the wire.

A small but important example:

```text
<llsd>
  <map>
    <key>id</key>       <integer>42</integer>
    <key>agent_id</key> <uuid>11111111-2222-3333-4444-555555555555</uuid>
    <key>events</key>
    <array>
      <map>
        <key>message</key> <string>TeleportFinish</string>
        <key>body</key>    <map> … </map>
      </map>
    </array>
  </map>
</llsd>
```

That shape — a map with an `id` and an `events` array of `{message, body}` maps
— is exactly an [event-queue](caps.md#the-event-queue-eventqueueget) response.

## A real-world gotcha: binary-encoded integers

OpenSim (and sometimes Second Life) delivers some `uint`/`ulong` fields not as
`<integer>` but as **big-endian bytes inside a `<binary>` element**. A naive
reader that only handles `<integer>` will read those fields as `0`. When a
numeric LLSD field is mysteriously zero, check whether it arrived as binary and
decode the bytes big-endian.

---

> **In this codebase**
>
> - The value tree is the `Llsd` enum in the `sl-llsd` crate
>   (`sl-llsd/src/value.rs`), with variants `Undef`, `Boolean`, `Integer`,
>   `Real`, `String`, `Uuid`, `Date`, `Uri`, `Binary`, `Array`, `Map`
>   (re-exported through `sl-wire`'s `llsd` module, and onward as
>   `sl-proto`'s `Llsd`).
> - Accessors make trees ergonomic to walk: `Llsd::get(key)`, `index(i)`,
>   `as_array`, `as_map`, plus scalar coercions. Use these rather than matching
>   variants by hand. The typed `field_*` / `require_*` map accessors return
>   `LlsdError` (missing / wrong-kind field), which `sl-wire` transports as
>   `WireError::Llsd` — keeping structured-data faults distinguishable from
>   text-scalar ones (`WireError::InvalidScalar` / `InvalidUuid`).
> - The XML parser lives in `sl-llsd`; the CAPS request-body builders
>   (`build_seed_request`, `build_event_queue_request`,
>   `build_object_media_update_request`, …) and response parsers
>   (`parse_seed_response`, `parse_event_queue_response`, …) stay in
>   `sl-wire/src/llsd.rs`, since they depend on `WireError` and the typed
>   `sl-types` keys.
> - For the binary-integer gotcha, the tolerant `llsd_u32` / `llsd_u64` helpers
>   in `sl-proto/src/session/conversions.rs` decode a number whether it arrived
>   as `<integer>` or as big-endian `<binary>`.
