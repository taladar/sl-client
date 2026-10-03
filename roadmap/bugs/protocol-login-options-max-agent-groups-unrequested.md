---
id: protocol-login-options-max-agent-groups-unrequested
title: max-agent-groups is never requested at login but is gated on the option
topic: protocol
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`sl-wire/src/login.rs`' default `options` list omits `"max-agent-groups"`,
with a comment saying it is a standard top-level field that needs no option —
but `filter_options` (the model of what a grid honouring the list sends,
used by the fake grid) drops it unless requested, and the reference requests
it explicitly (`lllogininstance.cpp:189`). `LoginSuccess.max_agent_groups`
is passed on to the account.

## Effect

Either the comment or the filter is wrong. If Second Life gates it like the
filter assumes, `max_agent_groups` is always `None` on SL — the same silent
failure the comment records for `map-server-url`.

## Fix

Check on aditi whether the field arrives without the option; then request it
(as the reference does) and make the filter and comment agree. Longer term a
`LoginOption` enum with each consumed field tied to its option (see
[[idiomatic-protocol-string-vocabularies]]).
