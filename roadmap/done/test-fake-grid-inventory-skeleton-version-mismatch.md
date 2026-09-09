---
id: test-fake-grid-inventory-skeleton-version-mismatch
title: Login skeleton reports folder version -1 while AIS reports 8, crashing the viewer
topic: test
status: done
origin: first Firestorm cross-check harness run (2026-09-01)
points: 3
refs: [test-firestorm-crosscheck-runner, test-fake-grid-catalogue-clears-inventory-root]
---

Done (2026-09-09). The crash half was already gone —
[[protocol-sim-caps-inventory]]'s follow-up commit seeded the nineteen
system folders, and Firestorm has reached `STATE_STARTED` since. What
remained was the accounting, and it
was four separate defects in the AIS surface, each found by reading one
Firestorm run's log against `llaisapi.cpp` and each fixed and re-measured
against the next run.

**`?depth=0` meant "nothing".** It is the reference's own parameter and
it counts levels of recursion *below* the listing, so `depth=0` asks for
a folder's own children — `LLInventoryModelBackgroundFetch` sends exactly
that for every ordinary folder fetch. We read it as a level count and
answered the commonest fetch there is with the folder alone. Now
`SimInventoryTree::listing_to_depth` lists the folder and `depth` further
levels below it.

**A listing did not name what it held.** `AISUpdate::parseDescendentCount`
believes a descendent count only from an `_embedded` block naming all
three of `categories`, `links` and `items`, and `parseCategory` refuses to
record a folder's `version` until it has that count ("don't set version
unless correct children count is present"). We omitted `_embedded`
entirely when a folder had no children, so a viewer held those folders at
version *and* count unknown for the session — which is what every
`Accounting failed … version: unknown (-1)` line was. All three keys are
emitted for every folder the listing opened, empty or not, and a folder
the listing did *not* open carries no `_embedded` at all: "holds nothing"
and "was not fetched" are different answers, and the new
`InventoryListing`/`InventoryListingChildren` pair is that distinction.

**The subtree was flattened.** [[protocol-sim-caps-inventory]] served a
recursive fetch as one top-level `_embedded`, deliberately, because our
own client parser read only that level — information-equivalent for us
and useless to the reference, which attributes children by nesting. The
serializer now nests one level per level of the listing, as the real
service does, and the client parser recurses (`gather_ais_embedded`) so it
keeps seeing the whole reply. `&children=<ids>` is honoured too, as the
subset fetch it is.

**A created folder came back nameless.** The reference's create body is
`{ categories: [ { category_id, parent_id, type_default, name } ] }`
(`LLInventoryCategory::asAISCreateCatLLSD` wrapped by
`LLInventoryModel::createNewCategory`); our parser read a flat
`{ name, type }` and so filed every folder a real viewer created under an
**empty name**. The viewer then could not find the folder it had just
made and made it again — `no #Firestorm folder yet. Creating …` six times
in one session. The parser now accepts both shapes and our builder emits
the reference's, and a create reply states the new folder as *empty*
rather than unlisted, so its version is recorded instead of unknown.

**One fixture fix fell out of it.** The stock account holds a mesh, and
no seeded folder had its class, so `class_folder` filed it at the agent
root — where the reference viewer's model does not keep it, leaving the
root's count one higher on the grid than in the viewer for the whole
session. `FT_MESH` (49) is now a seeded system folder like the rest.

Measured across four Firestorm runs against the stock scenario
(`sl-crosscheck --only firestorm`), before → after:

| | before | after |
| --- | --- | --- |
| `Accounting failed` | 13 | 1 |
| `version mismatch` | 13 | 1–4 |
| `server == -1` (count unknown) | 7 | 0 |
| `#Firestorm` re-created | 6 | 1 |

What is left is not this defect. The single remaining `Accounting failed`
is a ±1 at the moment the viewer creates a folder (the grid states the
parent's count from before the create while the viewer has already added
the child), and the remaining `version mismatch` lines are all on
`#Firestorm` while the Firestorm bridge mutates it concurrently — replies
to concurrent AIS requests arriving out of order, which the reference
handles by design ("Adjusting local version") and which a real service
has too. Telling those apart from Second Life's own behaviour needs a
real-grid capture, not another fake-grid change.

Covered by the rewritten `ais3_children_fetch_honours_depth` (depth 0 is
a listing; nesting one level inside another; all three keys; an unopened
folder carrying no `_embedded`; a subset fetch), the new
`the_login_skeleton_and_ais_agree_about_folder_versions` end-to-end test
(the check this task asked for: log in, then walk AIS and compare), and
the sl-wire round trips for both create-body shapes.

`sl-fake-grid` also grew the request log it did not have: every capability
exchange at `debug` (which URL, what status) and its bodies at `trace`.
Every finding above came from reading a real viewer's log against this
grid's, and this side had nothing to read.

---

Context: [context/testing.md](../context/testing.md).

Logging Firestorm into the **stock** scenario (no `--catalogue`, so
[[test-fake-grid-catalogue-clears-inventory-root]] is out of the way) gets all
the way through `STATE_INVENTORY_SEND2` and then dies:

```text
STATE_INVENTORY_SEND2 --> STATE_INVENTORY_CALLBACKS
WARNING accountForUpdate : Accounting failed for 'My Inventory'
                   version: unknown (-1)
WARNING doUpdate : version mismatch for category My Inventory,
                   viewer version -1 AIS version 8 !!!Adjusting local version!!!
ERROR   llpanelplaces.cpp(1334) showAddedLandmarkInfo : ASSERT (item)
```

The viewer then puts up its "Firestorm has crashed" dialog.

Two disagreements, one after the other. The login response's
`inventory-skeleton` gives the root category a version the viewer reads as
**-1 / unknown**, while the AIS (`/cap/…` inventory) surface reports that same
category at **version 8**. The viewer resyncs, and in doing so fires an
inventory-changed callback naming item ids the local model does not have —
which is what the assert trips on.

So the skeleton's `version` per folder must agree with whatever the AIS
surface will report for that folder, and the ids announced in a change
callback must be ids the client can actually fetch. Getting the first right
probably fixes the second.

The `showAddedLandmarkInfo` assert is genuinely too strict — the line right
after it already handles a null item, and the ids come from the network — and
has been patched locally in the Firestorm tree. Past it, the viewer stops
again, one layer deeper and for a better reason:

```text
WARNING Places : inventory-changed callback named item
                 00000000-0000-0000-0000-000000000002
                 which is not in the inventory model; ignoring
ERROR   llinventorymodel.cpp(986) findCategoryUUIDForTypeInRoot :
                 ASSERT (!isInventoryUsable())
```

**That second assert is correct and must not be patched.** It is not an
over-strict guard on network input; it is the viewer refusing to continue with
an inventory model it could not build. Silencing it would carry a known-bad
model into code that assumes a good one — trading a clean stop for a subtle
rendering difference, which is the exact failure mode the cross-check exists
to detect rather than manufacture.

Note the item id: `00000000-0000-0000-0000-000000000002` is a sequential
placeholder rather than a real asset id. Together with the version disagreement
that suggests the skeleton is synthesised without reference to what the AIS
surface will actually serve, and that one root cause produces all three
symptoms (the accounting warning, the phantom callback id, and the unusable
model). Fixing the skeleton should retire all of them at once; there is no
need to chase them separately.

Version accounting is exactly the sort of thing a round-trip test will not
catch, because it is a *relationship between two surfaces* (the login skeleton
and AIS) rather than a property of either. Worth a check that logs in and then
walks AIS, asserting the versions match.
