---
id: protocol-audit-tokio-caps-refetch
title: The tokio region-change caps fetch stalls the UDP pump, then swallows its failure
topic: protocol
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

`sl-client-tokio/src/lib.rs:716` is
`caps = fetch_capabilities(...).await.unwrap_or_default()`, executed **inline in
the single `select!` run loop**. Three defects in one line:

- it `await`s a network round-trip while holding the loop, so
  `socket.recv_from`, ACKs and retransmits stall for up to the client's 60 s
  timeout — long enough to kill the circuit. The bevy runtime does the same work
  on its event-queue worker thread (`sl-client-bevy/src/caps.rs:89`);
- `unwrap_or_default()` swallows the failure into an empty cap map with **no
  diagnostic**, even though the crate's own `report_caps_failure` sentinel
  machinery (`caps.rs:20-31`) exists for exactly this;
- `spawn_event_queue` then returns `None` (`caps.rs:87`) and never retries, so a
  single transient seed-caps failure permanently kills the event queue, asset
  caps and inventory for that region until the next crossing. The bevy side
  handles "no `EventQueueGet` yet" by blocking for a later switch
  (`caps.rs:361-369`).

Two neighbours worth fixing in the same pass:

- `lib.rs:370` — `Client::connect` builds its login `reqwest::Client` with **no
  timeout**. Async reqwest has no default (unlike `reqwest::blocking`, which the
  bevy login path relies on), so a grid that accepts the connection and never
  answers hangs `connect()` forever. The other three client builds all set 60 s.
- `caps.rs:128-132` — the event-queue long poll `continue`s with **no backoff**
  on a body-read or parse failure, so a grid returning an unparsable 200 puts
  the client into an unthrottled request loop. The transport-error arm at `:117`
  does sleep 1 s, so the omission looks accidental. Same in
  `sl-client-bevy/src/caps.rs:305-310`.
