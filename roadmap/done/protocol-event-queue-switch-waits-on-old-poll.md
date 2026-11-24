---
id: protocol-event-queue-switch-waits-on-old-poll
title: The Bevy event-queue worker can miss a region switch for a whole long poll
topic: protocol
status: done
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Confirmed by reading the code. Not reproduced live. The Bevy driver only: the
tokio driver aborts its poller.

## Fix (2026-11-24)

Each long poll now runs on its own thread, and the worker waits on whichever
comes first, a returned poll or a switch. The worker keeps a record of every
queue with a poll still open, including queues it has left, and never starts a
second poll on one of them, so returning to a region waits for that region's
old poll. Batches from an abandoned poll are still delivered but no longer move
the ack. Two loopback tests cover the switch and the return.

Live on aditi, 14 world-map teleports at 30 s per region all arrived, with the
queue re-targeted on each. This did not cure
[[viewer-teleport-back-gets-no-answer]], which reproduced at 60 s per region
with the queue switching promptly.

## Bad order

A batch carrying `TeleportFinish` or `CrossedRegion` is delivered, and the
driver's switch to the new region comes more than 2 s later. That is normal for
a teleport, because the switch waits for the UDP arrival.

## Cause

`run_event_queue` (`sl-client-bevy/src/caps.rs`) waits 2 s for the switch, then
enters a blocking long poll on the old queue. Commands are only read between
rounds, and OpenSim holds a poll for 58 s.

## Effect

Until the old poll returns, the new region has a stale capability map and no
event queue. Neighbours do not appear, a further `CrossedRegion` is missed, and
the next teleport's finish goes unseen, so it fails at 30 s.

## Fix

Make the poll interruptible by a switch, or use a short poll timeout while a
region change is pending. Polling again with the same ack is safe.
