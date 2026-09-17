---
id: viewer-sit-on-neighbour-object-uses-root-circuit
title: Sitting on an object in a neighbour region fails silently instead of "move closer"
topic: viewer
status: done
origin: hover-tooltip neighbour-region "Loading…" investigation (2026-09-17)
refs: [viewer-hover-tooltip-202ms-frame-spike]
---

Context: [context/viewer.md](../context/viewer.md).

Sitting on an object in a neighbour region is **not supposed to work**: the
simulator refuses a sit from a child agent. OpenSim's `LLClientView`
(`HandleAgentSit` → `SendCantSitBecauseChildAgentResponse`) answers with the
alert "Try moving closer. Can't sit on object because it is not in the same
region as you.", and Firestorm shows the same text (its `SitFailNotSameRegion`
notification) — the user's experience in Firestorm is exactly that message.

What the viewer should do is reach that refusal the way the reference does.
`Session::sit_on` sends `AgentRequestSit` on the **root** circuit and arms the
sit timer there, while the reference sends it to the object's own region
(`llviewermenu.cpp`: `object->getRegion()->sendReliableMessage()`). The root
simulator does not know the object at all, so instead of the "move closer"
answer the request presumably goes unanswered until the sit timer fires.

This was left out of the object-region routing that landed with
[[viewer-hover-tooltip-202ms-frame-spike]] (`Session::circuit_for_object`)
because the sit is a small exchange rather than one send: the neighbour's
`AvatarSitResponse` (answered with `AgentSit`, which is what the neighbour
refuses) and its `AlertMessage` arrive on the child circuit, which
`dispatch_child` does not handle today, and the `SitState` machine and its
timer assume the root circuit.

## Verify

First confirm the current symptom on aditi (sit on a prim just across a region
border: what, if anything, is shown, and does the sit timer fire), and check
what Second Life's simulator sends a child agent (the OpenSim path above is
the model, not proof for SL). Then route the request to the object's circuit,
handle the child's sit response / alert, and confirm the viewer shows the same
"move closer" refusal as Firestorm.

## Findings (2026-09-17)

**Second Life (aditi), before the change.** A `sl-repl-tokio` probe logged in
at Ahern and sat on a root prim in the west neighbour, Dore. The request went to
the **root** simulator. Unlike the premise above, Second Life's root simulator
**does** answer: ~3 s later it sent `AlertMessage` "Try moving closer…" with
`AlertInfo` `SitFailNotSameRegion`. The session did not treat that as the end
of the sit, so 15 s later the sit timer fired
`ExpectedReplyMissing request=Sit`. In the viewer that is the user-visible
"request got no reply" toast (`USER_VISIBLE_REQUESTS`), shown right after the
refusal. On OpenSim the root simulator does not know the object, and the
neighbour's refusal (bare text, no `AlertInfo`) could not arrive, because
nothing had asked the neighbour.

## Fix

- `Session::sit_on` sends `AgentRequestSit` on `circuit_for_object`, the
  object's own region, as the reference does. The sit deadline stays on the
  root circuit (the wait is the agent's), and the pending state records the
  circuit asked: `SitState::AwaitingResponse { circuit }`.
- `dispatch_child` handles `AlertMessage` the way the root does, as
  `Event::AlertMessage` (shared `handle_alert_message`). The reference's
  `process_alert_message` takes alerts from any region.
- A refusal ends the pending sit (`end_sit_refused_by`), clearing the timer so
  no "no reply" is reported. A refusal is either a named sit refusal from any
  region (`SIT_REFUSAL_ALERTS`: `SitFailNotSameRegion`, `SitFailCantMove`,
  `SitFailNotAllowedOnLand`, `CantSitNoRoom`, `CantSitNoSuitableSurface`, the
  reference's refusal notifications) or any alert from the neighbour that was
  asked, since a child agent's sit can only be refused and OpenSim does not
  name it.
- The sit timeout reports a missing reply only while a sit is still pending. A
  sit a stand or teleport already ended no longer times out afterwards.
- Tests (`lifecycle`): the neighbour sit goes to the child circuit, and its
  bare-text refusal is surfaced and ends the sit; a named refusal on the root
  ends it, while another alert does not; a sit ended by a stand is not
  reported missing. The existing timeout test is unchanged and passes.

The viewer needed no change. `ingest_alert_messages` already raises the keyed
`SitFailNotSameRegion` catalogue notification, and OpenSim's plain text as a
system message.

Scope note: the named refusals also end a pending sit on an object in the
agent's own region (e.g. "no room"). That is the same defect (a refusal
followed by a false "no reply"), fixed by the same check.

## Live verification

- **aditi**, the same probe after the change: the request goes to Dore, and
  `SitFailNotSameRegion` arrives in **0.2 s** (3 s via the root before). No
  sit timeout.
- **Local OpenSim**: the agent in Default Region (250, 134) sits on a root prim
  in East Region. The neighbour's bare-text "Try moving closer…" is surfaced
  10 ms after the request. No sit timeout.
- An unrelated logout-reply timeout seen in these OpenSim runs, and in a
  control run without a sit, is filed as
  [[protocol-logout-reply-sometimes-missing-on-opensim]].
