---
id: viewer-p31-12b
title: Eye-blink morph (LLEyeMotion blink)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — split out of P31.12 (head & eye look-at)
refs: [viewer-p31-12]
blocked_by: [viewer-p31-12a]
---

Context: [context/viewer.md](../context/viewer.md).

**P31.12b. Eye-blink morph.** The blink half of `LLEyeMotion`, split out of
[[viewer-p31-12]] because it needs the per-frame visual-param morph pipeline
([[viewer-p31-12a]]) the head & eye **rotation** work did not. The reference
drives the eyelids by morphing the `Blink_Left` / `Blink_Right` visual-params
between 0 and 1 on a random blink timer (`EYE_BLINK_MIN_TIME` ..
`EYE_BLINK_MAX_TIME` between blinks, closing over `EYE_BLINK_SPEED` with
`EYE_BLINK_TIME_DELTA` between the two eyes, held shut for
`EYE_BLINK_CLOSE_TIME`), and also blinks opportunistically while the eyes move
(`LLEyeMotion::onUpdate`). Port that timer and drive the two blink params
through the P31.12a per-frame morph capability. The P31.12 eye-saccade state
machine (`crate::look_at::Saccade`) already advances every frame and is the
natural home for the blink timer once the morph pipeline exists. Reference: the
blink block of `LLEyeMotion::onUpdate` in `llheadrotmotion.cpp`.
