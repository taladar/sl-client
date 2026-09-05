---
id: test-fake-grid-concurrent-edits
title: Somebody else changed it and nobody was told
topic: test
status: done
origin: scoping test-fake-grid-asset-round-trip (2026-09-05)
points: 5
refs:
  [
    test-fake-grid-edit-surfaces,
    test-fake-grid-asset-round-trip,
    test-asset-save-mutation-survey,
    test-fake-grid-object-write-path,
    test-object-properties,
    viewer-task-inventory-open-and-save-back,
    viewer-floaters-never-reread-after-a-push,
  ]
---

Context: [context/testing.md](../context/testing.md).

The third way a grid's copy differs from a viewer's, and the only one that is
not about the save path at all: **another avatar changed it.** It is not
special to inventory. Two residents in the same About Land floater, two
builders with the same prim selected, two estate managers in the region
console, two people editing one prim's notecard or script — in every case the
loser is left showing a record the grid no longer holds, and can overwrite
the winner's change without ever seeing it.

For a viewer the failure is identical on every surface: a floater that keeps
its own edited copy, never re-reads, and cannot tell "unchanged" from
"changed by somebody else".

## There is nothing to arbitrate with

Second Life has no edit lock, no two-phase commit and no consensus: selection
is a subscription, not a mutex, and two residents may hold the same object or
the same About Land form open indefinitely. Latency alone therefore makes
conflicting edits *always* possible — a conflict is not an error case a grid
prevents, it is the steady state, and last-write-wins is very probably the
whole of the grid's policy.

Which moves the burden onto the viewer, and changes what is worth testing.
The interesting bug is not "loses the race" — somebody has to — it is
**silently reasserting stale state afterwards**. `ParcelPropertiesUpdate` is
the sharpest case: it carries the *whole* record (its own doc says "start
from `ParcelUpdate::default` and set the fields to change"), so a floater
populated from a stale read, with one checkbox flipped, sends every other
field back as it was minutes ago and quietly reverts whatever somebody else
changed in between. `MultipleObjectUpdate` and the estate forms have the same
shape.

So the property under test is convergence, not arbitration: after a push, a
viewer's *next* write must carry the pushed values for the fields it did not
itself touch.

And this is a reason to stage it offline rather than live. Without locking, a
live grid's interleaving is luck; the fake grid's region lock already
serialises writes, so a test can stage exactly "A reads, B writes, A writes"
and get the same answer every run.

## Done

Four new `RegionChange` variants, each going to a different set of sessions,
because each surface's subscription is a different question:

- **`PropertiesChanged`** goes to the sessions holding the object *selected*
  and to nobody else. `ObjectSelect` / `ObjectDeselect` were already typed by
  [[test-fake-grid-edit-surfaces]]; what was missing was that anything
  *remembered* them, so each session now keeps an `object_edits::Selection`
  that a select opens and a deselect closes, and `run_region_watcher` consults
  it before forwarding. `SimSession::send_object_properties` — the full form,
  which the task listed as wanted — turned out to exist already.
- **`ParcelChanged`** goes to the avatars standing on the parcel, which is
  OpenSim's `SendLandUpdateToAvatarsOverMe`, as a sequence-zero
  `ParcelProperties`. The fake grid tracks no movement, so "standing on" is
  where the session arrived; telling the whole region instead would have made
  a test of *who is told* pass for the wrong reason.
- **`RegionConfigured`** goes to everyone in the region. There is no
  subscription to belong to: every avatar is standing in it.
- **`TerrainRetextured`** likewise, and is the odd one: a terrain composition
  travels only in a `RegionHandshake`, which is stamped with the *receiving*
  session's identity — so the region publishes the composition and each
  watcher builds its own message. `texturecommit`'s whole purpose is that
  everybody sees it, and until now only the estate manager who applied it did.

The prim task inventory rides on the first of those and was the reason to do
it. A contents serial is a prim's whole freshness marker and it is a field of
the properties record, travelling nowhere else — so `UpdateTaskInventory` now
pushes the record as well as advancing it, to the writer and to every other
selector. Without that a resident with the prim's contents open holds a
listing the region no longer has and cannot tell.

One two-avatar case per surface in `client_end_to_end`, each staging "both
look, one writes, the other is told unasked, a read from either returns the
survivor". The parcel case runs the whole argument end to end: a save built
from the record read at open reverts the other resident's rename, and a save
built from the pushed record does not. The object case also pins the
*negative* — a resident who deselects stops being told — which is what makes
the push a subscription rather than a broadcast with extra steps.

Two things found on the way. `answer_estate_request` returned a `bool` the
driver discarded, so a region save reached nobody; it returns changes now.
And `setregioninfo`'s numeric parameters are formatted `"%.6f"` by the
reference client, while the estate parser took digits alone — so the region's
agent limit was read as absent and that half of every Region-tab save was
silently dropped. It is the two-avatar test that asked the question that
found it.

Deliberately not done: an `estatechangeinfo` still answers only the manager
who sent it. An `EstateInfo` reply is correlated by the *invoice* the request
carried, and what a simulator puts in that field for an unsolicited push is a
guess this grid should not make until a live run says.

The viewer half is [[viewer-floaters-never-reread-after-a-push]]: nothing in
the viewer consumes any of these pushes as a re-read, and About Land seeds its
edit draft exactly once per open.

What a **real** grid does on the collision is still worth one run to confirm
rather than assume — whether anything arbitrates at all, whether the losing
viewer is told unasked, whether a script save resets the script, and whether
a prim's contents serial advances on an in-place asset replacement (if it
does not, a cached listing stays "valid" while naming a stale asset and a
viewer cannot notice at all). [[test-asset-save-mutation-survey]] measures it;
this task supplied the mechanism. The expected finding is "nothing
arbitrates", and that is what makes convergence the viewer's job rather than
the grid's.
