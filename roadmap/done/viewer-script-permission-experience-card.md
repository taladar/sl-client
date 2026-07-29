---
id: viewer-script-permission-experience-card
title: Script permission-request experience card (ScriptQuestionExperience)
topic: viewer
status: done
origin: deferred from viewer-permission-request-dialog (2026-07-29) — the
  experience variant parked pending the experience acceptance/management surface
blocked_by: [viewer-experience-permission-dialog]
refs: [viewer-permission-request-dialog, viewer-permission-active-grants]
---

Context: [context/viewer.md](../context/viewer.md).

The script permission host ([[viewer-permission-request-dialog]]) renders an
experience-backed `ScriptQuestion` (one carrying an `experience_id`) with the
**standard** card: it lists `Participate in an experience` among the requested
permissions and its grant carries the experience id to the session mirror. The
reference viewer instead shows a distinct **`ScriptQuestionExperience`** card:

- names the **experience** (resolved from its key), with the grid-wide /
  region-local wording and the "you will not see this message again for this
  experience unless it is revoked" note;
- offers **Block Experience** alongside Yes / No / Block Object — blocking the
  experience, not just muting the one object;
- on grant, records the acceptance in the **experience permission store** so a
  script running under that accepted experience is thereafter **auto-granted**
  without a prompt (the third auto-grant class the parent task names).

All three need the experience acceptance / management surface —
[[viewer-experience-permission-dialog]] — which owns the allowed / blocked /
forget lists and the `AgentExperience` / `ExperiencePermission` caps. Once that
lands, add the `ScriptQuestionExperience` card shape to the script permission
host and gate the standard card so an accepted experience skips the prompt
entirely (register the auto-grant so [[viewer-permission-active-grants]] and the
control / camera consumers see it).

Reference (Firestorm, read-only): the `ScriptQuestionExperience` notification
and `process_script_experience_details` / the `experience_permission` event pump
in `llviewermessage.cpp`, `LLExperienceCache`.

## Outcome (2026-07-29)

Delivered together with (and by) [[viewer-experience-permission-dialog]] — the
two tasks describe the same `ScriptQuestionExperience` card, so implementing the
acceptance surface completed this one.

`src/script_permission.rs` now **skips** a non-caution experience-backed
`ScriptQuestion` (`experience_id.is_some()`), and `src/experience_permission.rs`
raises the distinct experience card instead: it resolves the experience name /
grid-vs-land scope (`RequestExperienceInfo`), shows the "not shown again unless
revoked" note, offers **Block Experience** (and Block Object) beside Yes / No,
and on grant records the acceptance in the experience permission store
(`SetExperiencePermission` `Allow`).

Not done — and now understood to be moot:
**no client-side auto-grant registration**. The reference does not auto-grant
for an accepted experience; the **simulator** computes the implicit grant and
simply does not send a `ScriptQuestion` for an already-accepted experience (see
the `src/script_permission.rs` module docs). So there is nothing for
[[viewer-permission-active-grants]] to register from an acceptance — the "gate
the standard card" the original note imagined is the sim's job, already handled
by routing the (only-ever-arriving, not-yet-accepted) experience request to the
experience card.
