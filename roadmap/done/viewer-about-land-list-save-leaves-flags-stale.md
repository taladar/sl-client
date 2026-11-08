---
id: viewer-about-land-list-save-leaves-flags-stale
title: Saving an allow or ban list on OpenSim leaves About Land's access flags stale
topic: viewer
status: done
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

## Done (2026-11-07)

- **Read-back after every list save** (`sl-viewer-places/src/about_land.rs`,
  `send_access_list`): the `UpdateParcelAccessList` is followed by a
  `RequestParcelProperties` over one point inside the parcel —
  `ParcelInfo::point_inside` (`sl-proto`), the centre of the first 4×4 m block
  its bitmap marks, since a bounding-box point can lie on a neighbour. Its
  fresh sequence id is awaited by no window, so the reply merges as a record of
  the bound parcel. OpenSim's handlers for both messages are synchronous
  (`LLClientView`, `AddLocalPacketHandler(…, false)`), so the read is answered
  after the list is applied. On Second Life it is a harmless re-read.
- **Flags merge per checkbox** (`sl-proto/src/types/merge.rs`,
  `merge_unedited_bits`, wired into `ParcelUpdate::merge_unedited` through a
  new `field by merge_fn` form of the macro). Merged whole, one unapplied tick
  on any other Access/Options box held `USE_ACCESS_LIST` at its seeded value,
  so the read-back changed nothing and the next **Apply** still re-opened the
  parcel.
- A multi-pick on Add now sends the list once rather than once per resident.
- Tests: `merge_unedited_bits` and `point_inside` unit tests; About Land's
  `a_flipped_list_flag_reaches_the_form_beside_a_pending_tick`; and the
  two-viewer e2e `an_allow_list_save_moves_the_anyone_can_visit_box`
  (OpenSim-flavoured fake grid: the owner adds a resident by pasted key, the
  "anyone can visit" box clears; Remove ticks it again). The e2e fails at the
  "box clears" step with the read-back disabled.
