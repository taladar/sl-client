---
id: viewer-sit-on-neighbour-object-uses-root-circuit
title: Sitting on an object in a neighbour region fails silently instead of "move closer"
topic: viewer
status: bugs
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
