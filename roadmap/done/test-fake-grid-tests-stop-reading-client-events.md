---
id: test-fake-grid-tests-stop-reading-client-events
title: Fake-grid integration tests stop reading the client's events and can stall it
topic: test
status: done
origin: gridspec-terrain commit runs (2026-10-08)
refs: [gridspec-terrain, gridspec-object-update-stream]
---

Context: [context/testing.md](../context/testing.md).

## Observation

`sl-client-tokio`'s run loop awaits the send of every event, so a session
whose events go unread stops once its channel is full — and with it stops
sending what it was commanded to. `sl-fake-grid/tests/determinism.rs` and
`tests/clock.rs` read events until the region handshake, then sent a chat
line and waited for the grid to hear it, with a channel of 256 and nobody
reading. A region's ground alone is more events than that. They had been
passing because the chat usually got out first; the per-message terrain
event added on 2026-10-08 made `determinism` fail every time and `clock`
once in three. Both now drain the channel on a task.

`tests/timeline.rs` (six sessions) and `tests/client_end_to_end.rs` (ten)
open the same 256-event channel. Some read it throughout; the ones that
stop reading, or read only until the event they wait for, have the same
stall waiting for a busier arrival.

## What to do

One helper for these tests that starts the client and hands back the events
through an unbounded forwarder, as `sl-conformance`'s `context.rs` does, and
every session opened through it.

## A wait that never ends (2026-10-09)

The same file's `wait_for` restarts its timeout with every event it reads.
A Second-Life-flavoured fake grid sends the wind every second since
[[gridspec-terrain]], so a wait for something that will not come no longer
ends at all: `the_arriving_agent_gets_its_own_appearance` waited for an
animation its first wait had already stepped over — the appearance comes
after the handshake reply since [[gridspec-object-update-stream]], and so
after the animation — and held a pre-commit run for two and three-quarter
hours. That test now asks for both in one `wait_until`, which has one
deadline. Every other `wait_for` in `client_end_to_end.rs` and
`timeline.rs` still has the per-event timeout; they belong on the one
deadline too.

## Resolution (2026-11-20)

`sl_fake_grid::run_client` (`sl-fake-grid/src/client_run.rs`) starts a
client's run loop behind a forwarder per channel that drains events and
diagnostics onto unbounded ones, and hands back a `RunningClient` (events,
diagnostics, command sender, run task). A forwarder whose reader has gone keeps
draining, so a dropped receiver no longer stalls the client either. It lives in
the library rather than a shared test module so every test binary can use it
without dead-code noise, and the book's fake-grid page says to use it.

All 26 hand-rolled `Client::run` starts under `sl-fake-grid/tests/`
(`client_end_to_end.rs` 11, `timeline.rs` 6, `scripted_avatars.rs` 4,
`determinism.rs` 2, `clock.rs`, `heartbeat.rs`, `scripts.rs`) now go through it.
The workarounds it replaces are gone: the drain tasks in `determinism.rs` and
`clock.rs` and `client_end_to_end.rs`'s `ignore_events`. The filtering
forwarders in `heartbeat.rs`, `scripts.rs`, `scripted_avatars.rs` and
`determinism.rs`'s transcript stay, as filters. The diagnostics channel was a
second instance of the same stall: every test kept its receiver unread at a
capacity of 16, which a diagnostics-enabled session could fill.

The one-deadline half was already done by the time this was picked up: every
event wait in `client_end_to_end.rs` and `timeline.rs` reads against one
`whole_wait()` deadline (`wait_on`) or one `timeout` around the whole loop
(`wait_until`); the last stale doc comment saying otherwise is corrected.

The grid's own `scripted_avatars` runner already drained its client through a
forwarder and is unchanged.
