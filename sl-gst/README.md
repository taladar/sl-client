# sl-gst

The GStreamer playback media engine for a Second Life / OpenSim viewer —
the second implementation of the `sl-media` `MediaBackend` boundary,
alongside the `sl-cef` browser engine. It plays what the browser cannot:
**direct video / audio URLs** (`.mp4`, `.webm`, `.mkv`, HLS / DASH
manifests, RTSP) on media-on-a-prim faces, and **parcel radio streams**
(Shoutcast / Icecast, with the ICY "now playing" title) through the
separate `AudioStreamPlayer`.

The split mirrors the reference viewer, whose `mime_types.xml` dispatches
`video/*` and `audio/*` to a dedicated media plugin (libvlc, or GStreamer
on Linux) while HTML goes to CEF — and it exists for codec reasons:
prebuilt CEF is open-codec-only, while GStreamer picks up the **system's**
decoders (VA-API / libav on Linux, Media Foundation on Windows,
VideoToolbox on macOS).

**We ship no encumbered decoder.** H.264 / AAC come from the user's
platform plugins or not at all; when a decoder is missing the failure is
loud — GStreamer's `missing-plugin` messages become the surface's error
text ("needs an H.264 decoder — install the matching GStreamer plugin"),
and `playback_gaps()` enumerates absent capabilities at startup.

Frames cross the boundary as CPU BGRA (a `videoconvert ! appsink` video
sink, stride removed); audio currently goes straight to the system device
(`autoaudiosink`) — the same interim as the CEF engine — until the shared
viewer mixer (`viewer-audio-backend`) lands, at which point both engines'
PCM feeds the mixer and media-on-a-prim audio becomes positional.

GStreamer core is LGPL and linked dynamically; the `gstreamer-rs`
bindings are MIT/Apache-2.0.
