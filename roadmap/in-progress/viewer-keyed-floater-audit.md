---
id: viewer-keyed-floater-audit
title: Audit the other per-subject singletons onto the keyed-floater scaffold
topic: viewer
status: in-progress
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

- ~~**Group profile** (`group_profile.rs`, `"group-profile"`)~~ — **done**
  (2026-09-06). Its state, dirty flags, UI handles and the three list
  projections (members / notices / roles) are components on the window; the
  view syncs, the sub-panel rebuilds, the row populate/bind passes and all four
  observers iterate or resolve their window; the row observers and the action
  observer find theirs with `host_floater`, so a Save saves *that* window's
  group from *that* window's fields. `RequestedGroupNotices` stays a resource
  (it dedupes fetches by notice id, not per window), and the tables keep
  persisting their sort / widths by table name as the reference does.
  Live-checked on the local grid: two groups, two windows with their own lists
  and selections, closing one leaves the other, re-opening raises.
- **Item Properties** (`inventory_properties.rs`, `"item-properties"`) — per
  inventory item.
- **About Landmark** (`about_landmark.rs`, `"about-landmark"`) — per landmark
  item ([[viewer-about-landmark-floater]]).
- ~~**Notecard / script editors** (`edit_notecard.rs` `"notecard-editor"`,
  `edit_script.rs` `"script-editor"`)~~ — **done** (2026-09-06). Each window
  carries its own `NotecardEditorState` / `ScriptEditorState` (source, baseline,
  in-flight load and save, field entities, run state), and a task-held asset is
  keyed by **object and item**, since two rezzed copies of one object carry the
  same item ids. The edit-losing bug is fixed twice over: a second asset opens
  its own window, and re-opening an asset already up only **raises** it — never
  re-fetches, which is what used to replace typed text with the grid's copy
  (`reopening_a_notecard_does_not_refetch_it`, `reopening_a_script_does_not_\
  refetch_it`). The Save / Running observers resolve their window with
  `host_floater`, and the notecard **drop** now names its window:
  `AddEmbeddedItem` carries the editor entity that `inventory_drag`'s
  `notecard_target_at` resolved, instead of the drop landing in whichever
  notecard a resource held. Live-checked on the local grid: two notecards and
  two scripts, each window with its own text, Save reporting into its own
  status line, and a re-open focusing without touching what was typed. The
  check also turned up three bugs of its own, all filed and two already fixed —
  [[viewer-new-notecard-unreadable-on-opensim]],
  [[viewer-saved-asset-reopens-stale]] and
  [[viewer-notecard-preview-ignores-unsaved-text]].
- ~~**Texture picker** (`ui_texture_picker.rs`, `"texture-picker"`)~~ —
  **done** (2026-09-07), and the scaffold's **named** half's first customer:
  the window is keyed by the *field* being picked for (a swatch's element id,
  carried as the new `TextureSwatchField` component and named in
  `OpenTexturePicker::field`), so each field's window remembers its own
  position and size under `texture-picker_<field>_rect`. Two swatches sharing
  an element id share a window, deliberately — they are the same field. Closing
  ends the window, so `revert_on_close` now reads the manager's close
  **command** before the pass that carries it out, rather than noticing a
  hidden panel afterwards; OK / Cancel clear the requester first, which is what
  keeps that revert from undoing the choice they just made.
- **About Land** (`about_land.rs`, `"about-land"`) — per parcel.
- **About Region** (`about_region.rs`) and the **web browser**
  (`web_floater.rs`) — check what the reference does before converting; region
  info is arguably one window, the browser arguably tabs.

Not every floater should be keyed (Preferences, Search, the minimap, the
inventory and the Conversations floater are singletons in the reference too),
so this is an audit, not a sweep. A window that stays a singleton is a finding
worth writing down here, not a no-op.

## What the first conversion taught the scaffold

Opening a keyed window from a click **inside another floater** — a group row in
a resident's profile — put the new window *behind* the one clicked, every time.
One press does both things: the row's handler asks for the window, and the
press bubbles to the host floater's root observer as a `BringToFront`. The open
systems ran before the manager's command pass, so the new window took its z
first and the click's raise then landed on top of it.

`FloaterSystems::Commands` is that lesson: **a keyed window's open system must
run after the command pass**, so its raise is the frame's last word. Both
converted modules order themselves that way, the next conversion must too, and
`a_window_opened_by_a_click_lands_above_the_window_clicked` drives exactly that
frame (it failed before the ordering, and the test fixture is deliberately in
the same set the plugin uses, since an `.after` on an empty set is silently a
no-op).

## How to verify each conversion

Open the window on two different subjects: both must exist, each showing its
own subject, each closable on its own without disturbing the other. For the
editors, additionally: unsaved text in the first window must survive opening
the second (that is the bug the conversion prevents, and it is invisible in any
check that only counts windows).

Reference (Firestorm, read-only): `llfloaterreg` (name + key instance
registry); `LLFloater::getControlName` for which keys get their own saved rect.
