---
id: server-fake-grid-top-objects-report
title: Fake grid — a top-objects report with rows in it, and a return that finds them
topic: server
status: done
origin: follow-up of [[viewer-region-top-objects]] (2026-09-13)
refs: [viewer-region-top-objects, test-firestorm-fake-grid-crosscheck]
---

Context: [context/protocol.md](../context/protocol.md).

The fake grid answers a `LandStatRequest` with an **empty** report
(`parcel_edits.rs`), on the reasoning that a region running no scripts and
simulating no physics has nothing to report. That was right while nothing
consumed the reply; now that the Top Scripts / Top Colliders windows do
([[viewer-region-top-objects]]), an empty answer exercises one line of the
window and nothing else — the list, the sort, the selection, the id readout, the
`DataExtended` columns and every action are all untested offline, and the live
OpenSim had to stand in for them (which needed busy-loop scripts merge-loaded
into a region by hand).

Two pieces, and the second is a fidelity bug rather than a missing feature.

## A report with rows

Serve rows built from the scene the scenario already describes — the objects
are there, with ids, names, owners and positions, and the parcel each stands on
is resolvable. What a fake region cannot measure is the **score**, so the
scenario states it: which objects are "busy", and how busy.

- Honour the request's `ReportType`: the scripts report and the colliders report
  are different lists, and a scene can declare an object busy in one, the other,
  or both.
- Honour `ParcelLocalID`: `0` is the whole region, anything else is that parcel
  (`requestFlags & 1` in OpenSim's own handler).
- Honour the three filter flags (`STAT_FILTER_BY_OWNER` / `_BY_OBJECT` /
  `_BY_PARCEL_NAME`) as OpenSim does — a case-insensitive `contains` on the
  owner name, the object name, or the parcel name. This is the only way the
  window's three filter rows can be tested at all, since they filter
  *region-side*.
- Fill the **`DataExtended`** half (parcel name, rez timestamp, script memory,
  public URL count, owner id) — the event-queue form carries it and the window
  has four columns for it. A grid that left it out would let those columns rot.
- Keep answering over the **event queue**, as it does now.

## A return that finds what the report named

`ParcelObjectsReturned` matches objects by `parcel_at_position(...) ==
Some(local_id)`, so a **region-wide** return — `LocalID = -1` with an explicit
task-id list, which is exactly what the reference's top-objects return sends and
what our window sends — matches nothing and silently does nothing. OpenSim
branches on `localID == -1` and returns the named objects wherever they stand
(`LandManagementModule::ReturnObjectsInParcel`); the fake grid should take the
same branch, for the return and for `ParcelDisableObjects`.

A disable has no observable effect in a grid that runs no scripts, so the
honest end of that path is that the request is accepted and the objects stay —
worth a test that says so, rather than an unhandled event.

## Worth a test

A `client_end_to_end`-style test that asks for both reports, checks the rows and
their extended half, narrows with each filter flag, returns the objects the
report named, and sees them killed. That is the whole window's protocol surface
exercised with no grid, no scripts and no viewer.

## Done (2026-09-13)

`SceneFixtures::object_costs` — an
[`ObjectCost`](../../sl-fake-grid/src/world.rs) per object: the script time, the
collision count, and the script memory and public-URL count the `DataExtended`
half carries. Every field is **stated by the scene**, because a fake region
measures nothing; a cost of zero in a report's own unit keeps the object out of
that report, the way a simulator's own threshold does.

The stock scene costs its one scripted object (`STOCK_SCRIPT_TIME`,
`STOCK_SCRIPT_MEMORY_BYTES`), so the default grid answers a top-scripts request
with a row. The catalogue costs three prims — two scripted with different
scores, one colliding and not scripted — so a report there has an order to get
wrong and the two reports are not the same list.

`parcel_edits::land_stat_rows` builds the reply: the report's own unit, the
parcel scope (`ParcelLocalID`, honoured with OpenSim's by-parcel flag), the
three name filters as a case-insensitive `contains` on the owner / object /
parcel name, the `DataExtended` half, highest score first, capped at OpenSim's
hundred rows. Still answered over the event queue.

`on_scope` fixes the return: `LocalID = -1` means **the whole region**, which is
what the reference's top-objects return sends and what
`LandManagementModule::ReturnObjectsInParcel` branches on. The fake grid matched
only by parcel, so that request was accepted and silently did nothing.
`ParcelDisableObjects` is answered too — a region that runs no scripts has
nothing observable to do, and saying so beats leaving the event unhandled.

Verified by `client_end_to_end`'s
`a_top_objects_report_lists_the_scene_and_a_return_finds_it`: the report lists
the stock object with its extended half, a filter that matches nothing empties
it and the object's own name fills it again, the collider report is empty rather
than missing, the region-wide return kills the object, and the report is empty
afterwards. The whole surface of the Top Scripts / Top Colliders
windows, with no grid and no viewer.
