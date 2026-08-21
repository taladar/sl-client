---
id: viewer-profile-floater-single-instance
title: A second resident profile replaces the first instead of opening its own
topic: viewer
status: bugs
origin: seen on aditi while live-checking [[viewer-conference-start-ui]]
  (2026-08-21)
refs: [viewer-social-profiles, viewer-ui-floater-basic,
  viewer-social-group-profile, viewer-about-land-options-tab,
  viewer-about-landmark-floater]
---

Context: [context/viewer.md](../context/viewer.md).

With one resident's profile open, opening a second resident's profile **reuses
the same window**: the subject is swapped and the first profile is gone. The
reference viewer opens one profile floater **per resident** — they stack, and
you can compare two people side by side.

## Why

`crate::avatar_profile` is built as a singleton: one `PROFILE_FLOATER_ID =
"avatar-profile"` window, one `ProfileState { target }`, one `ProfileUi`.
`open_profile` reads only the **last** `OpenAvatarProfile` of the frame and, for
a different agent, calls `state.reset(agent)`, re-requests the properties /
picks / classifieds / notes, invalidates the retained tabs, and shows the one
floater. There is nowhere for a second subject to live.

The floater scaffold is singleton-shaped too: `FloaterSpec::id` is a
`&'static str` and `floater_panel(&floaters, id)` looks a floater up by that id,
so "another instance of this kind, keyed by its subject" is not expressible
today. That is the real work here — a **keyed multi-instance floater** (the
reference's `LLFloaterReg::showInstance("profile", LLSD().with("id", id))`,
whose registry is keyed by name *and* key) — with per-instance state and
per-instance persistence, rather than anything profile-specific.

## Check the other instanced windows for the same shape

This is almost certainly not the only singleton that should be keyed. Each of
these opens per *subject* and shares one id today; when the keyed-instance
scaffold lands, audit them and convert the ones the reference keys:

- **About Land** (`about_land.rs`, `"about-land"`) — per parcel.
- **About Landmark** (`about_landmark.rs`, `"about-landmark"`) — per landmark
  item ([[viewer-about-landmark-floater]]).
- **Texture picker** (`ui_texture_picker.rs`, `"texture-picker"`) — per field
  being edited; two open pickers is a real workflow.
- **Item Properties** (`inventory_properties.rs`, `"item-properties"`) — per
  inventory item.
- **Notecard / script editors** (`edit_notecard.rs` `"notecard-editor"`,
  `edit_script.rs` `"script-editor"`) — per asset; the reference happily opens
  several scripts at once, and this one also risks *losing edits* when the
  window is re-pointed.
- **Group profile** (`group_profile.rs`, `"group-profile"`) — per group, the
  exact sibling of this bug.
- **About Region** (`about_region.rs`) and the **web browser**
  (`web_floater.rs`) — check what the reference does before converting; region
  info is arguably one window, the browser arguably tabs.

Not every floater should be keyed (Preferences, Search, the minimap, the
inventory and the Conversations floater are singletons in the reference too),
so this is an audit, not a sweep.

## How to verify

Open two residents' profiles from different places (a radar row, a name link in
chat) — both windows must exist, each showing its own subject, each closable on
its own, and their positions must persist independently.

Reference (Firestorm, read-only): `llfloaterreg` (name + key instance
registry), `llpanelprofile` / `llfloaterprofile` (opened per agent id).
