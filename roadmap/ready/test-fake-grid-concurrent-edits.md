---
id: test-fake-grid-concurrent-edits
title: Somebody else changed it and nobody was told
topic: test
status: ready
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
"changed by somebody else". None of it is answerable today at any tier.

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

## What a simulator pushes, and what the fake grid has

A real simulator tells everyone who is looking, and "looking" is a different
subscription per surface. That is the whole design question here.

- **A parcel** — an unsolicited `ParcelProperties` with sequence id `0`.
  The fake grid *already sends this shape* on arrival
  (`UNSOLICITED_SEQUENCE_ID`), so the parcel case needs only a reason to send
  it again, not a new sender.
- **An object** — the full `ObjectProperties`, whose `inventory_serial` is
  also how a prim's contents change is announced. Two gaps here:
  `send_object_properties_family` exists but the **full form has no sender at
  all**, and `ObjectSelect` / `ObjectDeselect` are still raw-forwarded, so no
  simulator built on `SimSession` knows *who has an object selected* — which
  is exactly the subscription the push goes to.
- **A region / estate** — `RegionInfo` and the estate replies, of which the
  fake grid answers one method today.

The carrier already exists in the middle: [[test-fake-grid-object-write-path]]
made the region's world shared and gave it a `RegionUpdate` broadcast with a
per-session watcher, precisely so one session's change reaches the others.
`Rezzed` and `Killed` are the two variants it has; everything above is
another variant and another `send_*` at the far end.

Wanted:

- typed `ServerEvent`s for `ObjectSelect` / `ObjectDeselect` and per-session
  selection state, since a push needs a subscription;
- `SimSession::send_object_properties` — the full form, carrying
  `inventory_serial`;
- more `RegionChange` variants, and the watcher turning each into the message
  the surface uses: object properties to selectors, a sequence-0
  `ParcelProperties` to the parcel's occupants, region info to the region;
- a two-avatar offline case per surface: both look, one writes, the other is
  told without asking, and a read from either returns the survivor.

Most of this needs [[test-fake-grid-edit-surfaces]] first — there is nothing
to broadcast until a client edit sticks. The exception is the prim task
inventory, whose write path landed with the shared world, so the notecard /
script collision is the one instance that can be built and tested now. That
is why this is `ready` rather than `blocked`.

What a **real** grid does on the collision is still worth one run to confirm
rather than assume — whether anything arbitrates at all, whether the losing
viewer is told unasked, whether a script save resets the script, and whether
a prim's contents serial advances on an in-place asset replacement (if it
does not, a cached listing stays "valid" while naming a stale asset and a
viewer cannot notice at all). [[test-asset-save-mutation-survey]] measures it;
this task supplies the mechanism. The expected finding is "nothing arbitrates",
and that is worth recording explicitly, because it is what makes convergence
the viewer's job rather than the grid's.

Acceptance: two fake-grid avatars looking at the same thing — a prim's task
inventory to begin with — both change it; the second change reaches the first
avatar unasked, carrying the surface's freshness marker; a read from either
returns the surviving record; and the first avatar's next write does not
revert the fields it never touched.
