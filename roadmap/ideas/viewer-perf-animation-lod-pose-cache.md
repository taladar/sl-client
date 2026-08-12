---
id: viewer-perf-animation-lod-pose-cache
title: Animation pose cache + temporal/budget pose LOD (no-GPU crowd stopgaps)
topic: viewer
status: ideas
origin: design discussion off the 2026-08-12 critical-path capture
refs: [viewer-perf-avatar-pose-extract-skins, viewer-perf-gpu-avatar-crowd, viewer-avatar-impostors-billboard, viewer-perf-skeleton-single-solve]
---

Context: [context/viewer.md](../context/viewer.md).

The near-term, **no-GPU-rewrite** levers for the animated crowd, chosen
because the dance-club case defeats the usual ones: everyone dancing (so
per-joint `set_if_neq` saves little) and everyone near (so distance LOD /
impostors [[viewer-avatar-impostors-billboard]] don't apply). These buy time
before the [[viewer-perf-gpu-avatar-crowd]] rewrite, at a small quality cost.

## 1. Pose-sample cache keyed on (animation asset, quantized phase)

Several avatars commonly play the **same** animation asset (synced dancers, or
the same dance HUD). For a given (asset, playhead) the **sampled local pose**
(keyframe decode + interpolation + priority/ease blend) is identical for
everyone on it. Compute it **once** per distinct (asset, phase-bucket) and
reuse; unsynced dancers quantize the playhead into ~30 buckets/s and hit a
small LRU. Sampling drops from `O(avatars)` to `O(distinct asset × bucket)`.

**Scope, precisely:** this shares the **sampled local rotations only**. It does
**not** share FK, transform propagation, or `extract_skins` — each avatar's
**rest skeleton differs** (shape sliders, appearance joint offsets, volume-bone
scales), so the shared rotations compose onto a different base and the final
skin matrices are per-avatar. Same dance ≠ same skin matrices unless same
shape. So this trims the ~1 ms `pose_avatar_skeletons` sampling stage, **not**
the ~5–7 ms `extract_skins` (which needs per-joint-skip or the GPU rewrite).
Modest standalone win; it becomes powerful only in the GPU design, where the
shared clip is read coherently and FK+skin happen GPU-side.

## 2. Temporal / budget pose-update LOD

Distance LOD fails in a club (all near), so LOD by **update rate under a
budget** instead: near/important avatars pose every frame; the rest refresh at
a reduced rate (~20–30 Hz, round-robin under a per-frame skeleton budget) with
their skin palette held between updates. Cuts the `O(avatars × joints × fps)`
product regardless of distance. Cost: a small smoothness hit on the dance —
a genuine quality tradeoff (the GPU design has none). Pick the refresh set by
screen-space size × recency, not raw distance, so on-screen dancers stay
smooth and the ones behind others degrade first.

## 3. Bone-count LOD

Skin lower-importance avatars with a **reduced skeleton** (drop face/finger
bones; collapse Bento extras) — fewer joints to FK, propagate, and extract.
Composes with #2 (fewer bones × lower rate). Needs a per-avatar skeleton-LOD
selection and a weight remap to the reduced joint set.

## Sequencing / verification

Land #1 and #2 first (independent, cheap, reversible), measure on a populated
region (or a scripted multi-avatar OpenSim scene) via `pose_avatar_skeletons`
mean + `ExtractSchedule` median with the window visible. #3 is a bigger change
(skeleton LOD + weight remap). All three are stopgaps for
[[viewer-perf-gpu-avatar-crowd]], not substitutes — keep them only as long as
they earn their complexity once the GPU path exists. Related CPU-side precursor:
[[viewer-perf-skeleton-single-solve]].
