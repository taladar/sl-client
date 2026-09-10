---
id: viewer-environment-day-cycle-editor
title: Day-cycle editor
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-environment-fixed-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The EEP day-cycle editor: arrange sky (and water) settings as **keyframes
on tracks over a day timeline** — the ground sky track, the water track,
and the altitude sky tracks — with a scrubber previewing any time of day
live, per-keyframe editing (opening the fixed editors from
[[viewer-environment-fixed-editor]] in-place), track copy, day length
metadata, and save/load as a day-cycle settings asset. The P22.6 day-cycle
*interpolation* already renders such assets; this authors them.

Reference (Firestorm, read-only): `llfloatereditextdaycycle`,
`floater_edit_ext_day_cycle.xml`, `llsettingsdaycycle`.

Deps: [[viewer-environment-fixed-editor]] (the per-frame editors and the
settings-asset save path).

## Done

A third window in `sl-viewer-environment`, `day_cycle_editor`: the five track
buttons down the left, the timeline across the top with its scrubber over the
keyframe strip, the transport and the track / frame verbs under it, and the same
knob tabs the fixed editors show over whichever keyframe is selected. It opens
on the same `OpenSettingsEditor` the other two read — the inventory's Open, and
the My Environments row's **Edit** — and saves through the same two paths (an
in-place `UpdateSettingsAgentInventory`, or a Save As onto the viewer's shared
settings-creation queue).

**A day cycle is a model, and the model went in the pure crate.** `DayCycle`
grew `LLSettingsDay`'s own operations — `insert_sky_keyframe`,
`insert_water_keyframe`, `move_keyframe`, `remove_keyframe`, `clear_track`,
`clone_track`, `keyframe_near`, `split_shared_frame`, `blended_sky` /
`blended_water` per track — so the editing rules are unit-testable without a
window, and `EnvironmentSettings`' altitude-selected blend is now the same
lookup with a track index in front of it.

**The reference's five tracks are not five indices.** It numbers them `0..=4`
with water first, while our `sky_tracks` is a list of the sky tracks alone — a
bare index means two different tracks depending on which side of that boundary
wrote it. `DayTrack` is that boundary named once, with the reference numbering
as a conversion at the edges and a round-trip test over it.

**A keyframe's frame is split before it is edited.** Frames are referenced *by
name*, and one asset may legally name the same frame from two keyframes — so a
knob written straight into "the selected keyframe's frame" would silently change
a keyframe nobody selected. Every edit goes through `split_shared_frame` first,
which mints a copy only when the frame is actually shared. The reference has the
same rule for a different reason: it clones on every insert
(`buildDerivedClone`), so two of its keyframes are never one object to begin
with. Removing a keyframe drops its frame definition when nothing else names it,
or a saved asset would grow a frame for every edit ever made and never shrink.

**The scrubber is a frozen frame, not an animated day.** The reference does not
install the cycle in `ENV_EDIT` and let the clock run it; it blends the cycle at
the scrubber into a scratch sky and a scratch water and installs *those*
(`updateEditEnvironment` and its two `LLTrackBlenderLoopingManual`s). So does
this window, which is what makes the scrubber a preview of a *time of day*
rather than a request to wait for one — and Play then simply walks it, a whole
day in the reference's sixty seconds. While the water track is selected the sky
still has to come from somewhere, and it comes from the ground track, as the
reference's `skytrack = mCurrentTrack ? mCurrentTrack : 1` does.

**Between two keyframes there is nothing to write into.** The knob pages then
show the *blend* — so you can see what the cycle does at 4 a.m. — and a slider
moved there is refused rather than landing in a frame nobody chose. That is the
reference's lock icon, said in words instead: a hint line above the pages. The
same rule covers a read-only item and a running Play, which is what its
`updateButtons` bundles into `can_manipulate`.

**Which knob is on which tab is one table for the crate now** (`tabs.rs`), moved
out of the fixed editors: the reference shares those panels verbatim between the
two floaters, and two copies of the list would be two ways for a knob to end up
on no tab or two. The "every knob is on exactly one tab" test moved with it and
now covers both windows.

