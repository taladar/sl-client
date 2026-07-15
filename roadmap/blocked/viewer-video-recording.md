---
id: viewer-video-recording
title: In-viewer video recording (machinima capture)
topic: viewer
status: blocked
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold, viewer-video-playback]
---

Context: [context/viewer.md](../context/viewer.md).

Record the viewer's output to a video file, in-viewer — for machinima and
clips.

**No SL viewer has this.** The reference viewer has no built-in recorder
(verified: Firestorm has no frame-dump / capture path), so SL machinima is done
with external screen capture (OBS). We are unusually well placed to offer it
built-in, for one reason: **GStreamer will already be in the tree** for
media-on-a-prim video playback ([[viewer-video-playback]]), and GStreamer
*encodes* as well as decodes — so the encoder is a dependency we already carry,
with the same system/OS codecs and the same VA-API hardware path (in reverse).

Shape:

- **Capture** the rendered frame each tick. The clean path is to render the
  camera to an offscreen wgpu texture and read it back (or, better, keep it on
  the GPU) rather than grabbing the window — that decouples recording resolution
  from window size and lets the UI be excluded.
- **Encode** by feeding those frames into a GStreamer pipeline via `appsrc` →
  encoder → muxer → file (H.264 / VP9 / AV1, hardware-encoded where available),
  with the **audio** side pulled from [[viewer-audio-backend]]'s mixer so the
  clip carries in-world sound. The A/V sync problem is the mirror of the
  playback one — the audio clock is master; keep GStreamer's timestamps.
- **Machinima ergonomics** are the point, and they are what an external recorder
  cannot do as well: hide the UI, record at a **fixed frame rate decoupled from
  render rate** (render as fast or slow as the scene needs, emit a steady 30/60
  fps — the offline-render trick that makes heavy scenes smooth), a chosen
  resolution independent of the window, and a clean start/stop with a recording
  indicator. Camera-path playback and depth-of-field belong to the camera system
  ([[viewer-camera-third-person-orbit]]), not here.

The honest scope question for the fleshing-out agent: how much of this to build
vs. leaving serious machinima to OBS. A minimum viable recorder (fixed-fps
capture + GStreamer encode + audio) is genuinely useful and cheap given the
encoder is already present; the elaborate end (timeline, keyframed camera, post
effects) is a project of its own and probably not worth it.

Reference: no reference-viewer precedent — this is new. GStreamer `appsrc` +
`vah264enc` / `x264enc` / `vp9enc` / `av1enc` + `qtmux`/`matroskamux`.

Deps: [[viewer-ui-widget-scaffold]] (the controls), [[viewer-video-playback]]
(the shared GStreamer stack — this is its encode side).
