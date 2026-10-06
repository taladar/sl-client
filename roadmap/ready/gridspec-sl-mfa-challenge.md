---
id: gridspec-sl-mfa-challenge
title: Record Second Life's MFA challenge and whether a remembered mfa_hash gets past it
topic: gridspec
status: ready
origin: gridspec-login-refusals (2026-10-06)
refs: [gridspec-login-refusals, gridspec-login, test-e2e-live-grids]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The client and the fake grid model a challenge as `reason: mfa_challenge`
with a `message` and an `mfa_hash`, and a success that may carry an
`mfa_hash` to send back instead of a code — from the 2026-06-25 aditi logins.
The reference viewer shows the challenge's `message_id` looked up in its
string table (`lllogininstance.cpp`), not its `message`.

On 2026-10-06 aditi challenged the test account on **none** of seven logins,
although its credentials carry an `mfa_command`: a login with neither a code
nor a hash was admitted, and no success carried an `mfa_hash`. Whether the
account's second factor is off on aditi, or aditi stopped asking, is not
known.

## Discover

`login-refusals` already records it when it happens (`mfa_challenge_*`: the
message, the fields, the hash's length; `mfa_hash_reused`): it sends the
second login bare first, then with the remembered hash alone, then with a
code. So this is a re-run once an account is challenged again — check the
account's second factor on aditi first (the user's call), or run the case on
the main grid.

Also: what a **wrong code** is answered with (`key`, going by the wrong
password text's "Second Factor Token (if enabled)" line) — one attempt.

## Document

`book/src/gridspec/login.md` § Refusals, the first "Not measured" row.

## Fake grid

`MfaPolicy` per flavour if the shape differs from what it sends today (a
`message_id`, an incident id).

## Viewer

Show the localised challenge text
([[viewer-login-refusal-localised-text]]).
