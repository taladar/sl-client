---
id: viewer-about-land-objects-return
title: About Land — object return, editable autoreturn & small option gaps
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-parcel-options-general, viewer-parcel-options-access-media,
  viewer-about-land-options-tab]
---

Context: [context/viewer.md](../context/viewer.md).

The reference About Land Objects tab acts on parcel objects: Return per
class (owner/group/other, with live counts), Return per selected owner
from the object-owners list, Show (select/beacon) per class, and an
editable autoreturn ("clean other time") minutes field. Our Objects tab
(`sl-viewer-places/src/about_land.rs`) renders the counts and the
owners table but the only action is `RefreshOwners`, and autoreturn is a
read-only value node. The write paths exist in `sl-proto/src/command.rs`:
`ReturnParcelObjects`, `SelectParcelObjects` / `DisableParcelObjects`, and
`SetParcelOtherCleanTime`. `ReturnParcelObjects` and `DisableParcelObjects`
are already sent by the Top Objects floater
(`sl-viewer-places/src/top_objects.rs`), the pattern to follow; About Land
sends none of them.

Also fold in two small reference gaps whose write paths already exist:
the Access tab's "Sell passes to" Anyone/Group combo (a flag combination
of USE_ACCESS_GROUP + USE_PASS_LIST, see `llfloaterland.cpp`
onCommitGroupCheck) and the Firestorm Options-tab "Teleport" button that
teleports the agent to the parcel's landing point (trivial; teleport
command exists).

Reference (Firestorm, read-only): `indra/newview/llfloaterland.cpp`
(LLPanelLandObjects, LLPanelLandAccess),
`indra/newview/skins/default/xui/en/floater_about_land.xml`.
