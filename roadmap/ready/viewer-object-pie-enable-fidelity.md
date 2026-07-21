---
id: viewer-object-pie-enable-fidelity
title: Reference-faithful object pie enable predicates + mute naming
topic: viewer
status: ready
origin: follow-up from viewer-object-context-menu (2026-07-21)
refs: [viewer-object-context-menu, viewer-object-selection-core]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-object-context-menu]] wired the object pie's simple actions behind
**deliberately simplified** enable gates, reading only the update-flag bits of
the picked linkset. The reference's predicates are richer; close the gaps:

- **Take / Delete / Return** are gated on you-owner
  (`FLAGS_OBJECT_YOU_OWNER`) only. The reference also admits: god mode
  (take / delete anything), group-deeded objects where the agent's role
  permits, and — for Return — any object over land the agent owns or manages
  (`Object.EnableReturn`), plus the locked / no-transfer refinements of
  `visible_take_object`.
- **Take Copy** reads `FLAGS_OBJECT_COPY` alone; the reference's
  `Tools.EnableTakeCopy` also excludes buy-only and permanent (pathfinding)
  objects.
- **Touch** reads `FLAGS_HANDLE_TOUCH` alone; the reference's
  `Object.EnableTouch` also enables on the object's **click action** (touch /
  play / open / pay / buy), and the reference relabels the slice from the
  click action (e.g. a pay-action object shows `Pay` behaviourally on left
  click). Decide how much of the click-action surface belongs here vs the
  left-click path.
- **Sit Here** is enabled whenever standing; the reference's
  `Object.EnableSit` refinements (already seated on *this* object, sit
  disabled by script) are not read.
- **Mute name race**: the pie fires `RequestObjectPropertiesFamily` on open
  and a Mute picked before the reply lands goes out with an **empty name**
  (the mute list then shows a blank row). Hold the mute until the name is
  known, or retro-update the mute-list entry when the reply arrives.

Several of these need data the update stream does not carry (land ownership,
group roles, god state, script sit flags); pull each from the session state
that owns it as those surfaces land, and light the predicates up one deliberate
edit at a time — addresses never move.
