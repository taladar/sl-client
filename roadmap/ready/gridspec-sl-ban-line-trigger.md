---
id: gridspec-sl-ban-line-trigger
title: Pin down when Second Life pushes a ban line
topic: gridspec
status: ready
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
