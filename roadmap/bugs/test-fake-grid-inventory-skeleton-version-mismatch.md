---
id: test-fake-grid-inventory-skeleton-version-mismatch
title: Login skeleton reports folder version -1 while AIS reports 8, crashing the viewer
topic: test
status: bugs
origin: first Firestorm cross-check harness run (2026-09-01)
points: 3
refs: [test-firestorm-crosscheck-runner, test-fake-grid-catalogue-clears-inventory-root]
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
