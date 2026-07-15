---
id: viewer-permission-request-dialog
title: Script permission-request dialog (ScriptQuestion)
topic: viewer
status: blocked
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
