---
id: protocol-agent-access-meaning
title: What the login response's agent_access actually means
topic: protocol
status: deferred
origin: measured but not explained doing protocol-account-benefits-package (2026-09-08)
refs: [protocol-account-benefits-package]
---

Context: [context/protocol.md](../context/protocol.md).

**Deferred deliberately, and not work to go looking for.** Settling this needs
either an age-verified Second Life avatar or a login at a differently-rated
start region — a second account or a manual relocation, neither of which an
agent should go and arrange. Nothing is blocked on the answer: no code in this
workspace reads the field, and the reference viewer does not read it at login
either.

So: **do not actively pursue this.** If the meaning turns up anyway — in a
Linden wiki page, a viewer source comment, a protocol reference, or a
conformance run that happens to log in somewhere differently rated — resolve it
then and move this to `done/`. That is the only way it should get closed.

## What is known

`agent_access` is one of three maturity fields in the login response, and the
only one whose rule is not established:

| field | what it is |
| --- | --- |
| `agent_access_max` | the entitlement; a client may not raise its preference above it |
| `agent_region_access` | the account's *preference*, seeded into `PreferredMaturity` |
| `agent_access` | **unexplained** |

It is not the preference and not the ceiling. Aditi answered `M` on three runs
(2026-09-08) for an account whose ceiling *and* preference were both `A`, and
OpenSim's `LLLoginResponse` hard-codes `M` for every avatar with no per-account
maturity anywhere in the grid.

## The two readings that fit

- **A clearance** — what the account is cleared for, as against what its type
  permits. A beta account that has never been age-verified would be cleared to
  Moderate while entitled to Adult, which is exactly the measured `M`/`A`. The
  same axis carried the pre-2010 Teen Grid restriction, PG-cleared accounts on
  an adult-capable grid; that population aged out long ago, so it explains the
  field's shape rather than any case still in the wild.
- **The start region's own rating**, nothing to do with the account at all.

## Why the measurement could not separate them

`login-handshake` records `start_region_maturity` beside the account fields for
exactly this purpose, and the answer was unlucky: that avatar's aditi start
region is *itself* Mature, so both readings predict the `M` observed. The local
OpenSim grid does show the field diverging from its region (a `Pg` region still
answering `M`) — but OpenSim hard-codes the value, so it says nothing about
Second Life.

What the run does rule out is a **vestigial constant**: the value coincides with
something rather than sitting where it was left, which "it means nothing any
more" does not explain.

## What would settle it

One login, no new machinery — the metric is already recorded:

- an aditi login from a **PG** or **Adult** start region: if `agent_access`
  follows it, the field is about the region; if it stays `M`, it is about the
  account; or
- a login by an **age-verified** avatar: if `agent_access` reads `A` where an
  unverified one reads `M`, the clearance reading is confirmed.

If the region reading wins, `sl-fake-grid` should derive the field from the
region rather than from the account's ceiling (`runtime.rs`, `enrich_success`),
and `AccountConfig::maturity_ceiling` stops being the input to it. If the
clearance reading wins, `AccountConfig` wants an age-verification knob so a
viewer's adult-content paths can be tested against an unverified account.

Acceptance: the rule behind `agent_access` is written down in
`sl-wire::LoginSuccess`'s field docs as a measurement or a cited source rather
than as two hypotheses, and `sl-fake-grid` derives the field from whichever
input turns out to govern it.
