---
id: viewer-audit-probe-nonuniform-scale-shear
title: A reflection probe's non-uniform volume scale shears its sampling frame
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/probes.rs:861` — the probe holder spawns with
`Transform::from_scale(probe.volume_scale())` and
`rotation: sample_rotation(world_rotation)`, where `sample_rotation` is
`world_rotation.inverse()`.

The doc immediately below quotes the composition it must undo —
`*original_transform * Affine3A::from_quat(self.rotation)` — and
`original_transform` is the **full affine including scale**. So the linear part
is `R * S * R^-1`: a shear for any non-uniform `S`. Only the rotation is
cancelled; the scale is folded into the reflection sampling frame as well as the
influence volume. Reflected directions bend anisotropically.

The doc's own phrasing ("leaving the `Transform` — and therefore the influence
volume — still tracking the prim") shows the scale was reasoned about as *only*
the volume. Worst on mirrors, where `hero_volume_scale` floors a thin panel to
`(2, 3, 1)` (`:1779`).

The existing test (`:2029`) composes `Quat`s only, so it structurally cannot see
this — widening it to `Affine3A` is the one assertion that catches it.

## Fixed (2026-08-28)

The two jobs Bevy 0.19 did with one matrix are now separate, in our Bevy fork
(`taladar/bevy`, branch `sl-client-externally-posed-skin`, commit
`edb520a4f`, pinned as the workspace's new bevy rev):

- `LightProbeComponent::get_world_from_light_matrix` is left alone — the probe's
  transform, so the influence volume is exactly the entity's box, which for our
  holder is the prim's oriented box (`R * S`).
- a new `LightProbeComponent::get_sample_rotation` returns the rotation taking a
  world-space direction into the frame the texture was authored in. For an
  `EnvironmentMapLight` that is its world rotation undone with the `rotation`
  field composed on top: `(world_rotation * self.rotation).inverse()`. It ships
  to the GPU as a per-probe quaternion and the shader rotates the world-space
  direction by it instead of pushing that direction through the volume matrix.

They could not both be satisfied by one affine: the volume wants `R * S` and the
sampling frame wants a rotation, and `R * S * R⁻¹` is a shear for every
non-uniform `S`. The old composition therefore did neither — the volume was
*not* the prim's box (the audit's "still tracking the prim" was the doc's claim,
not the behaviour), and reflected directions were stretched along the prim's
axes.

Parallax correction still intersects in light-probe space, where a
non-uniformly scaled probe has squashed the hit point, so the forward matrix
ships too and the hit is taken back to world space before the rotation — which
is what a box-projected cubemap wants anyway (and what the reference viewer's
`boxIntersect` does). We enable no `ParallaxCorrection`, so this path is
untested beyond compiling; for a uniformly scaled probe it is unchanged.

The viewer side needed **no** change to what it passes: `sample_rotation` is
still `world_rotation.inverse()`, which under the new rule composes to identity
— the cube read in the space `copy_probe_faces` captured it in — while the
holder's `Transform::from_scale(volume_scale)` is now the volume and nothing
else.

Adjacent fix, same lines: re-aiming a turning prim's sampling frame wrote only
`GeneratedEnvironmentMapLight.rotation`. Bevy derives the `EnvironmentMapLight`
the light-probe pass actually samples through **once** (its query is
`Without<EnvironmentMapLight>`) and never refreshes it, so a spinning mirror
kept reflecting at whatever angle it was bound at. `reaim_sample_frame` writes
both components, the discipline `calibrate_probe_intensity` already carries for
the intensity.

### Tests

`a_local_probe_samples_its_cube_in_world_space` is widened as the audit asked,
and both of its claims are put to **Bevy's own** composition functions rather
than restated locally, so a Bevy bump that changed either rule (or dropped the
fork's split) fails the test rather than the frame: the sampling frame resolves
to identity for four prim rotations, and the affine the volume is built from
comes back as the prim's `2 × 3 × 1` box, unsheared by the `rotation` field.

### Verified

- The two unit assertions above, against Bevy's own functions.
- `render_readback`'s `the_mirror_reflects_each_neighbour_on_its_own_side` — a
  real GPU render of a real local probe through the whole new path — still puts
  each coloured neighbour on its own side of the mirror, as do the other three
  readback checks.
- A local OpenSim session renders clean, which is what settles the GPU struct:
  the `LightProbes` uniform lives in the **view** bind group, so wgpu validates
  its size against every draw's declared struct — a Rust/WGSL layout
  disagreement is a hard error before anything appears, not a subtle one.

What is *not* pixel-verified is the anisotropic case itself: the readback
scene's probe is a uniform 6 m sphere volume, where the old and new
compositions agree exactly, and a box probe with a non-uniform scale needs
Second Life content (`metallic-sphere-among-prims` is the only mirror fixture,
and OpenSim serves no reflection-probe blocks). A pixel test whose failure
under the old composition has not been demonstrated would be the module's own
documented trap — the first version of that check "was measuring the cubes, not
the mirror" — so the claim is left to the algebra, which is exact.
