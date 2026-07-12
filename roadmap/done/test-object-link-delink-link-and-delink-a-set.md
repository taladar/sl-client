---
id: test-object-link-delink
title: link and delink a set
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 8 — Objects & scene graph `[both]`
---

Context: [context/test.md](../context/test.md).

`object-link-delink` — link and delink a set. `1av`. Both halves of
the build-tool link operation, against a self-manufactured set so the round
trip is self-contained: **create** three throwaway cubes with
[`Command::RezObject`] (`ObjectAdd`) spaced above a reference primitive —
one root plus two children, the smallest genuine *set* (as opposed to a
two-prim pair) — each an [`Event::ObjectAdded`] with a region-local id not
seen during the initial scene settle; **link** them into one linkset with
[`Command::LinkObjects`] (root id first, `ObjectLink`), verified by each
child re-broadcasting as an [`Event::ObjectUpdated`] whose
[`Object::parent_id`] now points at the root; **delink** with
[`Command::DelinkObjects`] (`ObjectDelink`), verified by each former child
re-broadcasting with its parent back to zero; then **clean up** by derezing
the whole set to Trash ([`Command::DerezObjects`] /
[`DeRezDestination::Trash`]), each confirmed by an [`Event::ObjectRemoved`]
(`KillObject`), leaving the scene as found. OpenSim's `ObjectLink` handler
links only same-owner prims and needs no prior selection, so the fresh
same-owner set links cleanly; the child's local id is preserved across the
link (only its `ParentID` changes, no `KillObject`), which is what makes the
re-parenting observable as an update rather than a remove/re-add. Reuses the
`start_location` hook and the settle/reference machinery of
`object-rez-derez` (Default Region on OpenSim, where the workspace's rezzed
test object is the placement reference; a no-primitive landing region records
`partial` on SL). No new client code — the
`LinkObjects`/`DelinkObjects`/`RezObject`/`DerezObjects` surface all existed;
only the new case. Green on OpenSim: create / link / delink / delete all
confirmed, link and delink RTTs ≈ 90 ms. The Trash cleanup leaves three
items per run — bounded inventory residue, acceptable on a throwaway grid.
`[both]`; the aditi run is deferred with the batch (no aditi record this
session).
