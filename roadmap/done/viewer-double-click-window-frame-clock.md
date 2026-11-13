---
id: viewer-double-click-window-frame-clock
title: Lists time double clicks with a constant on the frame clock, so a slow viewer loses them
topic: viewer
status: done
origin: viewer-terraform-parcel-refusal commit hook (2026-11-13)
refs: [viewer-terraform-parcel-refusal]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`e2e_login::an_opensim_flavoured_login_loads_the_library` failed in the full
hook suite and passed alone; at `CPUQuota=15%` it failed three runs of three.
The Library root stayed `loaded: false`: the double click that expands it was
never seen.

## Why

Eleven surfaces told a double click from two single clicks with a constant
of their own (0.35 s in the inventory tree and gallery, 0.4 s in the radar,
People, Groups, the profile's group list, Top Objects, the object Contents
list, the minimap, the world map and the world teleport fallback), measured
on the frame clock. The synthetic input plays a double click two frames apart
and pins picking's `multi_click_interval` to make that one double click
however long the frames take — but only the world teleport read that
interval. At a few frames a second each frame may advance the clock by
0.25 s, so two frames is 0.5 s and the lists saw two single clicks. A real
resident on a slow viewer lost double clicks the same way.

## Resolution

`sl_viewer_ui_core::double_click::DoubleClickWindow`, a system param over the
app's `PickingSettings::multi_click_interval` (picking's 0.5 s default where
there are none) and its clock, is the one window every surface asks; the
eleven constants are gone. A unit test pins the window and the pin; the
Library test passes at `CPUQuota=15%`.
