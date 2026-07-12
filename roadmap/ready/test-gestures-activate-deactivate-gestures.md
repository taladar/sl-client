---
id: test-gestures
title: activate / deactivate gestures
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`gestures` — activate / deactivate gestures. `1av`.

**Library follow-up (deferred, not a test).** A high-level appearance API
that abstracts over the two ways to change an avatar's appearance — the
legacy client-side bake (`UploadBakedTexture` + `AgentSetAppearance`,
OpenSim) and modern server-side central baking (Current Outfit Folder over
AIS3 + `UpdateAvatarAppearance`, Second Life) — dispatching by capability
presence the way the runtimes already select paths elsewhere. Caveat that
bounds the design: the legacy path needs a real JPEG-2000 *baking* pipeline
the headless client lacks (`sl-texture` is decode-only), so a genuine "apply
this outfit" is fully realizable only on the server-side path; the legacy
side can advertise pre-computed bakes but not composite new ones. To be
designed before any code.
