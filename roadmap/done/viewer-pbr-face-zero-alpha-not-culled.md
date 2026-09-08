---
id: viewer-pbr-face-zero-alpha-not-culled
title: A PBR face at zero base-colour alpha is still built (the glTF half of the transparency cull)
topic: viewer
status: done
origin: split out while porting the alpha-pool cull (2026-09-08)
refs: [viewer-animesh-transparent-box-shell]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-animesh-transparent-box-shell]] ported the reference's
fully-transparent cull for a **legacy** face: `objects::is_fully_transparent`
reads the texture entry's tint alpha at face build and hides the face, so it
leaves the colour *and* shadow passes exactly as `LLVOVolume::rebuildGeom`'s
`if (alpha > 0.f || te->getGlow() > 0.f)` gate leaves it out of every batch.

The glTF half — a face whose render material is `BLEND` at base-colour alpha
`0` — was not ported, so such a face was still built and still cast the solid
shadow the legacy fix removed.

## What the reference actually says

The gate is stated twice, once per pool a PBR face can reach, and reading both
settled what the verdict is:

- **The alpha pool**, which is where a blending PBR face really goes:
  `LLPipeline::getPoolTypeFromTE` returns `POOL_ALPHA` for *any*
  `ALPHA_MODE_BLEND` material, and a non-blend PBR material is forced to
  `POOL_GLTF_PBR` a few lines earlier — so "PBR face in the alpha pool" means
  "blending". There the gate reads `alpha = is_pbr ? gltf_mat->mBaseColor[3] :
  te->getColor()[3]`, and the `te->getGlow() > 0.f` term rescues it.
- **The opaque pools**, the `should_render` block this item quoted, which the
  reference's own comment calls unreachable in theory.

Both cull the same face, so one predicate expresses both:
`materials::pbr_face_is_fully_transparent` — `BLEND`, base-colour alpha exactly
zero, and the texture entry not glowing. The alpha-mode test is what keeps the
gate off an `OPAQUE` or `MASK` material carrying a zero alpha factor it never
renders with; the glow term stays a *legacy* fact even for a PBR face, because
that is where the reference reads it from.

The two halves are **alternatives, not a conjunction**, and that was the second
half of the fix: the build-time legacy verdict had been applied to every face,
PBR ones included, so a face whose tint was transparent but whose glTF base
colour is opaque was wrongly hidden. A PBR face's verdict now comes from its
composed material and overwrites whatever the build wrote.

## How it is wired

As sketched: `FaceSlot` carries the face `entity` (and the texture entry's
`glow`, the one term the material does not supply), every path that can change
the verdict pushes `(Entity, bool)` onto a queue on the manager, and
`apply_pbr_face_visibility` drains it into `Visibility`. The paths are
`recompose_face` (registration, a material decoding, an override arriving, the
picker's live preview), `revert_face_to_diffuse` and the FIRE-35138
Blinn-Phong hide — the last two handing the face **back** to the legacy tint
verdict, since a face showing its Blinn-Phong layer is judged as a legacy face.

The drain runs in `PostUpdate` before Bevy propagates visibility, rather than
beside the material systems in `Update`. That is unambiguously after both the
systems that queue a verdict and the object build, which re-describes a rebuilt
face with the legacy verdict (`spawn_face_entity` cannot know a material is
coming) — so the PBR answer lands over it in the same frame, and no ordering
edge has to be spelled out against a dozen systems.

## The pick had to learn the difference

The legacy fix let `ObjectPicker::pick` reach a hidden face by re-deriving the
tint verdict, which says nothing about a PBR face — so the glTF gate would have
made an invisible PBR prim unclickable, exactly the regression the legacy half
had to avoid. Both gates now write a `TransparencyCulled` marker component
alongside the `Visibility`, and the pick admits a hidden entity only when it
carries that. It is the honest predicate anyway: "hidden" alone cannot tell an
invisible prim from a derendered avatar. The `P` probe reports it too.

## Verification

Unit tests: the verdict table (each clause of the reference's condition), and an
ECS test that composes two faces through the real queue + drain — an
opaque-tinted face whose material blends at zero alpha ends up hidden and
marked, a zero-tinted face carrying an opaque material ends up shown and
unmarked. The legacy build test now also pins the marker.

Not reproduced live: the reference's own comment says a face reaches the quoted
block only in theory, and authoring a `BLEND`-at-alpha-0 material to see it is a
test of the fixture rather than of the viewer. What a live run is worth here is
the **negative** — that the gate does not fire where it should not — and that is
not something eyeballing a region can establish: "nothing looks missing" from an
operator who has not inventoried the region's PBR content is barely evidence at
all.

So the cull says what it hid. `materials::TRANSPARENCY_CULL_LOG_TARGET` logs one
line per face the glTF gate takes out of every pass, naming the object and face
index (`RUST_LOG=info,sl_viewer::transparency_cull=debug`), and only on the
transition into the cull. A silent "stop drawing this" is otherwise
indistinguishable from a fetch, a decode or a build that never produced the face
— which is the shape of every bug this gate could introduce.

An aditi session with it on, closed once the region had fully rezzed, logged
**zero** culls: the gate hid nothing there. Its other direction cannot hide
anything either — a PBR verdict overriding the legacy one only ever shows a face
the tint would have culled.
