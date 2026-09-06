---
id: viewer-keyed-floater-audit
title: Audit the other per-subject singletons onto the keyed-floater scaffold
topic: viewer
status: ready
origin: split out of [[viewer-profile-floater-single-instance]] when the
  keyed-instance scaffold landed (2026-09-06)
refs: [viewer-profile-floater-single-instance, viewer-ui-floater-basic,
  viewer-social-group-profile, viewer-about-land-options-tab,
  viewer-about-landmark-floater, viewer-notecard-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The keyed multi-instance floater scaffold is in
([[viewer-profile-floater-single-instance]]): `FloaterKey`, `KeyedFloaters`,
per-instance state on the window, a cascade, and close-ends-the-window. The
avatar profile is converted and is the worked example.

The profile was not the only singleton that should open per subject. Each
window below opens *on* something and shares one id today; convert the ones the
reference keys, following the profile's shape — state as components on the
floater root, an open path through `KeyedFloaters::open`, content built at
spawn instead of `DeferredFloaterContent`.

- **Group profile** (`group_profile.rs`, `"group-profile"`) — per group, the
  exact sibling of the profile bug, and the same module shape.
- **Item Properties** (`inventory_properties.rs`, `"item-properties"`) — per
  inventory item.
- **About Landmark** (`about_landmark.rs`, `"about-landmark"`) — per landmark
  item ([[viewer-about-landmark-floater]]).
- **Notecard / script editors** (`edit_notecard.rs` `"notecard-editor"`,
  `edit_script.rs` `"script-editor"`) — per asset; the reference happily opens
  several scripts at once, and this one also risks *losing edits* when the
  window is re-pointed, so it is the highest-value conversion after the
  profile.
- **Texture picker** (`ui_texture_picker.rs`, `"texture-picker"`) — per field
  being edited; two open pickers is a real workflow. This is the first likely
  customer for `FloaterKey::Named`, whose instances *do* keep their own
  geometry.
- **About Land** (`about_land.rs`, `"about-land"`) — per parcel.
- **About Region** (`about_region.rs`) and the **web browser**
  (`web_floater.rs`) — check what the reference does before converting; region
  info is arguably one window, the browser arguably tabs.

Not every floater should be keyed (Preferences, Search, the minimap, the
inventory and the Conversations floater are singletons in the reference too),
so this is an audit, not a sweep. A window that stays a singleton is a finding
worth writing down here, not a no-op.

## How to verify each conversion

Open the window on two different subjects: both must exist, each showing its
own subject, each closable on its own without disturbing the other. For the
editors, additionally: unsaved text in the first window must survive opening
the second (that is the bug the conversion prevents, and it is invisible in any
check that only counts windows).

Reference (Firestorm, read-only): `llfloaterreg` (name + key instance
registry); `LLFloater::getControlName` for which keys get their own saved rect.
