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
- ~~**Item Properties** (`inventory_properties.rs`, `"item-properties"`)~~ —
  **done** (2026-09-07). Keyed by item id; the shown item and the name /
  description / price field entities are components on the window, and the
  toggle observer and the Enter-commit resolve theirs (the commit by *which
  window's field has the keyboard*, which is the honest reading of "commit
  what is being typed"). Unlike the asset editors, an open on an item already
  up **rebuilds** the window rather than only raising it: the re-open is how a
  permission toggle repaints every checkbox from the updated snapshot, so the
  content build was split out (`build_properties_content`) and both paths call
  it. Live-checked on the local grid: two items, two windows, a checkbox
  flipping and repainting only its own, Enter committing the window being typed
  in, and a close leaving the other. The check also turned up three findings in
  the window's own content: [[viewer-item-price-field-silently-dropped]] and
  [[viewer-unticking-for-sale-erased-the-price]] (both fixed — the second was
  data loss, and took `ItemInfo::sale` from an `Option` pair to a `SaleInfo`
  carrying both wire fields), and
  [[viewer-disabled-field-selection-flash]] (filed). A fourth followed from
  reading the reference while fixing those: every control in the window was
  gated on one "is it mine" flag, where `LLFloaterProperties::refresh` gates
  each on the bit it is about — so the window offered an anyone-copy on an item
  the owner cannot copy, and next-owner rights the creator never permitted
  ([[viewer-item-permission-gates]], fixed).
- **The two per-type previews** (`inventory_properties.rs`, `"preview-texture"`
  / `"preview-animation"`) — still singletons, and the same argument applies
  (comparing two textures is a real workflow). They were missing from this list
  rather than deliberately excluded; they share the properties module and would
  convert the same way.
- ~~**About Landmark** (`about_landmark.rs`, `"about-landmark"`)~~ — **done**
  (2026-09-07). Keyed by the landmark's inventory id
  ([[viewer-about-landmark-floater]]); the shown item, the resolve chain's
  progress (pending asset, parcel id, details, deadline), the built row handles
  and the SLURL are components on the window, and the copy / teleport / map
  observers resolve theirs with `host_floater`. The one thing that did not fall
  out of the pattern is the resolve itself: `RemoteParcelId` comes back as a
  bare id naming no request, so two windows resolving at once could not tell
  whose answer had arrived. `ParcelResolveQueue` serialises them — one request
  in flight, the reply belongs to the head, a timed-out window leaves the queue
  — and the protocol-level fix is filed as
  [[viewer-remote-parcel-id-uncorrelated]]. Writing
  `parcel_resolves_are_serialised` turned up a second half of the same problem:
  the reply pass offered each event to every window in turn, so the window
  *behind* the head — which becomes the head the moment the first is answered —
  took the same answer as its own. A `RemoteParcelId` is now consumed once, by
  the head; `ParcelDetails`, which does name its parcel, still reaches every
  window waiting on it. The chain also folds before it drives (replies and
  expiries, then the next question), so a handoff costs no frame.
- **The remaining inventory-item editors** — every one of these opens *on* an
  item and shares one id today, so each is a keyed conversion with the same
  shape as the notecard editor:
  - **Wearable editor** (`edit_wearable.rs`, `"wearable-editor"`) — bodyparts
    and clothing layers. Editing a shape while comparing it against a skin is
    an ordinary workflow, and the reference keys this one by item.
  - **Material editor** (`edit_material_asset.rs`, `"material-editor"`) — per
    material item.
  - **Object contents** (`edit_contents.rs`, `"object-contents"`) — per object
    (task id), not per item: the window lists one object's inventory.
  - **Colour picker** (`ui_color_picker.rs`, `"color-picker"`) — the same
    argument as the texture picker, and the same **named** key: the field being
    picked for.
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
- **About Land** (`about_land.rs`, `"about-land"`) — per parcel, and **About
  Region** (`about_region.rs`) — per region. The reference keeps both as
  singletons (`LLFloaterLand` / `LLFloaterRegionInfo` open on *the parcel you
  are standing on* and *the region you are in*, so there is only ever one
  subject). We key them anyway, by user decision: this viewer can show a parcel
  or region it is not standing in — from a landmark, a search hit, a place
  profile — and comparing two parcels' covenants or two regions' settings is a
  real thing to want. That is a deliberate divergence from the reference, and
  this is where it is written down.
- **Web browser** (`web_floater.rs`) — check what the reference does before
  converting; the browser is arguably tabs rather than windows.

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
