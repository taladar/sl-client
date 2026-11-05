---
id: viewer-media-served-mime-probe
title: Probe the served MIME type before dispatching media to an engine
topic: viewer
status: ideas
origin: split out of viewer-video-playback (2026-11-05)
refs: [viewer-video-playback, viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer asks the server what a media URL serves (an HTTP probe,
`LLViewerMediaImpl::mimeDiscoveryCoro`) before choosing between the browser and
the video plugin. We dispatch by the URL's scheme and file extension instead
(`sl_media::classify_url`), so a video or audio stream served from an
extension-less URL — a Shoutcast mount, a CDN token URL — lands in the browser.

Parcel media already uses the parcel's declared type first
(`sl_media::classify_media`) and falls back to the URL only when the type is
empty or unknown; media on a prim has no declared type and always uses the URL.

Scope, if extension dispatch proves insufficient in practice: a `HEAD` (or
ranged `GET`) probe for `http` / `https` URLs whose extension says nothing,
dispatching on the served `Content-Type` through `classify_mime`, with the
URL-based guess as the fallback when the probe fails. The probe must go through
the same media URL allowlist and proxy settings as the surfaces.