The timeline is this crate's own widget rather than a toolkit one — two strips,
a pooled set of markers placed by their left edge, and the pointer mapped into
the same span they are placed in (a half-marker error there is not a wrong
number, it is a keyframe that jumps sideways the instant it is grabbed). Markers
are shown and placed, never respawned, and every write in the window's reconcile
is gated on the value actually differing: a `LogicalInset` or a `Translated`
written every frame is a relayout or a re-resolve every frame over a window
where nothing moved.

**Day length is the region's.** The reference takes it from the context the
editor was opened in and shows the percentage alone for an inventory item; this
window only ever edits an inventory item, so it takes the length of the day the
agent is standing in — an authored cycle is going to be worn by *some* day, and
the region's is the only one on hand. The readout and the five tick labels say
`NN% (H:MM)` where there is one, and `NN%` where there is not.

**The per-keyframe editing is the reference's own arrangement.** The task text
asks for the fixed editors to be opened "in-place"; what the reference actually
does is share the *panels* between the two floaters
(`panel_settings_sky_atmos.xml` and friends are the same files in
`llfloaterfixedenvironment` and `llfloatereditextdaycycle`), and that is what
this does — the same knob tables, drawn inside this window over the selected
keyframe. Opening the fixed-editor *floater* would have been the wrong shape:
that window is bound to an inventory item and previews through the edit layer,
and a keyframe is neither.

Both greyed **New Day Cycle** entries — the inventory's create menu and the My
Environments creator row — are live, and the library's Edit no longer refuses a
day cycle.

## Not done — and why

- **No Apply To Parcel / Apply To Region.** The reference's Save flyout can
  commit the cycle straight to the land it was opened from. Publishing an
  environment to land is [[viewer-region-environment-panel]]'s job, which owns
  the permission tests (`canAgentUpdateRegionEnvironment` /
  `canAgentUpdateParcelEnvironment`) and the altitude-track scoping that go with
  them; offering the verb here without them would be a button that fails on most
  land.
- **No Import.** The reference reads a legacy WindLight day preset off disk.
  That is [[viewer-environment-import-legacy-presets]]'s job.
- **Load Track takes the matching track.** The reference's track picker lets you
  say *which* track of the chosen cycle to take; ours takes the one with the
  same index, which is what its picker does when it is left alone. A cycle's
  ground track belongs over a ground track.
- **No shift-drag frame copy, and no double-click to add.** Both are second ways
  to reach a button that is already there (`Add Frame`), and the first needs a
  modifier read inside a drag gesture the widget layer does not carry yet.

## Verified

`cargo test --release -p sl-proto --lib -- types::environment::` — 39 green,
including eight new ones over the track operations (the reference numbering
round-trips both ways; writing to an absent altitude track materialises it; a
keyframe cannot be added onto another and the slop lookup wraps round midnight;
a dragged keyframe re-sorts and refuses a collision; a removal drops an orphan
frame but never empties the ground or water track; clearing keeps what a cycle
cannot do without; a cloned track is copies rather than references and water and
sky do not mix; a shared frame is split before it is edited) and the one that
pins the editor's per-track blend and the renderer's altitude-selected one as
the same lookup.

`cargo test --release -p sl-viewer-environment --lib` — the crate's tests green,
including the new day-cycle ones (the pointer maps into the span a marker is
placed in; the ticks span the whole day; skipping wraps at midnight; selecting
snaps onto a keyframe and lets go between them; the pages show the selected
frame or the blend; the water track still previews a sky; the verbs are offered
exactly where they would work; a full track stops offering more; Play covers the
day in the reference's minute; the readout reads a clock only where there is a
day length; a save writes the whole cycle under the name in the field) and the
scheduling sweep, which now stands this window's systems up too.

Not verified live: no day cycle has been opened, scrubbed, keyframed and saved
against a grid. The save path is the part worth driving — the flags stamp and
`UpdateSettingsAgentInventory` are both grid behaviour — and so is the preview,
which is the only part whose value is what the sky actually looks like.
