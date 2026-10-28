---
id: server-lsl-forced-delays-per-flavour
title: Charge each grid flavour's measured forced delays and throttles, not the keyword file's
topic: server
status: ready
origin: gridspec-lsl-throttles (2026-10-28)
blocked_by: [gridspec-lsl-throttles]
refs: [server-lsl-vm-execution, server-lsl-lib-comms, server-lsl-lib-http-url,
  server-lsl-lib-task-inventory, server-lsl-lib-detection-sensors,
  server-lsl-lib-email-xmlrpc, gridspec-infra-fake-flavour-conformance]
---

Context: [context/lsl.md](../context/lsl.md).

`sl-lsl-runtime` charges every library call the `sleep` of Linden Lab's
keyword file (`Builtin::sleep`, applied in `vm/instance.rs` after the call),
one value for every grid and every outcome. The measurements in
`book/src/gridspec/lsl.md` § Forced delays and throttles say that is wrong in
both directions, and that the delay depends on the grid and, on OpenSim, on
how the call went.

## What to build

- **A forced-delay table per flavour** that overrides the keyword file's
  value where a grid measured otherwise. Second Life: `llTextBox` 0 (not 1.0),
  `llGiveInventory` to an avatar 2 s (not 0). OpenSim:
  `llSetRemoteScriptAccessPin` 0, `llModPow` 1.0, `llLoadURL` 10.0,
  `llGiveInventory` 3 s, and every sleep scaled by the region's
  `ScriptDelayFactor`.
- **A delay that depends on the outcome.** OpenSim's `llParcelMediaQuery`
  without the land's rights and `llMapDestination` outside a touch return
  before their sleep, and **every OpenSim library error sleeps the script one
  second** (`LSL_Api.Error`), on top of or instead of the call's own delay. A
  call's implementation has to be able to say which.
- **OpenSim's channel-0 chat throttle**: more than eleven `llSay` / `llShout`
  lines on channel 0 within a second sleeps the script 2 s before the twelfth
  (the count resets a second after the last check). Second Life has none, and
  neither has a throttle on other channels.
- **The HTTP throttle per flavour**: Second Life 25 requests in 20 s,
  counting refused ones, so retrying keeps the script shut out until 20 s of
  silence; OpenSim a bucket of 3 that fills by one a second.
- **What is not throttled** stays unthrottled: 300 rezzes in a row, ten
  seconds of raycasts (1090 a second on Second Life), 600 region-says.

The Second Life numbers are frame-bound (a delay reads up to 0.022 s over
its value there); a fake grid charges the value itself.

## Acceptance

A probe run of `throttle/delays`, `throttle/chat` and `throttle/http-window`
against each fake flavour, once the fake grid runs scripts, agrees with that
grid's committed result file under `sl-conformance compare-probes`
([[gridspec-infra-fake-flavour-conformance]]).

## From server-lsl-runtime-errors (2026-10-28)

`llGetScriptState` of a missing script already charges its second on every
flavour (`library/control.rs`, `ctx.sleep(1.0)` beside `ctx.refuse`), since
aditi measured the same second there. When the outcome-dependent delay lands,
fold it into that mechanism rather than keep a second path.
