---
id: test-fake-grid-tests-stop-reading-client-events
title: Fake-grid integration tests stop reading the client's events and can stall it
topic: test
status: bugs
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
