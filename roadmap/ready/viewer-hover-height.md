---
id: viewer-hover-height
title: Avatar hover-height setting
topic: viewer
status: ready
origin: user request (2026-07)
refs: [viewer-quick-preferences, viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The hover-height slider: the Z offset that lifts or sinks your avatar relative
to the ground, which every resident ends up nudging because mesh bodies and
shoes rarely stand at exactly the height the skeleton thinks they do.

Both halves of the protocol are **already done and unused**:

- **Setting it.** `AgentPreferences.hover_height: Option<f64>` (`sl-wire`), sent
  and read over the `AgentPreferences` cap via `Command::SetAgentPreferences` /
  `Command::RequestAgentPreferences` with the reply as
  `Event::AgentPreferences` — done as `api-g14`. The viewer never calls either.
- **Seeing it.** `AvatarAppearance.hover_height: Option<Vector>` — other
  avatars' offsets arrive in the `AppearanceHover` block of their appearance
  update. The viewer decodes it and ignores it, so everyone else is drawn at the
  wrong height too.

Scope: apply the offset to the rendered avatar (own and others — it shifts the
avatar's apparent ground position, so it must interact correctly with the
locomotion / ground-snap path and with foot IK rather than fighting them), and
persist and round-trip it through the cap. This is the engine-side offset;
exposing a slider is a follow-up — the reference puts it in
[[viewer-quick-preferences]] with the value also visible in the preferences
floater ([[viewer-preferences-floater]]).

Related: the open bug `viewer-r23` (avatar stands too low, feet sinking into the
ground) is *not* this — do not "fix" R23 by dialling in a hover offset; but
whoever works either one should know the other exists, because a hover offset
applied on top of an already-wrong ground height will mask the real defect.

Reference (Firestorm, read-only): `llfloaterhoverheight`, `llagent` hover-height
plumbing, `LLVOAvatar::setHoverOffset`.

Builds on: `api-g14` `AgentPreferences` caps and the decoded
`AvatarAppearance.hover_height`.
