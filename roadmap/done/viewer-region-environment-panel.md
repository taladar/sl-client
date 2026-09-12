---
id: viewer-region-environment-panel
title: Region / parcel environment settings panel
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-region-options-general, viewer-environment-my-environments]
refs: [viewer-parcel-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

Publishing environments to land: the Region/Estate floater's
**Environment** tab and the parcel-level equivalent in About Land — choose
a day cycle (from the picker of [[viewer-environment-my-environments]]) or
the legacy defaults, set day length / offset, manage the altitude sky
tracks, apply / reset — written through the `ExtEnvironment` capability
(get/put per region or parcel; the parcel variant carries the parcel id).
The ingest side of `ExtEnvironment` exists (P22 reads region environments),
and [[protocol-sim-caps-region-info]] (2026-08-20) added the **write**
pairing in `sl-proto`: `Command::SetEnvironment` →
`build_environment_update_request` → `ExtEnvironment` PUT through both
runtimes (repl `set_environment` publishes day length/offset today). What
remains here is the two panels — day-cycle publishing (inline `day_cycle`
or `day_asset`), altitude-track management, per-track (`trackno`) scoping,
and the reset (DELETE) verb, none of which the repl command exercises.

Reference (Firestorm, read-only): `llpanelenvironment` /
`panel_region_environment.xml`, `llenvironment` (`ExtEnvironment` PUT).

Deps: [[viewer-region-options-general]] (the region floater the tab lives
in), [[viewer-environment-my-environments]] (the settings picker).

## Done

**One panel, hosted twice.** `sl-viewer-environment::land_environment` is the
reference's `LLPanelEnvironmentInfo`: not a window but a panel the Region /
Estate floater's Environment tab and About Land's both spawn
(`spawn_land_environment_panel`), differing in a `LandPanelKind` and four
controls. The host keeps it aimed by writing one `LandEnvironmentSubject`
component — the parcel, whether the window's region is still the agent's,
whether the agent may publish, the estate's override flag, the parcel's area —
and everything below that is the panel's, driven by systems over *every* panel
entity. Two About Land windows on two parcels therefore each edit their own,
and a reply for one cannot be taken by the other: the arriving
`Event::Environment` is matched on its own `parcel_id`.

**The two replies the panel needed did not exist on this side.** The
`ExtEnvironment` reply carries `day_asset` and `day_names`, and neither was
decoded — so "what is this track holding?" had no answer and the settings
picker could not open on what the field already held. `EnvironmentSettings`
grew both, `day_names` as a `DayNames` enum because the wire sends it two
ways: a **string** naming the whole cycle, or an **array** naming each of the
five tracks (the reference's `mDayCycleName` / `mNameList`, which is the same
split). `environment_to_llsd` emits them back, so the round trip is closed and
the fake grid's own encoder cannot drop them silently.

**The reset verb is new all the way down.** `Command::ResetEnvironment` is the
`ExtEnvironment` DELETE (`coroResetEnvironment`) through both runtimes and the
repl; `SimCaps` answers it by forgetting the entry
(`SimSession::reset_environment`, a new `ServerEvent::EnvironmentReset`) and
echoing what the land inherits — a parcel its region's environment, a region
the grid default, which is why the region entry is *replaced* rather than
removed. The URL the three verbs share is one function now,
`sl_proto::environment_cap_url`, which builds the query only for a parcel or a
track exactly as the reference does; the old code always sent
`?parcelid=-1` for the region.

**Publishing is batched behind Apply, and that is a deliberate divergence.**
The reference PUTs on every change — a slider's mouse-up, a pick. This
toolkit's slider has no end-of-drag signal, so per-change publishing would be
a PUT per pixel of a drag, and a draft is the only thing a Revert can revert
to. What reaches the wire is the same set of requests, batched by
`publish_requests`: one PUT with no `trackno` for the day length, the day
offset, the track altitudes (region scope only, as the reference scopes them)
and a whole-cycle `day_asset`, then one PUT per track whose asset was picked,
each with that track's `trackno`. The item's own permissions become the
environment's `FLAG_NOMOD` / `FLAG_NOTRANS`, as `onPickerCommitted` computes
them.

**"Parcel Owners May Override" is not an environment setting.** It is the
estate's `ALLOW_ENVIRONMENT_OVERRIDE` flag (`1 << 9`, confirmed against
`llregionflags.h` and OpenSim's `HandleEstateChangeInfo` `0x200` arm), which
travels with the rest of the estate info. So the panel owns the checkbox and
the reference's `EstateParcelEnvironmentOverride` confirmation, and writes
`AllowEnvironmentOverrideRequested`; About Region — which holds the estate
name and the other flags — turns it into `Command::SetEstateInfo`, clearing
the fixed-sun bit the way its Apply Estate button already does.

**About Land's Environment tab stops being a read-only summary.** The two
facts the *parcel record* carries (overrides allowed, the environment version)
stay as a header, and the panel replaces the day-cycle sentence and the "this
is a separate feature" note. It also grew the freeze the Region / Estate
floater already had: `AboutLandState` records the **circuit** the subject was
bound on, because the same region-local parcel id on a new circuit is a
different parcel, and publishing to it would land on land nobody was looking
at.

**Which reason a disabled panel gives is ordered, and tested.** Cross-region
outranks everything: every other reason is about the parcel in front of you,
and reporting "this parcel is too small" for a parcel in a region you have
left sends somebody off to enlarge the wrong thing. A *region* panel ignores
the three parcel gates entirely — an estate that has switched parcel overrides
off can still change its own environment, which is exactly where the reference
puts the test (the parcel subclass's `canEdit`).

## Not done — and why

- **No "Customize Day Cycle" button.** The reference's Edit opens the
  day-cycle editor on the land's *own* cycle — which is not an inventory item
  — and takes its commit back. `day_cycle_editor` is item-scoped throughout
  (`EditedItem`, Save writes onto the item it came from), so a land editing
  context is that window's own piece of work, filed as
  [[viewer-environment-land-day-cycle-edit]]. Until it lands a cycle is
  authored in inventory and published here, which is the reference's own
  `CONTEXT_INVENTORY` route.
- **The altitudes are three number fields, not a vertical multi-slider.** The
  toolkit has no multi-slider and the numbers are what the wire carries. They
  are sorted on Apply, as the simulator sorts them anyway
  (`ViewerEnvironment.SortAltitudes`).
- **A track's picker is filtered to that track's kind.** The reference's drop
  targets take anything; a water track being handed a sky is not a choice
  worth offering.
- **`trackno` does not work on OpenSim.** `EnvironmentModule` answers
  `trackno != -1` with "Environment Track not supported", so the per-track
  publish is a Second Life path. The failure surfaces as the grid's own
  message rather than being hidden.

## Verified

`cargo test --release -p sl-viewer-environment` — the panel's nine new tests
green (the unavailable-reason ordering for both scopes, the day offset's
round trip through the reference's ±12 h display convention, the apparent
time of day, an untouched draft publishing nothing, a parcel update omitting
the track altitudes, a per-track pick riding its own scoped request, an
item's permissions becoming the environment's flags, and a track's name
falling back by scope), plus the crate's scheduling sweep, which now builds
the panel's plugin.

`cargo test --release -p sl-proto` — the `day_asset` / `day_names` round trip
through `environment_to_llsd`, and the `ExtEnvironment` DELETE.

Not verified live: the panel's layout, and a publish against a grid. Worth
driving on OpenSim (region scope, day length and offset, then the reset) and
on aditi (the per-track `trackno` path, which OpenSim refuses).
