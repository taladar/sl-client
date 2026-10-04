---
id: viewer-login-voice-config-unread
title: The login response's voice-config is decoded and read by nobody
topic: viewer
status: done
origin: found flipping the fake grid to Second Life's login-options behaviour (2026-09-07)
points: 1
refs: [test-fake-grid-imitates-audit]
---

Context: [context/viewer.md](../context/viewer.md).

`LoginSuccess::voice_config` is decoded by `sl-wire`, sent by the fake grid
(`runtime.rs` builds it from the scenario's voice backend), and asserted by
`sl-fake-grid`'s `http_glue` test — and **nothing between the login response and
the viewer ever reads it**. A grep for `voice_config` outside `sl-wire` and
`sl-fake-grid` finds nothing.

Voice is fully in scope for this workspace, so this is not a field to delete on
sight; it is a field that has not been wired up. The viewer learns the voice
backend by two other routes today — `SimulatorFeatures.VoiceServerType` and the
`RequiredVoiceVersion` push on region entry — so nothing is broken. What is
missing is the *earliest* one: a viewer knows which backend the grid speaks
before it has a region at all, which is when it would decide whether to load a
voice implementation.

Surfaced by [[test-fake-grid-imitates-audit]]: the fake grid started honouring
the request's `options` list the way Second Life does, and `voice-config`
promptly vanished from the response because `LoginRequest::new` does not ask for
it. It does not ask for it because nothing reads it, which is the right rule —
so the fix is one of two things, not a third:

- read it, and add `voice-config` to the requested options; or
- decide the two later routes are enough, and say so where the field is
  declared, so the next person does not re-find this.

Acceptance: `voice_config` is either load-bearing (read by the viewer, and
requested at login) or documented as deliberately unused, and the `http_glue`
test's explicit `options.push("voice-config")` either becomes unnecessary or is
explained by that decision.

## Done (2026-10-04)

Settled by measurement in [[gridspec-login]]: **neither live grid sends
`voice-config`** — aditi did not even when the request asked for the option,
and a stock OpenSim has no such section. So the field is documented as
deliberately unread where `LoginSuccess::voice_config` is declared, the
viewer keeps learning the backend from `SimulatorFeatures.VoiceServerType` and
`RequiredVoiceVersion`, and the fake grid stopped sending the section on
either flavour; the `http_glue` test now asks for it and asserts it is absent.
