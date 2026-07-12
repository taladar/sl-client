---
id: protocol-26
title: Voice chat (done, signalling)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**26. Voice chat (done, signalling) ✅ — Vivox/WebRTC signalling via CAPS
(`ProvisionVoiceAccountRequest`, `ParcelVoiceInfoRequest`,
`VoiceSignalingRequest`) · 13 pts.** Per the scope decision (as with the
fetch-only #19/#23/#25), this delivers the grid-side **signalling** only — the
audio transport itself (a Vivox SIP/RTP session or a WebRTC peer connection) is
out of scope, the way rendering is for the world-cluster items. A caller that
supplies its own audio engine gets the full CAPS protocol; the WebRTC SDP/ICE it
produces is passed through verbatim and the grid's answer SDP is surfaced
opaque. Implemented in a new `sl-wire/src/voice.rs`:

- **`ProvisionVoiceAccountRequest`** — `Session`-driver command
  `RequestVoiceAccount { VoiceProvisionRequest }`.
  `VoiceProvisionRequest::vivox` POSTs `{ voice_server_type: "vivox" }` (the
  grid replies with the SIP account);
  `VoiceProvisionRequest::webrtc(offer_sdp, channel_type, parcel_local_id)`
  POSTs the nested `jsep` offer (`{ type: "offer", sdp }`) plus `channel_type`,
  `parcel_local_id?` and `voice_server_type: "webrtc"`, and `webrtc_logout`
  tears a session down. The reply (Vivox
  `{ username, password, voice_sip_uri_hostname, voice_account_server_name }`
  **or** WebRTC `{ viewer_session, jsep: { type: "answer", sdp } }`) decodes
  into a single `VoiceAccountInfo` (all-optional fields; `is_webrtc()`
  discriminates) → `Event::VoiceAccountProvisioned`.
- **`ParcelVoiceInfoRequest`** — command `RequestParcelVoiceInfo` POSTs the
  empty (`undef`) body; the reply
  `{ parcel_local_id, region_name, voice_credentials: { channel_uri } }` decodes
  into `ParcelVoiceInfo` (empty `channel_uri` → `None`, i.e. no voice on the
  parcel) → `Event::ParcelVoiceInfo`.
- **`VoiceSignalingRequest`** (WebRTC ICE trickle) — command
  `SendVoiceSignaling` (`viewer_session`, a `Vec<IceCandidate>`, `completed`)
  POSTs the `candidates` array (or the end-of-gathering
  `{ candidate: { completed: true } }`) keyed by the viewer session;
  fire-and-forget (the sim returns only an HTTP status, so no event).

New value types `VoiceProvisionRequest`, `VoiceAccountInfo`, `ParcelVoiceInfo`,
`IceCandidate` and the `VOICE_SERVER_TYPE_VIVOX`/`_WEBRTC` constants; LLSD
builders `build_provision_voice_account_request` /
`build_parcel_voice_info_request` / `build_voice_signaling_request`. Three caps
(`CAP_PROVISION_VOICE_ACCOUNT`, `CAP_PARCEL_VOICE_INFO`, `CAP_VOICE_SIGNALING`)
join the seed; the provision/parcel replies route through
`Session::handle_caps_event`. All wired as `Command`/`SlCommand` variants
through both runtimes (the cap POSTs run on a background task/thread). Field
names and request/response shapes were cross-checked against the Firestorm
viewer (`llvoicevivox.cpp` / `llvoicewebrtc.cpp`) and OpenSim's
`VivoxVoiceModule` / `FreeSwitchVoiceModule`. Covered by five `sl-wire`
unit tests (the Vivox/WebRTC provision build+decode, the parcel-voice
build+decode incl. the no-voice case, and the signaling bodies) and three
`lifecycle.rs` tests (the Vivox and WebRTC provision replies and the
parcel-voice reply through `handle_caps_event`), plus a new `voice` tokio
example. *Test: stock local
OpenSim ships **no** voice module, so the caps are usually absent there (the
commands then no-op and a clean login/logout is observed); real credentials need
a FreeSWITCH/Vivox-configured OpenSim or a Second Life region. Deferred (out of
scope): the audio media transport — opening the SIP/RTP or WebRTC session, audio
codecs, and generating the SDP/ICE.*
