# Communication Layer

This part covers the plumbing that everything else is built on. None of it is
visible to a user of the virtual world, but every feature in the
[Content Layer](../content/index.md) is expressed in terms of these primitives.

The pieces, roughly bottom-up:

- **[Sessions](sessions.md)** — what one logged-in agent's connection is, and
  the lifecycle it moves through from login to logout.
- **[LLUDP Transport](lludp-transport.md)** — the UDP packet framing: the
  header, the flag bits, sequence numbers, appended acknowledgements,
  zero-coding, and the reliability layer.
- **[Circuits](circuits.md)** — the per-simulator UDP connection an agent holds,
  how it is established, and how a client holds several at once.
- **[CAPS & the Event Queue](caps.md)** — the HTTPS side of the protocol:
  capability URLs, the seed capability, and the `EventQueueGet` long-poll that
  delivers asynchronous server events.
- **[LLSD](llsd.md)** — Linden Lab Structured Data, the serialization format
  used for almost everything that travels over CAPS.
- **[Messages & the Template](messages.md)** — how every LLUDP message is
  defined in a shared template file and turned into typed Rust structs at build
  time.

A useful mental model: **LLUDP carries the message template's messages**, and
**CAPS carries LLSD**. The transports and the formats pair up that way
throughout the protocol.
