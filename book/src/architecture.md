# Architecture

Before diving into the protocol, here is how the workspace is layered, because
the chapters refer to these crates constantly. The guiding principle is
**sans-I/O**: the protocol logic does no networking itself. It is expressed as
pure functions and state machines that consume bytes and produce bytes (plus
*events*), and a thin runtime crate does the actual sending and receiving. This
keeps the protocol testable without a network and lets the same core drive
different runtimes.

```text
   ┌─────────────────────────────────────────────────────────────┐
   │  sl-msg-template     parses message_template.msg → typed AST  │
   └───────────────┬─────────────────────────────────────────────┘
                   │ (build time, via sl-wire/build.rs)
                   ▼
   ┌─────────────────────────────────────────────────────────────┐
   │  sl-wire    sans-I/O codec: LLUDP framing, generated message  │
   │             types, LLSD, login XML-RPC, CAPS request bodies   │
   └───────────────┬─────────────────────────────────────────────┘
                   ▼
   ┌─────────────────────────────────────────────────────────────┐
   │  sl-proto   sans-I/O state machines: Session / SimSession,    │
   │             Command in → Event out, all the content types     │
   └───────────────┬─────────────────────────────────────────────┘
          ┌────────┴─────────┐
          ▼                  ▼
   ┌──────────────┐   ┌──────────────┐
   │sl-client-    │   │sl-client-bevy│   I/O drivers: UDP socket +
   │tokio         │   │              │   HTTP (reqwest), task wiring
   └──────┬───────┘   └──────────────┘
          ▼
   ┌──────────────┐
   │  sl-survey    │   a binary that consumes the tokio driver
   └──────────────┘
```

## The crates

- **`sl-msg-template`** — a parser for Linden Lab's `message_template.msg` file
  into a typed AST (`Template`, `MessageDef`, `BlockDef`, `FieldDef`). It runs
  at build time and has no runtime role. See
  [Messages & the Template](comms/messages.md).

- **`sl-wire`** — the sans-I/O **codec**. It owns the LLUDP packet framing
  (`PacketFlags`, `ParsedDatagram`, `parse_datagram`/`encode_datagram` in
  `sl-wire/src/header.rs`), the [`Message`](comms/messages.md) trait and
  `MessageId`, the generated message structs (`sl-wire/build.rs` reads the
  template and writes `sl-wire/src/messages.rs`), the [`Llsd`](comms/llsd.md)
  value type, the [login](content/login.md) XML-RPC codec, and the LLSD request
  builders for CAPS. It does no I/O.

- **`sl-proto`** — the sans-I/O **state machines** and all the content-level
  types. The client-side `Session` (`sl-proto/src/session.rs`) drives the
  connection lifecycle; the server-side mirror `SimSession`
  (`sl-proto/src/sim_session.rs`) is used by tests and tooling. Applications
  speak to it through two enums: they submit a `Command`
  (`sl-proto/src/command.rs`) and they receive an `Event`
  (`sl-proto/src/types/event.rs`). Every feature domain has a module under
  `sl-proto/src/types/` (`chat.rs`, `inventory.rs`, `group.rs`, `parcel.rs`, …).
  It does no I/O.

- **`sl-client-tokio`** — an async **I/O driver** built on Tokio. It owns the
  UDP socket and a `reqwest` HTTP client, pumps datagrams and CAPS requests in
  and out of a `Session`, and runs the event-queue long-poll
  (`sl-client-tokio/src/caps.rs`). This is the crate most applications use.

- **`sl-client-bevy`** — an alternative I/O driver that integrates the same
  `sl-proto` core into the [Bevy](https://bevyengine.org/) ECS, for clients
  built as Bevy apps.

- **`sl-survey`** — a headless binary (built on the tokio driver) that logs in,
  walks the map, and collects region/parcel metadata. A good worked example of
  the stack in use.

## The command/event flow

The sans-I/O contract is the same in both drivers:

1. The application submits a `Command` (e.g. `Command::Chat { … }`) to the
   `Session`.
2. The `Session` turns commands and incoming bytes into outgoing datagrams /
   HTTP requests, which the driver flushes onto the socket / HTTP client.
3. Incoming datagrams and CAPS/event-queue responses are fed back into the
   `Session`.
4. The `Session` emits `Event` values (e.g. `Event::ChatMessage(..)`) that the
   driver hands back to the application.

Because steps 1–4 are pure with respect to I/O, the protocol can be unit-tested
by pairing a `Session` with a `SimSession` and passing buffers between them —
no sockets required.

## Where types belong

A recurring design question in this workspace is whether a type belongs here or
in the shared `sl-types` crate. As a rule of thumb: vocabulary types reusable
across unrelated projects (vectors, region coordinates, common SL value types)
live in `sl-types`; types that only make sense in terms of this protocol (wire
messages, `Command`/`Event`, the per-feature structs) live in `sl-proto` /
`sl-wire`.
