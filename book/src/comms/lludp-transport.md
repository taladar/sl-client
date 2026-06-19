# LLUDP Transport

**LLUDP** is the Linden Lab UDP protocol — a thin framing and reliability layer
over plain UDP. It carries the real-time, high-volume traffic of the world:
object updates, avatar movement, terrain, sound, and (historically) chat. It is
deliberately loss-tolerant: most traffic is fire-and-forget, and the small
subset that must arrive is marked reliable and retransmitted.

This chapter describes the *envelope* — how a datagram is framed. What travels
inside the envelope is a [message](messages.md).

## The datagram layout

Every LLUDP datagram looks like this:

```text
┌──────┬───────────────────┬─────────┬───────────────┬───────────────┬───────┐
│flags │ sequence (u32 BE) │ extra   │ extra-header  │ message body  │ acks  │
│ 1 B  │      4 B          │ len 1 B │  (len bytes)  │ (maybe zero-  │(opt.) │
│      │                   │         │               │   coded)      │       │
└──────┴───────────────────┴─────────┴───────────────┴───────────────┴───────┘
        └──────────── 6-byte prelude ─────────────┘
```

- **Flags** — one byte of control bits (below).
- **Sequence number** — a 4-byte **big-endian** `u32`, incremented per packet on
  a circuit. It identifies the packet for acknowledgement and de-duplication.
- **Extra-header length** — one byte giving the number of extra-header bytes
  that follow (usually zero).
- **Message body** — the encoded message. If the `ZEROCODED` flag is set, this
  region is zero-coded and must be expanded before parsing.
- **Appended acks** — present only when the `ACK` flag is set; see below.

The fixed part of the prelude is six bytes (flags + sequence + extra-length).

## The flag bits

The first byte carries four flags:

| Flag | Bit | Meaning |
|------|-----|---------|
| `ZEROCODED` | `0x80` | The message body is [zero-coded](#zero-coding). |
| `RELIABLE`  | `0x40` | The packet must be acknowledged; the sender will retransmit until it is. |
| `RESENT`    | `0x20` | This packet is a retransmission of one sent earlier. |
| `ACK`       | `0x10` | The datagram has acknowledgements appended at its end. |

## Sequence numbers and reliability

Each [circuit](circuits.md) keeps an outgoing sequence counter, starting at 1
and incrementing per packet. Reliability is opt-in per packet:

- A packet sent with `RELIABLE` is held in an "unacknowledged" table keyed by
  its sequence number. If it is not acknowledged within the resend timeout
  (**1500 ms** in this implementation), it is resent with the `RESENT` flag set,
  up to a maximum number of attempts (**6**). Exhausting the attempts during the
  handshake is fatal to the session.
- The receiver acknowledges a reliable packet in one of two ways: by appending
  its sequence number to some other outgoing datagram (the `ACK` flag), or — for
  batches — by sending an explicit `PacketAck` message. Appended acks are the
  common, cheap path; `PacketAck` mops up the rest.
- The receiver also remembers recently seen sequence numbers so that a
  retransmission it has already processed is acknowledged again but not acted on
  twice.

### Appended acknowledgements

When the `ACK` flag is set, the acknowledgements ride at the *very end* of the
datagram, after the message body:

```text
… message body … │ ack₁ (u32 BE) │ ack₂ │ … │ ackₙ │ count (1 B) │
```

The final byte is the count *n*; the *n* big-endian `u32` values before it are
the acknowledged sequence numbers. A decoder must strip these from the tail
*before* trying to parse the body, because the body length is "everything
between the extra header and the appended acks".

## Zero-coding

`ZEROCODED` is a trivial run-length compression applied to the body only (never
the header or the acks). A run of zero bytes is replaced by the marker `0x00`
followed by a one-byte count of how many zeros it stands for. Runs longer than
255 are split into successive chunks. Because so many message fields are zero or
mostly-zero (UUIDs that are unset, reserved fields, small integers in wide
slots), this meaningfully shrinks typical traffic. The body must be expanded
back out before the message is parsed.

## Byte order, a recurring trap

LLUDP mixes endianness, which catches everyone at least once:

- **Packet sequence numbers and appended acks are big-endian** (network order).
- **Message field payloads are little-endian** — integers, floats, and so on
  inside the message body.
- LLSD binary integers (a different format entirely; see [LLSD](llsd.md)) are
  big-endian again.

When a value comes out wrong by byte-swap, this is almost always the cause.

---

> **In this codebase**
>
> - Framing lives in `sl-wire/src/header.rs`: `PacketFlags` (with the
>   `ZEROCODED`/`RELIABLE`/`RESENT`/`ACK` constants), `ParsedDatagram`, and the
>   `parse_datagram` / `encode_datagram` functions. Its module doc describes the
>   prelude and appended-ack layout precisely.
> - Zero-coding is `sl-wire/src/zerocode.rs`.
> - Endianness helpers are `sl-wire/src/endian.rs`; field-level reading/writing
>   is `sl-wire/src/field.rs` (`Reader`, `Writer`).
> - The reliability bookkeeping (the unacked table, seen-window, ack queue)
>   lives per-circuit in `sl-proto/src/session/circuit.rs`; the `RESEND_TIMEOUT`
>   (1500 ms) and `MAX_RESEND_ATTEMPTS` (6) constants are in
>   `sl-proto/src/session.rs`.
> - `PacketAck` is a generated message (see [Messages](messages.md)).
