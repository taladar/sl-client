---
id: viewer-teleport-reset-defeated-by-a-v1-grid
title: A grid that announces the teleport destination first suppresses the world reset
topic: viewer
status: ready
origin: split out while fixing [[viewer-teleport-never-resets-the-world]] (2026-09-08)
refs: [viewer-teleport-never-resets-the-world]
---

Context: [context/viewer.md](../context/viewer.md).

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

`TransferAgent_V2` — taken whenever the destination simulator speaks protocol
0.2 or newer, which every current OpenSim and Second Life itself do — sends no
such announcement, so this is a legacy-grid concern rather than a live one.
That is why it is split out rather than folded into
[[viewer-teleport-never-resets-the-world]], which fixed the same symptom's
much larger cause (the fake grid was V1-shaped).

## The shape of a fix

The discriminator is *when* the child appeared: a child held from **before** the
teleport was requested is the vehicle case; one that appeared during the
handover is the destination. Snapshotting `children`'s addresses at each of the
four sites that enter `SessionState::Teleporting` would capture that exactly.

The hole in that plan, and the thing to decide before writing it: a teleport the
**simulator** decided on enters `Teleporting` at `enter_remote_teleport`, i.e.
*at* the `TeleportFinish`, by which point a V1 grid's announcement has already
landed. A snapshot taken there is already contaminated. Either that corner is
accepted and written down, or the discriminator has to be something other than a
snapshot.

Worth weighing against doing nothing: the reference viewer has no reset of its
own at all — `LLWorld` drops a region only on `DisableSimulator` — so a client
that merely falls back to circuit retirement on a V1 grid is no worse than
Firestorm there. The argument for fixing it is that the stores *not* keyed by
circuit (`RiggedBindSkipLog`, `ObjectCostModel`) have no retirement to fall back
on.
