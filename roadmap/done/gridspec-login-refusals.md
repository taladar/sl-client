---
id: gridspec-login-refusals
title: Login refusal codes and texts, MFA, TOS/critical and presence on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-login-tos, viewer-login-screen,
  test-e2e-sweep-live-grid, gridspec-sl-mfa-challenge,
  viewer-login-refusal-localised-text, viewer-disconnect-screen]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-06)

Measured and written up in `book/src/gridspec/login.md` § Refusals.

- **Discover.** The new `login-refusals` case keeps one avatar in world and
  makes the declined logins beside it (`Session::attempt_login`: one request,
  never retried, the aditi cooldown waited out before each): a wrong
  password, an unknown account, and a second login of the avatar itself. It
  records each answer whole — `LoginFailure` and `MfaChallenge` now list
  every field a response carried (`response_fields`), `LoginFailure` keeps
  Second Life's incident id (`error_code`), and the tokio client's
  `LoginRejected` carries all of it, as the bevy client's already did. Run on
  aditi (twice), OpenSim and both fake flavours.
- **Findings.** Both grids answer a wrong password and an unknown account
  identically, as `key`. Second Life adds `message_id`
  (`LoginFailedAuthenticationFailed`), an empty `message_args` and
  `Linden_Error_Code`, with or without `extended_errors`. A second login is
  **admitted** on Second Life and **refused** (`presence`) on OpenSim; both
  kick the session already in world, each with its own `KickUser` text, and
  the login after that gets in.
- **Fake grid.** `ImitatedGrid::login_refusals`: each flavour's texts, the
  Second Life-only fields, and the second login (it used to be admitted
  beside the first, which stayed in world, on both). An unknown account is
  now answered exactly as a wrong password is — the two texts differed, in a
  function whose comment said they did not. `SimSession::kick`.
  `login-refusals` is in the offline list for both flavours.
- **Viewer.** `e2e_login` gained four tests: a refused login ends the run
  with each flavour's reason and text, and a second login from elsewhere
  kicks the viewer, which exits cleanly, on both. The viewer does not read
  `message_id` ([[viewer-login-refusal-localised-text]]) and shows a kick
  only in its log ([[viewer-disconnect-screen]], which now carries the two
  measured texts).
- **Not done here.** The MFA challenge shape and `mfa_hash` reuse: aditi did
  not challenge the test account on any login that day, so there was nothing
  to record — [[gridspec-sl-mfa-challenge]]. `tos` / `critical` are noted in
  [[test-e2e-sweep-live-grid]].

## Known already

The fake grid's refusals (`login_endpoint.rs`) use the wire crate's own
reasons; OpenSim's presence path (evict the ghost, then refuse) is read from
source; aditi MFA works through `mfa_command`.

## Discover

- `sl-repl --script` probes with a wrong password and an unknown account on
  both grids (**on aditi sparingly**: lockout risk, the shared cooldown
  guard); a second concurrent login of one avatar (presence).
- Record the MFA challenge shape and the `mfa_hash` reuse on aditi.
- TOS / critical messages cannot be provoked: record them opportunistically
  (note in [[test-e2e-sweep-live-grid]]).

## Document

`book/src/gridspec/login.md` § Refusals.

## Fake grid

Small — in this task: the measured reason codes and messages per flavour.

## Viewer

Presence classed as retryable, the MFA prompt, the refusal text shown
verbatim; `e2e` against each fake flavour's refusals.
