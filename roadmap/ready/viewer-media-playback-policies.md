---
id: viewer-media-playback-policies
title: Media playback policies — URL filter, perms, first-click, rolloff
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-media-prim-browser, viewer-streaming-audio,
       viewer-manual-music-stream-url, viewer-video-playback,
       viewer-anti-spam-filter]
---

Context: [context/viewer.md](../context/viewer.md).

The security/UX policy layer around parcel media, parcel music streams
and media-on-a-prim that the in-progress [[viewer-media-prim-browser]]
and [[viewer-streaming-audio]] tasks do not carry. The centrepiece is
the media filter (Singularity-lineage, `MediaEnableFilter`): before
playing a parcel's media URL or audio stream, check the URL's domain
against persisted per-account allow/deny lists; unknown domains prompt
(allow once / whitelist / deny once / blacklist,
`MediaFilterSinglePrompt`), and a Media Lists management floater
(`floater_media_lists.xml`, `fsfloatermedialists.cpp`) edits both
tables (add/remove; lists persist in `medialist.xml`). This is an
IP-privacy/security feature — media and stream servers see the
viewer's IP, and hostile parcels use media URLs as IP grabbers — so it
matters on Second Life proper. Our players (`media_audio.rs`,
`media_prim.rs`, `parcel_audio.rs`) auto-play with no URL gate at all;
the browser task only covers the per-face MediaEntry whitelist, which
is object-author-controlled, not viewer-user-controlled.

The rest of the policy set: allow scripts (llSetParcelMusicURL and
friends) to change media (`PermAllowScriptedMedia`), play media
attached to other avatars (`MediaShowOnOthers`), autoplay media on own
HUDs (`MediaAutoPlayHuds`), the first-click-interact object classes —
whether the first click on a media face starts interaction, scoped to
all / anyone's objects / HUDs / own / group / friend / landowner
(`media_first_click_*`) — and media attenuation distances
(`MediaRollOffMin` / `MediaRollOffMax`). `media_prim.rs` already has
`first_click_interact` plumbing but no policy set to consult. A manual
stream URL override ([[viewer-manual-music-stream-url]]) would flow
through the same filter.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/panel_preferences_sound.xml`,
`indra/newview/llviewerparcelmedia.cpp` (`filterMediaUrl`,
`filterAudioUrl`, MediaEnableFilter checks, list load/save),
`indra/newview/fsfloatermedialists.cpp`,
`indra/newview/skins/default/xui/en/floater_media_lists.xml`.
