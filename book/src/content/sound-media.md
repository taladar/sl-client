# Sound, Music & Media

Audio and media come in several distinct channels, and it helps to keep them
apart: short **sound effects** attached to objects, a parcel's **streaming
music**, **media** surfaces (web/video on a prim or parcel), and real-time
**voice** between avatars. They use different transports and different formats.

## Sound effects

Sound effects are short audio assets triggered in-world — a door clicking, a
gesture, a scripted noise. The region tells the client about them over
[UDP](../comms/lludp-transport.md):

- **Triggered sound** — a one-shot played at a location (`Event::SoundTrigger`).
- **Attached sound** — a sound bound to an object, which can loop and be stopped
  (`Event::AttachedSound`, with `Event::AttachedSoundGainChange` for volume
  changes). A set of **sound flags** controls looping, master/slave
  synchronization, queueing, and stopping.
- **Preload** — a hint to fetch a sound asset before it is needed, so playback
  is not delayed (`Event::PreloadSound`).

The sound *asset* itself is fetched like any other asset (by UUID, over the
asset [capabilities](../comms/caps.md)); these messages only say *what* to play,
*where*, and *how loud*.

The client can also **trigger** a one-shot sound itself:
`Command::TriggerSound { sound, gain, region_handle, position }` plays a sound
asset at a region-local position and linear `gain` (`0.0`..=`1.0`) — the
viewer→sim counterpart of the inbound `Event::SoundTrigger`. The
owner/object/parent ids are left for the simulator to fill, and (like the
reference viewer) it is sent unreliably as best-effort.

## Streaming music

Each [parcel](world.md#parcels) can advertise a **streaming audio URL** — an
ordinary internet radio / Shoutcast-style stream. It is delivered as part of the
parcel's properties, and the client simply hands the URL to an external audio
player. The protocol's only role is carrying the URL; it does not proxy the
audio.

## Media (web & video surfaces)

"Media" is richer than music: a web page, video, or image surface shown on a
parcel or on individual faces of an object. This is **media-on-a-prim (MOAP)**,
handled over [CAPS](../comms/caps.md):

- a **media entry** describes a surface's current URL, MIME type, size,
  auto-scale and looping, and per-role interaction permissions,
- the `ObjectMedia` capability gets and sets an object's per-face media
  (`Command::RequestObjectMedia` / `SetObjectMedia`), and the
  `ObjectMediaNavigate` capability points a surface at a new URL
  (`Command::NavigateObjectMedia`); results arrive as `Event::ObjectMedia`.

Older **parcel media** (a single video/image for a whole parcel) is separate:
its settings arrive as `Event::ParcelMediaUpdate`, and scripts can drive
playback — play, stop, pause, unload, seek — surfaced as
`Event::ParcelMediaCommand`.

## Voice

Voice is real-time spatial or group audio. The actual media path is **not** part
of LLUDP — it is handled by a separate voice subsystem, and the protocol's job
is only to *provision* the client into it:

- **Vivox** (the long-standing system) — the client requests an account and a
  parcel's channel credentials, then connects a Vivox/SIP client out-of-band.
- **WebRTC** (the newer Second Life path) — provisioning exchanges SDP offers
  and trickles ICE candidates through capabilities.

The relevant capabilities are `ProvisionVoiceAccountRequest`,
`ParcelVoiceInfoRequest`, and (WebRTC) `VoiceSignalingRequest`. The client
drives them with `Command::RequestVoiceAccount`, `RequestParcelVoiceInfo`, and
`SendVoiceSignaling`, and receives `Event::VoiceAccountProvisioned` and
`Event::ParcelVoiceInfo`. Actually carrying the audio is left to a voice client
the application supplies.

---

> **In this codebase**
>
> - Sound types/flags: `SoundFlags`, `SoundPreload` in
>   `sl-proto/src/types/appearance.rs`; events `SoundTrigger`, `AttachedSound`,
>   `AttachedSoundGainChange`, `PreloadSound` in `sl-proto/src/types/event.rs`.
>   The outbound `Command::TriggerSound` (helper `Session::trigger_sound`,
>   reusing typed `AssetKey` / `RegionHandle` / `RegionCoordinates`) decodes on
>   the simulator side as `ServerEvent::TriggerSound`
>   (`sl-proto/src/sim_session.rs`); REPL token `trigger_sound`.
> - Media: `ParcelMediaUpdateInfo`, `ParcelMediaCommand` in
>   `sl-proto/src/types/parcel.rs`; `MediaEntry` / `ObjectMediaResponse` (and
>   the navigation white-list check `MediaEntry::check_candidate_url`) in
>   `sl-wire/src/llsd.rs`; CAPS driver `sl-client-tokio/src/media.rs`; example
>   `sl-client-tokio/examples/object_media.rs`. Caps `CAP_OBJECT_MEDIA`,
>   `CAP_OBJECT_MEDIA_NAVIGATE`.
> - Rendering media: the engine-agnostic `MediaBackend` / `MediaSurface`
>   boundary lives in the `sl-media` crate (CPU BGRA frames, portable
>   VK-code + text input, navigation *and* playback status), with **two**
>   engines behind it: `sl-cef` embeds offscreen Chromium for web pages
>   (isolated per-surface request contexts, `sl-cef-helper` subprocess
>   binary) and `sl-gst` plays direct video / audio URLs through GStreamer
>   (`playbin3` + BGRA `appsink`, system decoders only, loud
>   `missing-plugin` errors). `sl_media::classify_url` dispatches a media
>   URL to its engine by scheme / extension (the reference's
>   `mime_types.xml` role). The Bevy viewer drives both in
>   `sl-client-bevy-viewer/src/media_engine.rs` (pump + frame mirror),
>   `browser_widget.rs` / `web_floater.rs` (embedded browser UI),
>   `media_prim.rs` (media-on-a-prim surfaces, focus and input routing) and
>   `media_controls.rs` (the floating per-face controls bar — browser
>   chrome for pages, transport + seek scrubber for video).
> - Parcel streaming audio: `sl_gst::AudioStreamPlayer` (audio-only
>   `playbin3`, ICY "now playing" tags) driven by
>   `sl-client-bevy-viewer/src/parcel_audio.rs` — per-parcel switching off
>   `music_url`, an autoplay policy behind the persisted
>   `MusicStreamEnabled` / `MusicStreamVolume` settings, and the bottom-bar
>   play / mute / volume cluster.
> - Voice: caps `CAP_PROVISION_VOICE_ACCOUNT`, `CAP_PARCEL_VOICE_INFO`,
>   `CAP_VOICE_SIGNALING`; LLSD helpers in `sl-wire/src/voice.rs`; driver
>   `sl-client-tokio/src/voice.rs`; example `sl-client-tokio/examples/voice.rs`.
>   Events `VoiceAccountProvisioned`, `ParcelVoiceInfo`.
