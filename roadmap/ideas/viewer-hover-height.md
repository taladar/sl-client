---
id: viewer-hover-height
title: Avatar hover-height setting
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-framework]
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
locomotion / ground-snap path and with foot IK rather than fighting them),
persist and round-trip it through the cap, and expose the slider. The natural
home for the slider is [[viewer-quick-preferences]] (where the reference viewer
puts it) with the value also visible in [[viewer-preferences-ui]] — hence the
wait for the UI framework.

Related: the open bug [[viewer-r23]] (avatar stands too low, feet sinking into
the ground) is *not* this — do not "fix" R23 by dialling in a hover offset; but
whoever works either one should know the other exists, because a hover offset
applied on top of an already-wrong ground height will mask the real defect.

Reference (Firestorm, read-only): `llfloaterhoverheight`, `llagent` hover-height
plumbing, `LLVOAvatar::setHoverOffset`.

Builds on: `api-g14` `AgentPreferences` caps and the decoded
`AvatarAppearance.hover_height`.

Deps: [[viewer-ui-framework]] (the slider has nowhere to live until the UI
exists).
