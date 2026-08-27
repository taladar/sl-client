---
id: protocol-audit-tokio-caps-refetch
title: The tokio region-change caps fetch stalls the UDP pump, then swallows its failure
topic: protocol
status: done
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

## Fixed (2026-08-27)

The region-change fetch is now `refetch_capabilities`, a **detached task**. The
run loop aborts the old region's event-queue poller the moment `RegionChanged`
arrives, spawns the refetch, and carries on pumping UDP; the new map comes back
over its own `mpsc` channel and is applied in a fresh `select!` arm, which is
where `spawn_simulator_features` / `spawn_event_queue` now run. Nothing in the
loop awaits the seed round-trip any more.

Each refetch is stamped with a **generation** counter bumped by the region
change that asked for it, and a map whose generation has been superseded is
dropped. Two crossings in quick succession would otherwise let a slow first
fetch land last and aim every capability at a region already left — the
previous inline code could not have this bug because it blocked, so the
detached version had to grow the guard.

The refetch **retries** a failed seed POST on the asset fetchers' existing
`transient_backoff` budget (8 retries, 200 ms doubling to a 5 s ceiling), and a
failure that survives the whole budget is reported over the caps channel under
a new `SEED_CAPABILITIES_TAG`, so it surfaces as a
`Diagnostic::ExpectedReplyMissing` instead of an empty map. Until a fetch
succeeds the **previous region's map stays in place** rather than being cleared:
its URLs are stale but mostly still answer, so a command issued mid-crossing
degrades instead of silently doing nothing (`unwrap_or_default()` used to hand
every one of the ~50 `caps.get(...)` sites an empty map). A region that really
advertises no `EventQueueGet` is logged as such.

The bevy runtime had the same defect one level down: its worker treated a
*failed* seed fetch and a region that advertises *no* event queue identically,
blocking for the next `Switch` in both cases — and since `CrossedRegion` is
itself an event-queue event, an agent whose seed fetch flaked could not cross to
produce that switch. `fetch_caps` now returns a `SeedOutcome` telling the two
apart, and the worker retries a failure on the same backoff budget (preempted by
a switch, via `recv_timeout`) before falling back to blocking.

Both neighbours fixed: `Client::connect`'s login client gets the 60 s timeout
the runtime's other three clients set, and the event-queue long poll now backs
off on an unreadable or unparsable body exactly as it does on a transport error
— in both runtimes.

Five tests: three pinning the bevy retry budget (a served map ends it, whatever
it advertises; consecutive failures back off then exhaust it; a success restores
it) and two pinning the tokio refetch's failure path — a region change with no
seed at all, and a seed nothing is listening on — each reporting exactly one
failure and installing no map. The second runs under `tokio::test(start_paused)`
so the whole backoff budget costs no wall clock.
