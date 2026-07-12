---
id: protocol-63
title: Voice service pairing (extends #26, Tier D). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**63. Voice service pairing (extends #26, Tier D). ✅ Done.**
`sl-wire/src/voice.rs` had the request builders +
`VoiceAccountInfo`/`ParcelVoiceInfo::from_llsd`; this added the server-side
inverse. **Request parsers** (inverse of the body builders, via #52's
`parse_llsd_xml`, lenient field-by-field defaults):
`parse_provision_voice_account_request` → `VoiceProvisionRequest` (the populated
fields select Vivox / WebRTC / logout, mirroring the builder; the nested `jsep`
offer SDP is read regardless of the always-`"offer"` type), and
`parse_voice_signaling_request` → `(viewer_session, Vec<IceCandidate>,
completed)` (the WebRTC ICE trickle — the `candidates` array *or* the
end-of-gathering `candidate.completed` flag, never both). **Response builders**
(server output, built on #52's `Llsd::to_llsd_xml`, inverse of the `from_llsd`
decoders): `VoiceAccountInfo::to_llsd` +
`build_provision_voice_account_response`
(only populated fields emitted — Vivox SIP keys, or the WebRTC session id +
nested JSEP `answer`) and `ParcelVoiceInfo::to_llsd` +
`build_parcel_voice_info_response` (the no-voice case emits an empty
`channel_uri`, the grid's drop-out-of-spatial-voice form). All re-exported from
`sl-wire` AND `sl-proto`. The voice *audio* transport stays out of scope — this
is only the signalling-endpoint pairing. *Test: 4 new unit round-trips in
`voice.rs` (provision request both backends + logout, signaling request both
forms, provision reply Vivox + WebRTC, parcel-voice reply incl. no-voice),
alongside the 5 existing client-side tests.* **Next = #64/F13.**
