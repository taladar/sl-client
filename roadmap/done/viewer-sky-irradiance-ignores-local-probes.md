---
id: viewer-sky-irradiance-ignores-local-probes
title: Sky-lit faces take their probe ambient from the view's probe, never from a placed probe
topic: viewer
status: done
origin: viewer-realtime-mirrors (2026-11-10)
refs: [viewer-realtime-mirrors, viewer-legacy-material-exact-port]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`sky_probe_irradiance` (`sl-client-bevy/src/sky_lighting.wgsl`), the diffuse
ambient every sky-lit prim face is lit by, samples only the view's own
(default) probe (`light_probes.view_cubemap_index`). A face standing inside a
placed reflection probe's volume therefore takes the outdoor sky's ambient
rather than what that probe captured — a room lit by its own probe stays lit
like the open sky.

The reference's `sampleReflectionProbes` / `sampleReflectionProbesLegacy`
take the ambient (`ambenv`) from the probes that hold the fragment, the way
they take the reflection.

The specular half had the same gap and was fixed for
[[viewer-realtime-mirrors]]: `sky_probe_radiance` now goes through Bevy's
per-fragment `compute_radiances`. The irradiance can follow the same way
(`compute_radiances` returns both), but it changes the look of every face
inside a probe and wants a before/after look, so it is its own task.

The legacy (Blinn-Phong) branch's environment reflection is
[[viewer-legacy-material-exact-port]]'s.

## Fix (2026-11-15)

- `sky_lighting.wgsl`: the irradiance and the radiance share one
  `sky_probe_radiances` helper over Bevy's clustered `compute_radiances`, so
  `sky_probe_irradiance` blends the probes that hold the fragment, falling
  back to the view's probe. `sky_irradiance` now takes the fragment
  (`SkyFragment`: world position, frag coord, orthographic flag), which
  `face_material.wgsl` and `terrain.wgsl` build.
- The Bevy fork gains upstream's `bevy_pbr` fix #25268 (`c7a16115c`,
  cherry-picked as `0066ea07`). Before it, `compute_radiances` assigned each
  probe's irradiance (`=`) instead of accumulating it (`+=`), so where two
  volumes overlap only the last probe's ambient would have counted.
- Test:
  `render_readback::a_face_inside_a_placed_probe_takes_its_ambient_from_it`
  puts a solid-red placed probe around the `prim-box` scene under a sky that is
  probe ambient only. The box reads red inside the probe and neutral without
  it.

## Before/after on the fake grid

`sl-crosscheck --scenario catalogue-eep --look-at reflection-probe`, run with
the binaries from before and after the fix, gives the same pixels on the probe
box (82, 73, 62 over the whole front face). Only sky noise of 9 or less
differs. That is expected: that probe is a 1 m box whose volume holds nothing
but its own faces. Those faces lie on the volume's edge, where the
`LightProbe` falloff takes its weight to zero, so they take the view's probe
either way.

So the catalogue gains a `probe-room` landmark: a plain box inside a red room
that is open to the south, filled by a transparent 3 m box probe. Mean sRGB of
the box's top face, which the sun does not reach, at `--look-at probe-room`:

| run | box top |
|---|---|
| before the fix | 30, 33, 42 (the open sky's blue) |
| after the fix | 32, 6, 9 (the room's red) |
| Firestorm | 41, 29, 20 |

Firestorm also takes the room's colour, but less saturated; that gap is
[[viewer-local-probe-ambient-saturation]].

Getting there turned up two more bugs, both fixed here:

- Firestorm left the fake grid as soon as the objects arrived:
  `zeroCodeExpand` refused a 2829-byte datagram ("attempt to write past
  reasonable encoded buffer size"). `SimSession` sent every object it was
  given in one `ObjectUpdate`, and the five new prims took the catalogue past
  the reference's 8 KiB `NET_BUFFER_SIZE`. `send_object_update`, its
  compressed form and `send_terse_update` now split their blocks across
  datagrams of at most 1200 bytes (`MTUBYTES`), as a simulator fills each one
  only until the next block would not fit. A loopback test sends 64 prims in
  each form.
- Two capture runs lost their pinned camera to the SpaceNavigator, which a
  windowed viewer reads straight off the machine ("flycam → third person
  (6-DOF device button)"). A capture run now leaves the device out, as it
  already left the cursor grab out.
