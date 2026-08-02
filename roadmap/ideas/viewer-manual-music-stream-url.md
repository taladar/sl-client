---
id: viewer-manual-music-stream-url
title: Manually specify a music-stream URL (cruises / travel events)
topic: viewer
status: ideas
origin: user request (2026-08-02)
refs: [viewer-streaming-audio]
---

Context: [context/viewer.md](../context/viewer.md).

Let the user set their **own** music-stream URL that plays independently of the
parcel's `music_url`, for group travel events — SL "cruises", conga lines,
sailing/flying tours, hunts — where a host or DJ streams the soundtrack while
the group crosses many parcels whose own streams (if any) are irrelevant.

Today [[viewer-streaming-audio]] follows only the parcel's advertised
`music_url`; crossing a parcel boundary switches or stops the stream. A manual
override would keep the chosen stream playing across parcels until the user
clears it.

## Sketch

- A small "set stream URL" affordance on the parcel-audio bar (the bar is now
  always shown, greyed when the parcel has no stream — a natural home): a text
  field / paste target, or an entry on a right-click / overflow menu.
- A manual URL takes precedence over the parcel `music_url`: while one is set,
  parcel changes do **not** switch or stop it; clearing it returns to the
  parcel-follow behaviour.
- Reuse the existing player, per-URL user-stop memory, and `MusicStreamVolume`;
  only the URL *source* changes (manual vs. parcel). The failure diagnosis
  (`media_diagnostics`) applies unchanged, so a bad pasted URL still names its
  real reason.
- Persist the last manual URL per account (optional), and offer a short history
  / favourites list so a regular cruise stream is one click away.

## Open questions

- Should a manual URL survive teleports and relogs, or is it session-only?
- Interaction with `MusicStreamEnabled` autoplay: does setting a manual URL
  imply "play now" regardless of the autoplay master switch?
- A tiny favourites list vs. just the last-used URL.
