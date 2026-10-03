---
id: gridspec-login-refusals
title: Login refusal codes and texts, MFA, TOS/critical and presence on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-login-tos, viewer-login-screen,
  test-e2e-sweep-live-grid]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
