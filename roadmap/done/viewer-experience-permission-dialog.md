---
id: viewer-experience-permission-dialog
title: Experience permission flow (accept / manage)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The **Experience** permission flow: the one-time experience-acceptance prompt
(accept / block) and a manage-experiences surface (allowed / blocked / forget).
This matters for permissions because a script running **under an accepted
experience** is auto-granted permissions without the per-request
[[viewer-permission-request-dialog]] — so accepting/blocking/forgetting an
experience is the real control point for that whole class of auto-grants.

Builds on the existing experience protocol (`experiences.rs`; the deferred
`protocol-34` experience key-value store is **not** required here).

Reference (Firestorm, read-only): `llfloaterexperiences`, `llexperiencelog`,
`llpanelexperiences`, and the `AgentExperience` / `ExperiencePermission` caps.

## Outcome (2026-07-29)

Two viewer surfaces on the existing (complete) experience protocol layer.

**Accept prompt** — `src/experience_permission.rs`
(`ExperiencePermissionPlugin`): the reference `ScriptQuestionExperience` toast.
A `ScriptQuestion` naming an experience (its `Experience.ExperienceID`) and not
a money-caution request is now **skipped** by `src/script_permission.rs` (its
permission-line helpers `recognized_mask` / `other_permission_keys` /
`is_caution` are now `pub(crate)`) and routed here. Because acceptance is
lasting and the card needs the experience's name / scope — absent from the
message — this host **defers** like the reference: it records the request keyed
by experience id, fetches `RequestExperienceInfo`, and raises the card only once
`ExperienceInfo` resolves. Yes admits the experience (`SetExperiencePermission`
`Allow`) and grants the recognised bits; No denies; **Block Experience**
denies + blocks; **Block Object** denies + mutes; the close × denies
conservatively. Emerald accent, sticky `Alert`, not persisted — matching the
sibling script dialogs.

**Manage surface** — `src/experiences_floater.rs` (`ExperiencesPlugin`), opened
from Avatar ▸ Experiences: the Allowed / Blocked lists with a per-row **Forget**
(`SetExperiencePermission` `Forget`, optimistic). Names resolve via
`RequestExperienceInfo` folded in as `ExperienceInfo` arrives (group-name-cache
shape). Key subtlety: the live-grid `ExperiencePreferences` PUT/DELETE reply
carries only the single edited experience, which `sl-proto` collapses into the
**same** `ExperiencePermissions` event as the full-list GET reply (but empty),
so the floater accepts that event as authoritative **only** while a GET it
issued is outstanding (`pending_full_list` counter); a forget updates
optimistically and ignores its own reply.

Gallery specimens for both (`experience-permission-toast`,
`experiences-floater`) sweep the layouts login-free; 6 new unit tests.
**Not live-verified**: an experience `ScriptQuestion` needs a scripted object
running under an experience, and experiences are SL-only (absent on OpenSim) —
exercise on aditi.

Deferred, unchanged from the reference: the experience-name line is plain text
(SLURL linkification is the shared [[viewer-url-linkification]] layer); the
floater covers allowed/blocked/forget only (the Admin / Contributor / Owned tabs
and experience-profile editing are separate surfaces).
