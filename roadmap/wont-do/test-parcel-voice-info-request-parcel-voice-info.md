---
id: test-parcel-voice-info
title: request parcel voice info
topic: test
status: wont-do
origin: TEST_ROADMAP.md — Phase 17 — Voice signalling `[aditi] 1av`
---

Context: [context/test.md](../context/test.md).

`parcel-voice-info` — request parcel voice info. `1av`.

**Won't do: a Vivox-era capability with no modern equivalent.** The
`ParcelVoiceInfoRequest` capability is used **only** by the viewer's Vivox
client (`llvoicevivox.cpp`); the WebRTC client never sends it — the WebRTC
spatial channel is bound through the per-session `ProvisionVoiceAccountRequest`
(`channel_type` / `parcel_local_id`) instead. Linden Lab has removed Vivox
completely, so Second Life (aditi) no longer meaningfully answers this cap, and
we will not implement Vivox to drive it on an opt-in OpenSim FreeSWITCH/Vivox
module. Unlike the deferred WebRTC voice cases ([[test-voice-account]] /
[[test-voice-signaling]]), this one is not waiting on the viewer's future voice
stack — there is no WebRTC parcel-voice-info request to replace it with, so
there is nothing here to revisit. The client-side signalling
(`Command::RequestParcelVoiceInfo` → `Event::ParcelVoiceInfo`) and its LLSD
decode already exist and are unit-tested in `sl-wire` for legacy/FreeSWITCH-only
grids; that decode stays, but no live conformance case will exercise it.
