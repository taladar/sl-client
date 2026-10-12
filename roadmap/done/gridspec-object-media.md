---
id: gridspec-object-media
title: Media on a prim: ObjectMedia, navigation and propagation on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-media-prim-browser,
  server-fake-grid-object-media-region, gridspec-lsl-prim-state]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-12)

Measured and written up in `book/src/gridspec/media.md` § Media on a prim.

- **Discover.** A new case, `object-media`: one avatar rezzes a cube, a
  second stands by, and the case POSTs each request to the capability
  itself (status and body), listens on both for the cube sent again and
  reads the media back. A prim with no media and the requests a region
  cannot serve; a set, the same set again, and the other avatar's read;
  navigation to a page, to the same page, to nothing, to text that is no
  URL, to a `data:` URL, of a bare face, of a face the prim has not got,
  and by the other avatar; navigation and a set by the other avatar under
  owner-only and nobody permissions; the whitelist, on, off it and empty;
  seven, one and no records for six faces, and media on another face; the
  face's media flag cleared and set over `ObjectImage`, and an
  `ObjectImage` stating another version; the client's own commands; a
  clear. Four runs on aditi (Mauve) and four on OpenSim, held to 187
  measured answers.
- **Findings.** The version is `x-mv:<10 digits>/<agent>` on both, up by 1
  for every accepted request whether or not anything changed, and names
  whoever sent it. Both tell both avatars with a full update carrying the
  version and never the media. Second Life refuses with **`200` and an
  `error` map** (`code`, `message`), OpenSim with `400`. An address that is
  no URL is kept by both (`https://not a url` on Second Life). A stranger's
  `UPDATE` changes nothing on either — and on OpenSim moves the version and
  is announced. OpenSim refuses the owner under `perms_interact` nobody and
  everything under an empty whitelist; Second Life neither. A short
  `UPDATE` on OpenSim writes face 0 and fails with nobody told. Clearing a
  face's media flag takes its media away on both without moving the
  version; setting it again brings an empty record on Second Life and
  nothing on OpenSim. OpenSim never clears the flag and does not set it on
  a face given media later, which then shows none.
- **Fake grid.** Not here, as planned:
  [[server-fake-grid-object-media-region]] is now unblocked and has the
  measured model; the case lists the live grids only until it lands.
- **Viewer.** The refetch on a new version was there and is now tested. Two
  gaps the census found are fixed: `MediaEntry::from_llsd` refused a record
  whose address is no URL, which cost the viewer the media of the whole
  prim (it now reads the address as none, with a warning), and a face that
  regained its media flag at an unchanged version kept the record held from
  before (it is fetched again, as the reference does). An `e2e` test on the
  fake flavours waits for the fake grid to version media at all.
- **Not measured.** Media on a child prim and on an attachment, the group
  bit, a prim taken and rezzed or copied, a prim with another face count,
  what a late arrival is handed, and what a script sets
  ([[gridspec-lsl-prim-state]]).

## Known already

The fake grid stores ObjectMedia per session and never bumps `MediaURL`; nothing
is measured live.

## Discover

An `object-media` case: set, navigate, version string format, propagation to a
second avatar, permission refusal, whitelist; both grids.

## Document

`book/src/gridspec/media.md` § Media on a prim.

## Fake grid

Large — [[server-fake-grid-object-media-region]].

## Viewer

Refetch on version change.
