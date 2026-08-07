---
id: viewer-avatar-dead-reckoning-translation-rubberband
title: Ease the avatar's rendered translation toward truth (dead-reckoning rubberband)
topic: viewer
status: done
origin: surfaced fixing viewer-third-person-cam-lag-vertical-flight (2026-07-30)
refs: [viewer-third-person-cam-lag-vertical-flight, viewer-p31-7, viewer-p31-4]
---

Context: [context/viewer.md](../context/viewer.md).

During **fast** flight the own avatar's rendered world position **rubberbands**:
between server updates the dead-reckoner (`physics.rs` `drive_avatar_motion`,
P31.4) extrapolates smoothly, but on each `ImprovedTerseObjectUpdate`
`apply_object` (`avatars.rs`) **hard-snaps** the anchor `Transform` to the
authoritative position. When prediction and truth diverge (fast motion, sparse
updates) that snap is a visible per-frame jump — measured `mean |d_root| ≈
0.21 m/frame`, up to `3.0 m` on a correction frame.

This is invisible with a lagging camera (the old world-space smoothing filtered
it), but the now-rigid third-person follow
([[viewer-third-person-cam-lag-vertical-flight]]) faithfully reproduces it as a
**view shake against the world** — the avatar stays framed (good) but the
background jumps on each server tick.

**Asymmetry to fix:** rotation is *eased* toward truth every frame
([[viewer-p31-7]]: `apply_smoothed_rotation` slerps
`rendered_rotation` toward the authoritative facing), but translation is not —
`apply_object` snaps it. The fix is the translation counterpart of P31.7: ease
the rendered position toward the authoritative / dead-reckoned position instead
of snapping.

Care needed:

- **Root-drop render offset (R23).** `drive_avatar_motion` deliberately moves
  the anchor by prediction *deltas* to preserve the baked-in vertical root-drop;
  a translation ease must ease toward truth **as a delta** (or re-apply the
  offset) so it does not clobber R23.
- **Region crossings.** The 256 m rebase / `recenter_terrain` and the
  region-cross prediction path must snap (not ease across the rebase), like the
  camera `resnap` does.
- **Own-avatar feel.** Easing adds a little lag to the own avatar's rendered
  position (the reference viewer has this via interpolation). Keep the constant
  short enough that input still feels responsive.
- Note `viewer-avatar-motion-render-smoothing` (done) fixed a *different* jitter
  (an avian pre-physics pass clobbering the head **joint** for `Update` readers)
  and explicitly ruled out the dead-reckoning snap for that case — this is the
  snap it left untouched.

## Done (2026-08-07)

`physics.rs` `drive_avatar_motion` now eases the rendered avatar translation
toward the authoritative / dead-reckoned position **every frame** (the
translation counterpart of P31.7's rotation easing), instead of hard-snapping on
each `ObjectUpdate`. A tracked `AvatarInterp::target_translation` (captured from
the anchor on each server update, advanced by the prediction delta between
updates — a Bevy-space delta, so the R23 root-drop offset is preserved) is what
`rendered_translation` eases toward via `eased_translation` (τ ≈ 100 ms). A
**region crossing** or any region-scale jump (`> 32 m`, a teleport / 256 m
rebase) **snaps** rather than gliding.

The continuous (every-frame) ease — not just on update frames — also fixed a
short teleport freezing **part-way** to the destination once the avatar stood
still and updates stopped (confirmed live on OpenSim: short teleports now
converge fully). Pure decision helper `eased_translation` is unit-tested (ease a
small correction; snap on crossing; snap a region-scale jump). The fast-flight
background-shake is the same per-update snap→ease mechanism; not separately
re-measured this session, but the ease directly addresses it.
