---
id: test-parcel-object-owners
title: request object owners / return objects
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

`parcel-object-owners` — request object owners / return objects.
`1av`. Runs as the estate-owner avatar (`--avatar estate-owner`), who owns the
region-wide parcel on the local grid, since both the object-owners request and
the object return need land rights. Exercises the two halves of the land
panel's "Objects" tab as a self-contained rez-tally-return-tally cycle that
leaves the region as found: (0) learn the region-centre parcel's local id and
owner from a `ParcelPropertiesRequest` reply (confirm we own it) and build a
[`ScopedParcelId`]; (1) request the per-owner tally with
`ParcelObjectOwnersRequest` ([`Command::RequestParcelObjectOwners`] →
[`Event::ParcelObjectOwners`], a `ParcelObjectOwnersReply` over UDP whose rows
are [`ParcelObjectOwner`]s) as a **baseline**, asserting we own no objects on
the parcel yet — the return returns objects *by owner*, so a clean baseline
guarantees the cycle touches only this case's throwaway object; (2) rez a
throwaway cube ([`Command::RezObject`], `ObjectAdd`) at the region centre,
identified as the first [`Event::ObjectAdded`] with an id not seen while the
initial scene settled; (3) re-request the tally and assert our owner now reads
one prim higher; (4) return our parcel objects with `ParcelReturnObjects`
([`Command::ReturnParcelObjects`], `ParcelReturnType::LIST` scoped to our
owner id — mirroring the viewer's "Return objects owned by \<owner\>" button,
whose owner ids the reference `LandObject.ReturnLandObjects` matches against
`primsOverMe`), confirmed by the [`Event::ObjectRemoved`] (`KillObject`) for
the cube's id; (5) re-request the tally a final time and assert our owner is
back to the baseline. New client code: only the `ParcelObjectOwner` re-export
from both `sl-client-tokio` and `sl-client-bevy` (it appears in the public
`Event::ParcelObjectOwners` variant but was missing from the re-exports — same
gap the earlier Phase 10 cases closed); the `RequestParcelObjectOwners`/
`ReturnParcelObjects` command surface and the `ParcelObjectOwnersReply` decode
all already existed. **Green on OpenSim's
Default Region:** the estate owner starts with no objects on the region-wide
parcel (`local_id` 1), the cube tallies as one prim (owner count 0 → 1), and
the return removes exactly that cube (owner count 1 → 0), leaving it in the
estate owner's Lost and Found. A ~2 s settle after each edit lets the
simulator update its per-parcel tally before the readback. `[both]`; the aditi
run is deferred with the batch — but like `parcel-divide-join` it likely needs
a **full owned region** (the fixed region-centre rez assumes we own the
region), so the aditi leg may be infeasible without a suitable owned parcel
and dynamic coordinates.
