# Messages & the Template

Every [LLUDP](lludp-transport.md) packet body is a **message**. There are
several hundred message types, and crucially they are not hard-coded in each
viewer: they are defined in a single shared text file,
**`message_template.msg`**, that Linden Lab publishes and every implementation
reads. This chapter explains that file's format and how this workspace turns it
into typed Rust at build time.

## The template format

`message_template.msg` is a nested, brace-delimited description. A message
declares its identity on one line, then lists its blocks, and each block lists
its fields:

```text
{
    UseCircuitCode Low 3 NotTrusted Unencoded
    {
        CircuitCode Single
        {   Code        U32     }
        {   SessionID   LLUUID  }
        {   ID          LLUUID  }
    }
}
```

The message header line is:

```text
<Name> <Frequency> <Number> <Trust> <Encoding> [extra flags…]
```

- **Name** — the message name, e.g. `UseCircuitCode`.
- **Frequency** — how the message id is encoded on the wire (see below).
- **Number** — the numeric id within its frequency class (or the full value for
  `Fixed`).
- **Trust** — `Trusted` (only valid from a trusted source, e.g. another
  simulator) or `NotTrusted` (a client may send it).
- **Encoding** — `Zerocoded` or `Unencoded`: the *default*
  [zero-coding](lludp-transport.md#zero-coding) for this message's body.
- **Extra flags** — occasional trailing markers such as `Deprecated` or
  `UDPDeprecated`.

### Frequency and the id encoding

The **frequency** classes exist to keep common messages cheap: the more often a
message is sent, the shorter its id. The id is written with a
frequency-dependent prefix:

| Frequency | Wire encoding | Range |
|-----------|---------------|-------|
| **High**   | one byte | the hottest messages (object/avatar updates) |
| **Medium** | `0xFF` + one byte | moderately frequent |
| **Low**    | `0xFF 0xFF` + big-endian `u16` | most messages |
| **Fixed**  | the full four bytes (`0xFFFFFFxx`) | a handful of special messages, e.g. `PacketAck` |

A decoder reads the first byte; if it is `0xFF` it reads the next, and so on,
which is how it tells the classes apart.

### Blocks and fields

A message body is a sequence of **blocks**, and each block has a
**cardinality**:

- **Single** — exactly one occurrence.
- **Multiple(n)** — exactly *n* occurrences.
- **Variable** — a count byte precedes the occurrences (zero or more).

Each block contains **fields** with a fixed type vocabulary: the integer widths
(`U8`…`U32`, `S8`…`S32`), `F32`/`F64`, `BOOL`, `LLUUID`,
`LLVector3`/`LLQuaternion` and friends, `IPADDR`/`IPPORT`, and two byte-blob
forms — `Fixed { len }` for a known length and `Variable { count_bytes }` for a
length-prefixed blob.

## From template to Rust

This workspace does not interpret the template at runtime. It generates code:

```text
message_template.msg ─parse─▶ Template AST ─codegen─▶ messages.rs ─▶ structs
     (vendored)             (sl-msg-template)      (sl-wire/build.rs)
```

1. **`sl-msg-template`** parses the file into a typed AST: a `Template` holding
   `MessageDef`s, each with a `Frequency`, `Trust`, `Encoding`, and a list of
   `BlockDef`s of `FieldDef`s.
2. **`sl-wire/build.rs`** runs at build time, walks that AST, and emits a Rust
   struct per block and per message, with `encode_body` / `decode_body` methods
   that read and write the fields in order.
3. Each generated message implements the **`Message`** trait, whose associated
   constants record its identity: `NAME`, `ID` (a `MessageId`), and `ZEROCODED`.

So sending a message is: construct the generated struct, look at its
`Message::ID` to write the frequency-coded prefix, call `encode_body`, then
frame the result with a [packet header](lludp-transport.md). Receiving is the
inverse: read the `MessageId`, dispatch to the matching type, call
`decode_body`.

Because the structs are generated from the canonical template, the wire format
stays in lock-step with Linden Lab's definition — adding support for a new
message is often just a matter of the template already containing it.

## When decoding goes wrong

Two things can happen on the receive path that are worth surfacing rather than
swallowing. A message body may **fail to decode** — truncated, corrupt, or a
template mismatch — and a message may decode fine but have **no handler** in the
session. Both are reported as [diagnostics](sessions.md#diagnostics)
(`DecodeFailed` with the byte offset where decoding stopped, and
`UnhandledMessage`) when diagnostics are enabled. To label them readably, the
build step also generates `message_name(MessageId) -> Option<&'static str>`, so
a numeric id can be turned back into its template name in logs and dumps.

---

> **In this codebase**
>
> - The template parser is the `sl-msg-template` crate: `parse` returns the AST
>   in `sl-msg-template/src/ast.rs` (`Template`, `MessageDef`, `BlockDef`,
>   `FieldDef`, and the `Frequency` / `Trust` / `Encoding` / `Cardinality` /
>   `FieldType` enums). The lexer/parser are `lexer.rs` / `parser.rs`.
> - The vendored template is `sl-wire/message_template.msg`; the code generator
>   is `sl-wire/build.rs`, which writes `sl-wire/src/messages.rs`'s generated
>   half (included via `messages.rs`).
> - The `Message` trait and `MessageId` (with `High`/`Medium`/`Low`/`Fixed`) are
>   in `sl-wire/src/message.rs`. `MessageId::encode` / `decode` handle the
>   frequency prefix.
> - Generated messages are reachable under `sl_wire::messages`.
> - `message_name(MessageId) -> Option<&'static str>` is generated alongside
>   them by `sl-wire/build.rs`; it backs the message-name labels in the
>   `DecodeFailed` / `UnhandledMessage` [diagnostics](sessions.md#diagnostics).
