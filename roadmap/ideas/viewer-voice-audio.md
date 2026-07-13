---
id: viewer-voice-audio
title: Voice audio transport (WebRTC + Vivox)
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Turn the existing voice **signalling** into actual talk / listen: modern SL
WebRTC peer + media, the legacy Vivox path, spatial voice mixing, microphone
capture, per-speaker volume, and "who's speaking" indicators.

**Build on a real WebRTC library — do not implement WebRTC ourselves.** First
fleshing-out step: evaluate the Rust WebRTC options (`webrtc` crate, `str0m`, or
a `libwebrtc` binding) against SL's actual offer/answer + codec + DTLS-SRTP
requirements, plus a `cpal`-style mic / output path, and pick one.

**This explicitly supersedes the recorded "voice = signalling only" scope
decision** (Vivox / WebRTC audio transport and "who's speaking" indicators were
previously out of scope). That decision dated from the protocol-library / MVP
era; the fleshing-out agent should consciously reconcile it — and reconsider the
in-scope boundary only if a suitable library genuinely isn't available.

Reference (Firestorm, read-only): `llwebrtc/`, `llvoicewebrtc`, `llvoicevivox`,
`llvoiceclient`, `llvoicechannel`, `llvoicevisualizer`,
`fsfloatervoicecontrols`.

Builds on: `protocol-26` voice signalling + `sl-client-bevy/src/voice.rs`.

Deps: [[viewer-ui-framework]].
