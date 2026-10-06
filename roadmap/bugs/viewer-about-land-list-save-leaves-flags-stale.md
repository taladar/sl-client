---
id: viewer-about-land-list-save-leaves-flags-stale
title: Saving an allow or ban list on OpenSim leaves About Land's access flags stale
topic: viewer
status: bugs
origin: gridspec-parcel-access-and-ban-lines (2026-10-05)
refs: [gridspec-parcel-access-and-ban-lines, viewer-parcel-options-access-media]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

OpenSim switches a parcel's `USE_ACCESS_LIST` / `USE_BAN_LIST` flag **on** when
a non-empty allow / ban list is saved and **off** when the list is emptied
(`LandObject.UpdateAccessList`; measured by `parcel-access-list`,
`book/src/gridspec/land.md` § The lists). It answers the
`ParcelAccessListUpdate` with nothing and pushes no parcel.

About Land (`sl-viewer-places/src/about_land.rs`) sends the list
(`send_access_list`) and keeps the parcel record it had. So after adding one
resident to the allowed list on OpenSim:

- the parcel is closed to everybody else, and the Access tab's "anyone can
  visit" box (`USE_ACCESS_LIST`, inverted) still shows it open;
- the next **Apply** on any tab re-asserts the whole record with the stale
  flag and opens the parcel again, silently undoing what the list did.

Second Life is not measured (no land); the reference viewer's reading is that
its flags are the checkboxes and nothing else.

## Fix

Read the parcel back after a list save where the grid may have changed it:
the rectangle form (`RequestParcelProperties` over one of the parcel's own
squares — OpenSim ignores the by-id form), merged like any other push for the
bound parcel. The OpenSim-flavoured fake grid changes the flag the same way
(`ParcelPolicy::list_update_sets_use_flag`), so an `e2e` test can add an
allowed resident and expect the checkbox to follow.
