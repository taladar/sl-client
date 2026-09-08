---
id: viewer-teleport-reset-defeated-by-a-v1-grid
title: A grid that announces the teleport destination first suppresses the world reset
topic: viewer
status: wont-do
origin: split out while fixing [[viewer-teleport-never-resets-the-world]] (2026-09-08)
refs: [viewer-teleport-never-resets-the-world]
---

Context: [context/viewer.md](../context/viewer.md).

**Decided against on 2026-09-08: no grid this viewer talks to does the thing
this would defend against.**

## What it was

`Session::begin_handover` keeps the departed world when the destination is
already a child circuit:

```text
let dest_is_child = self.children.contains_key(&dest);
```

The case that clause is written for is real — following a vehicle across
several regions can leave a still-held child two or three regions off, and
teleporting to it should not throw the scene away. But the test it uses cannot
tell that child from a **destination the simulator announced as part of this
very teleport**, and one shape of grid does exactly that: OpenSim's legacy
`TransferAgent_V1` sends `EnableSimulator` + `EstablishAgentCommunication`
before the `TeleportFinish` — and, being the `OutSideViewRange` branch, sends
them precisely for the distant destinations that *should* reset. There the
predicate is not merely defeated, it is inverted.

## Why not

Nothing sends that announcement.

- **Second Life does not.** It is what `TransferAgent_V2` was written to match,
  and OpenSim says so where it sends the finish: *"New protocol: send TP Finish
  directly, without prior ES or EAC. That's what happens in the Linden grid."*
  SL is what this viewer targets, so this is the case that decides it.
- **Current OpenSim does not.** V2 is taken whenever the destination simulator
  reports protocol 0.2 or newer; V1 is the fallback for a destination older
  than that, which is not a thing the local test grid or any maintained OpenSim
  is.
- **The fake grid no longer does.** Its V1-shaped announcement was the whole of
  [[viewer-teleport-never-resets-the-world]], and it is fixed.

So the defended-against grid is hypothetical, and the fix is not free: the
discriminator would have to be *when* the child circuit appeared, snapshotting
`children` at each of the four sites that enter `SessionState::Teleporting` —
and that snapshot is contaminated for a teleport the **simulator** decided on,
which enters `Teleporting` at `enter_remote_teleport`, i.e. *at* the
`TeleportFinish`, by which point a V1 grid's announcement has already landed.
Carrying that machinery, and its known hole, for a grid nobody runs is worse
than not carrying it.

The blast radius if the hypothetical grid did turn up is also small. The
reference viewer has no reset of its own at all — `LLWorld` drops a region only
on `DisableSimulator` — so a client that falls back to circuit retirement there
is exactly as good as Firestorm, except for the two world-scoped stores not
keyed by circuit (`RiggedBindSkipLog`, `ObjectCostModel`), which would hold
stale entries rather than corrupt anything.

## What would reopen it

A live grid observed keeping the world across a genuinely distant teleport.
`detect_world_reset` logs which way the flag fell on every region change, at
`info` — *"the world is kept and re-based"* after a jump across the grid is the
symptom, and it now costs nothing to notice.
