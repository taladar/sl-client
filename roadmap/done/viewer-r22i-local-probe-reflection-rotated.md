---
id: viewer-r22i
title: Local reflection probes reflect the world rotated 90° about X
topic: viewer
status: done
origin: found in the viewer-render-test-harness gallery (2026-07), by looking at a mirror sphere and noticing the reflections did not line up with what they reflected
refs: [viewer-render-test-harness, viewer-render-readback-tier]
---

Context: [context/viewer.md](../context/viewer.md).

**Fixed.** Every **local** reflection probe (`crate::probes`, P33.2) sampled its
captured cubemap through the probe prim's world rotation, so the reflection it
cast was the world turned by the Second Life → Bevy basis change (−90° about X)
*and* by the prim's own rotation. A neighbour below the prim appeared to one
side; one behind it appeared below. The **default** probe was unaffected.

## Root cause

Bevy builds a probe's sampling frame from the probe entity's
**world transform**, not from its `rotation` field alone
(`bevy_pbr/src/light_probe/environment_map.rs`):

<!-- `text`, not `rust`: this is a quotation of upstream Bevy source, not our
code, and tagging it `rust` puts it under ggh's rustfmt check — which then wants
it laid out exactly as upstream has it, at a width MD013 rejects. -->

```text
fn get_world_from_light_matrix(&self, original_transform: &Affine3A)
    -> Affine3A
{
    // Take the `rotation` field into account.
    *original_transform * Affine3A::from_quat(self.rotation)
}
```

and `environment_map.wgsl` transforms the reflection direction *into* that frame
(`light_from_world`) before sampling the cube.

But `copy_probe_faces` captures the cube in **Bevy world space**. So any
rotation the probe holder inherits rotates the reflection — and it always
inherits one: `spawn_probe_holder` parents the holder to the prim's object
entity (deliberately, so the influence volume tracks the prim), and every root
object entity carries the basis change in its world rotation
(`sl_to_bevy_object_rotation` = `sl_to_bevy_rotation()` × the prim's own
rotation). The holder was given `rotation: Quat::IDENTITY`, so nothing cancelled
it.

The comment that misled: the *default* probe's `rotation: Quat::IDENTITY`
carries the note "the cube is captured directly in Bevy world space, so it
samples with no extra reorientation" — which is true
**for a view environment map**, where Bevy uses only the `rotation` field
(`view_rotation`) and never the camera's transform. The local path does not work
that way, and the identical-looking line was wrong there.

## The fix

`sample_rotation(world_rotation) = world_rotation.inverse()` on the holder's
`GeneratedEnvironmentMapLight`, so `world_from_light` composes back to identity
and the cube is read in the space it was captured in. The holder's `Transform` —
and therefore the **influence volume** — still tracks the prim, which is the
whole reason it is parented to it.

Re-derived when the prim turns (a spinning mirror), compared with `abs_diff_eq`
so a prim at rest costs no per-frame churn — the same discipline as the rest of
`drive_local_probes`.

## Why it took this long to see

Nothing was *broken*: no invariant, no log line, no crash. The probe captured,
the volume bound, the mirror was shiny, and the reflection was plausible from
any angle you had not thought about. It needed a mirror with
**distinctly identifiable things around it** and a person asking "is the yellow
one where the yellow one should be" — which is exactly what
[[viewer-render-test-harness]]'s `metallic-sphere-among-prims` scene is, and it
was found within minutes of that scene first rendering.

`probes::tests::a_local_probe_samples_its_cube_in_world_space` pins it by
asserting the composition Bevy performs resolves to identity for any prim
rotation. [[viewer-render-readback-tier]] is what could have caught it without a
human: "the red neighbour's reflection lands on the red side" is a pixel
assertion.
