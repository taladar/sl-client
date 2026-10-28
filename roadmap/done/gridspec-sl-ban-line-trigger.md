---
id: gridspec-sl-ban-line-trigger
title: Pin down when Second Life pushes a ban line
topic: gridspec
status: done
origin: gridspec-parcel-access-and-ban-lines (2026-10-05)
refs: [gridspec-parcel-access-and-ban-lines, viewer-parcel-ban-lines-on-refusal,
  server-fake-grid-parcel-access-enforcement, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

A ban line on Second Life is a full `ParcelProperties` for the closed parcel
under sequence id `-20000` / `-30000` / `-40000`, over the event queue
(`book/src/gridspec/land.md` § The ban line). `parcel-ban-line` saw it on
aditi — four records 0.4–0.5 s apart, starting 0.2 s after the arrival push —
on **two of four** logins that landed 7–12 m from a parcel closed by list and
group, and on **none** of a dozen approaches: flying into the line (refused,
alerted, lifted over it), walking into it (refused, alerted, stopped), standing
beside it.

What the two logins that got it had in common, and the two that did not: the
previous session had ended *in the air* (a level flight let go a few metres
up) rather than on foot, so the avatar may have arrived falling. The parcel
under the avatar differed too (16 and 14 against 7).

## Discover

What decides the push. Candidates, cheapest first:

- arrive beside the parcel in the air and on the ground, several times each,
  and count (`parcel-ban-line` parks the avatar 10 m out; a variant that ends
  hovering is a one-line change);
- fall, or walk slowly, within a few metres of the line without touching it;
- capture the reference viewer doing the same on aditi
  (`sl-conformance-trace` over a pcap with `LogMessages`) and read when its
  `ParcelProperties` under `-20000` arrive — and whether anything the viewer
  sends precedes them.

A ban (`-30000`) needs land ([[gridspec-aditi-test-land]]).

## Document

`book/src/gridspec/land.md` § The ban line: the "when it is pushed" cell.

## Fake grid

The rule goes into [[server-fake-grid-parcel-access-enforcement]] for the
Second Life flavour.

## Viewer

[[viewer-parcel-ban-lines-on-refusal]] must not depend on the answer: it has
to work when no line was pushed.

## Outcome (2026-10-28)

**The camera decides, not the avatar.** A new probe case, `ban-line-trigger`
(aditi; the `parcel-ban-line` helpers it shares are `pub(super)` now), moves
the agent's camera (`AgentUpdate`'s `CameraCenter` / `CameraAtAxis`) around
the closed parcel and counts the ban lines in each place, eight or six seconds
a place. Five aditi runs:

- run 1 moved the avatar and the camera separately: hovering, falling and
  walking into the line with the camera at the region's middle brought none;
  the camera alone beside the line brought one every 0.53 s with the avatar
  171 m away;
- runs 2–5 were all camera. Lines come while part of the parcel's column (the
  ground to 50 m above it) is in the camera's view within about 20 m: 21 m
  from the nearest square's centre yes, 22 m no; straight over it 68 m above
  the ground yes, 70 m no; 17 m from the column's top edge looking at it yes,
  looking down past it no; 23 m from it no either way; turned 75° aside at
  4 m yes, 90° no; 45° aside at 16 m yes, 60° no. The draw distance (`Far`)
  makes no difference.

This explains `parcel-ban-line`'s record: its session never set a camera, so
the camera sat at the region's middle and no approach brought a line; the
arrivals that got four lines in their first two seconds were, presumably,
seen through the avatar before the session's first `AgentUpdate` was read.
That last part is inferred, not measured. The run that would measure it needs
the avatar parked facing the parcel. It is now in Bonifacio, and twice a
three-minute flight from there towards the parcel stuck at 174/102. Booked in
`book/src/gridspec/land.md` § What makes Second Life push it. The rule is
written into [[server-fake-grid-parcel-access-enforcement]]. The viewer sends
its real camera, so it gets the lines as the reference does, and
[[viewer-parcel-ban-lines-on-refusal]] already keeps a line that came.
