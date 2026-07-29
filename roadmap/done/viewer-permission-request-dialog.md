---
id: viewer-permission-request-dialog
title: Script permission-request dialog (ScriptQuestion)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The script run-time permission-request dialog (`ScriptQuestion`): show the
requesting object / script and the requested permission bits (take controls,
control camera, take money/debit, trigger animation, attach, track camera,
teleport, …), accept / decline, and send the grant reply (`ScriptAnswerYes` with
the granted mask). Hosted in the [[viewer-ui-notification-host]].

**Honour the auto-grant exceptions that bypass the dialog entirely.** Certain
requests are granted automatically without a prompt when the requesting object
is:

- an **attachment** worn by the agent,
- an object the agent is **sitting on**, or
- a script running under an **accepted experience**
  ([[viewer-experience-permission-dialog]]).

Those get a fixed auto-granted subset (take-controls, trigger-animation,
track/control-camera, attach); the dialog must **not** prompt for them and the
grant must still register so downstream consumers
([[viewer-input-script-control-capture]], [[viewer-camera-script-control]]) see
it. Active grants are tracked/revoked by [[viewer-permission-active-grants]].

Reference (Firestorm, read-only): `lltoastscriptquestion`, `llscriptfloater`,
`llnotifications`; auto-grant rules in `LLScriptQuestion` /
`process_script_question`.

Builds on: the existing permission-request protocol handshake.

## Done

New viewer module `src/script_permission.rs` (`ScriptPermissionPlugin`), a
sibling of the script-dialog / load-url hosts. It consumes
`Event::ScriptPermissionRequest` (`ScriptQuestion`) and raises a **sticky**
`Alert` card into the shared notification host in two reference-faithful shapes:

- **standard** (`ScriptQuestion`): `'Object', an object owned by Owner, would
  like to:` + a bulleted line per requested permission + `Is this OK?` +
  **Yes / No / Block**;
- **caution** (`ScriptQuestionCaution`, `Critical` priority) when the request
  asks to **debit** L$: the money-access warning + any other requested
  permissions + **Allow access / Deny**.

Yes/Allow replies `ScriptAnswerYes` (`Command::AnswerScriptPermissions`) with
the recognised requested mask; No/Deny replies an empty mask (explicit deny);
Block denies **and** mutes the object; the close **×** denies conservatively (a
dismissed prompt never leaves a silent grant). The grant carries the request's
`experience_id` so the session grant mirror records it. Unmodelled permission
bits are dropped, matching the reference. All card text (the twelve reference
`[QUESTIONS]` strings included) lives in `en/main.ftl`; two gallery specimens
(standard and caution) are registered in `ELEMENTS` and swept by `ui_test`; four
pure unit tests cover the recognised-mask / caution / listed-keys logic.

**Auto-grant is the simulator's job, not the viewer's** (the one real deviation
from the task text above). Verified in both references: OpenSim's
`llRequestPermissions` computes `implicitPerms` for an attachment / sat-on
object and sends a `ScriptQuestion` only for the *non-implicit remainder*, and
Firestorm's `process_script_question` has no client-side auto-grant. So the
viewer never receives an implicit-only request, and a client-side auto-grant
would be dead code — this host faithfully prompts for whatever the sim actually
asks. The accepted-experience management surface (experience name,
*Block Experience*, the acceptance store, the `ScriptQuestionExperience` card)
stays with [[viewer-experience-permission-dialog]]; here an experience request
lists `Participate in an experience` and its grant carries the experience id.
Wiring the reference `ScriptQuestionExperience` card (experience name,
*Block Experience*, the accepted-experience auto-grant) into this host once that
surface lands is the follow-up [[viewer-script-permission-experience-card]]. The
caution card is used for every debit request — its `PermissionsCautionEnabled`
gate belongs to [[viewer-preferences-alerts-tab]].
