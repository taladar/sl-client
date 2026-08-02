---
id: viewer-parcel-music-stream-internal-data-error
title: Parcel music stream fails to play ("Internal data stream error")
topic: viewer
status: done
origin: user report (2026-08-02, aditi live testing)
refs: [viewer-streaming-audio, viewer-music-controls-push-chat-bar]
---

Context: [context/viewer.md](../context/viewer.md).

## Symptom

On a parcel that advertises a music-stream URL, the parcel-audio cluster
appears and the stream is attempted (autoplay when `MusicStreamEnabled`, or the
play button), but nothing plays. The stream player logged only:

```text
WARN sl_gst::stream: audio stream error: Internal data stream error.
```

Observed live on an aditi test region (2026-08-02) while verifying
[[viewer-music-controls-push-chat-bar]].

## Resolution (2026-08-02)

**Root cause of this report was a dead upstream stream, not the client
pipeline.** The aditi test parcel's `music_url` is
`http://pub3.di.fm/di_psychill`, whose host **`pub3.di.fm` no longer resolves**
(DI.FM long ago restructured its CDN). Our pipeline plays a normal Icecast MP3
stream (e.g. SomaFM) end to end, so nothing was wrong with playback — exactly
the "dead upstream stream mistaken for a client bug" the Verify note below
warned about.

The real defect was that the failure was **undiagnosable**: the error text said
nothing about *why*. Two layers hid the reason:

1. We discarded GStreamer's **debug string** — the generic
   `Internal data stream error.` on the bus carries its actual reason (element +
   flow return) only in the debug string, which `friendly_error` threw away.
2. `souphttpsrc` **swallows the network reason**: for a DNS / TCP-refused /
   TLS / HTTP failure it logs the real cause only at its own debug level and
   hands the pipeline a bare flow error (`reason error (-5)`), so the true cause
   never reaches the application bus at all (confirmed by reducing to a plain
   `souphttpsrc ! fakesink` — same opaque error for both a bad host and a
   refused port).

Fixes (all shipped; benefit **both** GStreamer consumers — the parcel radio
player and media-on-a-prim **video**, which share `sl-gst`):

- **`sl-gst` (`messages.rs`):** fold the debug string's human tail into the
  error; translate GStreamer's `streaming stopped, reason error (-5)` jargon
  into plain language; keep any meaningful demux reason (e.g. HLS "Could not
  update any variant playlist"); and flag a generic HTTP-**source** failure via
  a new `network_diagnosable` bit on `AudioStreamStatus` /
  `sl_media::SurfaceStatus`.
- **Viewer (`media_diagnostics.rs`, new):** when a stream reports such a generic
  failure, a shared URL-keyed `MediaDiagnostics` cache probes the URL itself on
  a background task and classifies the failure precisely — DNS, TCP connect
  (refused / timeout), TLS / certificate, or HTTP status + first body line. Both
  the parcel-audio bar and the media-on-a-prim controls show the recovered
  reason in place of the generic one.
- **First-error-wins guard:** a failing pipeline emits a *cascade* (an
  HTTP-source failure, then a typefind "not enough data"); the earlier code let
  the later, vaguer error overwrite the good message and clear the
  `network_diagnosable` flag. Both bus handlers now ignore further errors once a
  stream is already in error, so the first (root-cause) error stands.

Live-confirmed: on **aditi** the parcel-audio bar reads *"cannot reach
pub3.di.fm — DNS lookup failed (failed to lookup address information: Name or
service not known)"*; on **local OpenSim** a media-on-a-prim cube pointed at a
dead http host shows the same precise reason on its media-controls bar.

## Verify

Re-test on a parcel with a *known-good* stream (ideally one the reference
viewer plays) so a dead upstream stream is not mistaken for a client bug, then
confirm audio actually plays and the now-playing title / state update. (A dead
stream now names its own precise reason instead of an opaque internal error.)
