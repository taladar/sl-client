---
id: viewer-region-telehub
title: Telehub management floater
topic: viewer
status: done
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-general, viewer-region-options-estate, api-g8]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Region/Estate floater's General tab has a "Manage Telehub…"
button (`panel_region_general.xml`, `manage_telehub_btn`) that opens the
telehub floater (`llfloatertelehub.cpp`, `floater_telehub.xml`): connect
the region's telehub to the currently selected object, disconnect it, list
the telehub's spawn points, and add/remove spawn points (added at the
selected object's position), with in-world highlighting of the hub object
and each spawn position while the floater is open.

Our About Region floater (`sl-client-bevy-viewer/src/about_region.rs`) has
no telehub button and the viewer has no telehub UI at all, while the wire
side is complete and unused: sl-proto carries `RequestTelehubInfo`,
`ConnectTelehub`, `DisconnectTelehub` and `AddTelehubSpawnPoint`
(`sl-proto/src/command.rs`, all EstateOwnerMessage requests) plus the
`TelehubInfo` reply decode, all delivered by [[api-g8]]. Implementing this
means adding the Manage Telehub button to the Region tab (estate-owner
gated), a small floater listing the hub object and spawn points over
those commands, connect/disconnect wired to the current edit-tool
selection, and ideally beacon-style highlighting of hub/spawn positions.

Reference (Firestorm, read-only): `indra/newview/llfloatertelehub.cpp`,
`indra/newview/skins/default/xui/en/floater_telehub.xml`,
`indra/newview/skins/default/xui/en/panel_region_general.xml`.

## Done (2026-09-12)

The Telehub window exists, reached from the Region tab's new **Manage Telehub…**
button, and the four `EstateOwnerMessage`/`telehub` commands [[api-g8]] landed
unused now have their caller.

- **The window** (`sl-viewer-places/src/telehub.rs`, a singleton floater
  `telehub`): the reference's two status lines, Connect / Disconnect, the
  spawn-point list, Add / Remove Spawn, and its explanatory footer. It asks
  (`RequestTelehubInfo`) each time it opens and otherwise shows only what the
  region last said — every telehub method is answered by a fresh `TelehubInfo`,
  including the `info ui` read, so nothing here predicts what a command did.
  Closing it forgets the configuration, so a window reopened after a teleport
  asks the region it is now in.
- **The selection is the argument.** Connect and Add Spawn act on the build
  tools' selection, and send **one message per selected root prim** — the
  reference's `SEND_ONLY_ROOTS` — so three prims selected add three spawn
  points. Only `pcode::PRIMITIVE` roots count (`selectionAllPCode(
  LL_PCODE_VOLUME)`), and the estate gate is re-checked in the press observer
  rather than trusted from the button that opened the window.
- **In-world markers.** A new shared surface: `DebugBeacons` in
  `sl-viewer-world-api` (a group of markers per asking feature) drawn by
  `sl-viewer-world-scene/src/debug_beacons.rs` — the reference's
  `addDebugBeacon` / `renderObjectBeacons`, both passes (a see-through cross
  with the depth test off at quarter alpha, and a depth-tested small cross plus
  point cube). The telehub is marked yellow and the spawn point selected in the
  list orange, exactly as `LLFloaterTelehub::addBeacons` does, and both go when
  the window does.
- **A beacon follows its object.** A marker names an optional anchor
  `ObjectKey`; the renderer resolves every distinct anchor in one pass over the
  object table each frame (`scoped_by_full_keys`) and falls back to the position
  the reply carried only while the object is not in the scene. The spawn marker
  rides the hub's frame — `hub_pos + spawn * hub_rot` — which is how the
  simulator stores it.

### Divergences from the reference, deliberate

- **Line width.** The reference draws its beacons as 3–4 pixel `GL_LINES`;
  `wgpu` has no line width at all, so each arm is a thin **box** — world-space
  thickness instead of screen-space, which reads the same close up.
- **The Region floater stays open.** The reference hides `region_info` as it
  shows `telehubs`; ours is one window per region, and hiding an instance the
  person opened is a surprise rather than a tidy-up.
- **The tool is left alone.** The reference forces the translate tool on open so
  that something *can* be selected. The viewer's selection is not a floater's to
  commandeer.
- **Buttons are disabled, not hidden.** Everything in this window is a write, so
  the About Region convention (hide the write buttons) would leave an empty
  frame; as in the reference they grey out instead.

### The bug the live check found

The first live run put the orange spawn marker nowhere near the prim it was
recorded at. A root object's world rotation
(`sl_to_bevy_object_rotation`) **composes the Second Life → Bevy basis change
with the object's own orientation**, so the space it rotates *from* is Second
Life's — and the offset was being converted to Bevy axes first and then rotated
by it, applying the basis change twice. A spawn point 4 m north of the hub came
out 4 m below it. The offset now goes in raw, and the unit test that pins it
uses an **unrotated** anchor, which is the case the old test (a pure-east
offset, where both conventions agree) could not tell apart.

### One thing the tests caught

Adding the window to `AboutRegionPlugin` (the button writes `OpenTelehub`, and
an unregistered message panics on the first write) made the four About Region
instance tests panic on frame one: the plugin now reads the **selection** and
the **object table**, and writes the **beacons**, none of which that headless
app had. `TelehubPlugin` declares all three with `init_resource` — their real
owners (`sl-viewer-edit`, the world layer, the scene renderer) declare them the
same way, so whoever is built first mints the shared one — rather than the test
harness growing three more lines nobody would repeat next time.

### Verified

Unit: the four button preconditions, the spawn-point formatting, the marker set
(none / hub / hub + selected spawn), and the beacon placement in both frames.
The floater sweep and the registry check pass, so the window is held to the
whole layout matrix like every other one.

Live, on the local OpenSim as the estate owner: the Region tab's button opens
the window; Connect makes the selected prim the telehub and the status line
names it; Add Spawn records a spawn point and selects it; and — after the basis
fix above — the yellow and orange markers sit where the hub and the chosen spawn
point actually are.

### Not done here

The reference's beacon carries an optional **label** (`LLHUDText`, the area
search and pathfinding callers use it); the telehub callers pass `""`, so
`DebugBeacon` has no label field and the renderer draws none. The first caller
that wants one adds it.
