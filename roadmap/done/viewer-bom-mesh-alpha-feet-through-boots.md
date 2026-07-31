---
id: viewer-bom-mesh-alpha-feet-through-boots
title: BoM mesh body feet render through mesh boots (bake alpha ignored on BoM faces)
topic: viewer
status: done
origin: user report (2026-07-31, own avatar on aditi)
refs: [viewer-p17-3, viewer-r5, viewer-p14-3]
---

Context: [context/viewer.md](../context/viewer.md).

## Fixed 2026-07-31 (verified live on aditi)

`apply_bom_face_materials` now routes the BoM face alpha mode through a pure
`bom_face_alpha_mode(tint_alpha, bake_has_alpha)` helper (`avatars.rs`): an
opaque-tint face on a **carved** bake (`BakeAlpha::Masked` / `Transparent`,
the already-computed-but-previously-discarded `region_bake` flag) renders
`AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD)` so the hidden region vanishes;
bare skin (an `Opaque` bake) stays opaque, which is why this does **not**
reintroduce the R22d bare-skin see-through / arm UV-seam rings (those came from
masking an *un-carved* bake). Unit-tested (`bom_face_alpha_mode_masks_only_
carved_bakes`) and confirmed in-world: the mesh feet no longer show through the
boots, with no bare-skin/arm regression. The capture log showed the `lower`
region bake classified `Masked`, i.e. the carved case the fix targets.

## Symptom

Our own avatar's **mesh (BoM) body feet show through / clip out of worn mesh
boots**. Both feet and boots are mesh — this is **not** a system body part
poking out (so it is distinct from [[viewer-r3]], skin-weight normalisation).

The wearer hides the body under boots by wearing an **alpha layer** that carves
the feet/lower region transparent in the bake; the BoM mesh body should then not
render those faces. It renders them anyway.

## What the code does (confirmed, `avatars.rs`)

We **deliberately** render BoM mesh-body faces **opaque, ignoring the bake's
composited alpha** — and that is likely too broad, which is the bug:

- The **system-body region** bake path honours alpha: `apply_bake_image` sets
  `AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD)` when `classify_bake_alpha`
  reports `Masked`/`Transparent` (the reference avatar alpha-mask cutoff,
  `LLDrawPoolAvatar::sMinimumAlpha`), so an alpha wearable carved into the bake
  turns that region invisible.
- The **BoM mesh-face** path (`apply_bom_face_materials`, ~`avatars.rs:3818`)
  instead forces `alpha_mode` from the **face tint only** (`tint[3] < 255` →
  `Blend`, else `Opaque`) and **discards** the per-slot "bake has alpha" flag it
  just computed at `~:3781` (`region_bake.insert(…, alpha != BakeAlpha::Opaque)`
  → read back as `_bake_alpha`, unused). A fully-transparent *tint* still hides
  a face (`tint[3] == 0` → `Visibility::Hidden`), but an alpha layer carved into
  the *baked texture* does not.

The opaque choice is intentional and cites the reference: a BoM face has no
renderable alpha, so it batches into the opaque `sSimpleFaces` pass which does
not alpha-test — and applying bake alpha here previously "made bare skin
see-through (R22d) and cut UV-seam rings into the arm." So a naive "just honour
the bake alpha on BoM faces" flip is **known-regressive** and must not be the
fix.

## The real question / candidate fix

Reconcile the reference: bare skin (no alpha layer) classifies as
`BakeAlpha::Opaque`, so masking it is a no-op — the R22d breakage was applying
alpha where the bake had *none carved*. But **when an alpha layer *is* worn**,
the bake is `Masked`/`Transparent`, and the reference *does* hide those BoM body
regions (that is how BoM users hide their body under mesh clothing).

- Candidate: use the **already-computed** `_bake_alpha` — when the slot's bake
  is `Masked`/`Transparent`, render that BoM face with
  `AlphaMode::Mask(BAKE_ALPHA_MASK_THRESHOLD)` (same cutoff as the system-body
  path) instead of `Opaque`; keep `Opaque` for `BakeAlpha::Opaque` bakes so bare
  skin never goes see-through. Watch the R22d UV-seam-ring regression (mask
  threshold catching antialiased seam alpha) — verify the arm as well as the
  feet.
- Confirm against Firestorm whether BoM mesh-body faces alpha-mask on a
  worn-alpha-layer bake, and at what cutoff (`sMinimumAlpha` = 0.2).

## Verify

Live on aditi, own avatar wearing a BoM mesh body + mesh boots + a feet alpha
layer: the feet should vanish under the boots without bare skin (no alpha layer)
becoming see-through and without seam rings on the arm. A client-side render
test is feasible: drape a baked region texture with a transparent sub-region on
a BoM face and assert the carved fragments do not render while an all-opaque
bake stays fully visible.
