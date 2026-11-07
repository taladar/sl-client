---
id: server-lsl-forced-delays-per-flavour
title: Charge each grid flavour's measured forced delays and throttles, not the keyword file's
topic: server
status: done
origin: gridspec-lsl-throttles (2026-10-28)
blocked_by: [gridspec-lsl-throttles, server-lsl-lib-comms,
  server-lsl-lib-http-url, server-lsl-lib-task-inventory,
  server-lsl-lib-math-rotations, server-lsl-lib-land-region-env,
  server-lsl-lib-prim-state, server-fake-grid-script-engine-wiring]
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

## Blocked until its calls exist (2026-10-30)

Picked up and put back: the runtime implemented none of the calls this task
re-times (`llTextBox`, `llLoadURL`, `llGiveInventory`, `llModPow`,
`llSetRemoteScriptAccessPin`, `llParcelMediaQuery`, `llMapDestination`,
`llSay`/`llShout`, `llHTTPRequest`), so a per-flavour table, an outcome hook
and both throttles would have had no consumer, and the acceptance needs the
fake grid to run scripts. It waits on the tranches that add those calls and
on [[server-fake-grid-script-engine-wiring]].

## From server-lsl-lib-prim-state (2026-11-04)

The prim functions charge the keyword file's delays (`llGetPrimitiveParams`
0.2 s, `llSetPrimitiveParams` 0.2 s, the media calls 1 s). OpenSim's refusals of
a rule sleep a second of their own, written into `prim_params::args` and
`prim_params::set`, not the table.

## From server-lsl-lib-task-inventory (2026-11-07)

`llGiveInventory` to an avatar already sleeps each flavour's measured 2 s or
3 s (`library::rezzing::offer_delay`); move it into this task's table when the
table lands. The other delays of the tranche are still the keyword file's.

## Done (2026-11-07)

- **The table is `vm::delays`.** `delays::forced(flavour, descriptor)` is the
  keyword file's `sleep` where the grid agrees and the grid's own where not:
  Second Life's `llTextBox` 0; OpenSim's `llSetRemoteScriptAccessPin`,
  `llGetPrimitiveParams` and `llGetSimulatorHostname` 0, `llRequestUsername`
  and `llRequestUserKey` 0.1, `llModPow`, `llEjectFromLand`, `llCreateLink` and
  the three link-media calls 1.0, `llGiveInventory` 3.0, `llLoadURL` 10.0,
  `llSetPrimURL` 2.0 — every `ScriptSleep` of `LSL_Api.cs` / `OSSL_Api.cs`
  compared with the keyword file, not only the probe's rows. The link-media
  calls' second also makes XEngine's sleep-first path run for them; it never
  did with the keyword file's 0.
- **`ScriptDelayFactor`** is `EngineConfig::script_delay_factor` (1.0); OpenSim
  scales a forced delay by it, cuts it to whole milliseconds and drops one
  under 10 ms (`delays::scaled`, `LSL_Api.ScriptSleep`). `llSleep` and an
  error's second are not scaled.
- **Outcome-dependent delays**: `ScriptCtx::delay(seconds)` replaces the
  table's value for one call. OpenSim's early returns take none
  (`llParcelMediaQuery` / `llParcelMediaCommandList` / `llSetParcelMusicURL`
  off the owner's land, `llMapDestination` outside a touch, `llRequestUsername`
  / `llRequestUserKey` of an avatar in the region, a rez that rezzes nothing,
  `llTeleportAgentHome` of an avatar not there, `llPreloadSound` of a sound not
  found, a link not made, a refused `llDialog` / `llTextBox` /
  `llGiveInventory` / `llRemoteLoadScriptPin`, an unknown `llRequestAgentData`
  fact, which also answers `""` now); `llGiveInventoryList`'s refusals and
  gifts to prims 0.1, `llLoadURL`'s refusals and `llHTTPRequest`'s bad flag
  0.2; `llGiveInventory` to an avatar 2 s / 3 s; `osGetRegionMapTexture` and
  `osPreloadSound` their second.
- **The error's second**: `ScriptCtx::refuse` (and `refuse_to_owner`) on
  OpenSim is `LSL_Api.Error` and charges the second itself, each refusal its
  own; `ScriptCtx::error_pause` is for `OSSLShoutError` and Second Life's few
  complaints that cost one. `PureCtx::sleep` is gone; its `refuse` charges the
  same. The scattered `ctx.sleep(1.0)` beside refusals are folded in, and
  OpenSim refusals that had charged nothing (`llDialog`, `llTextBox`,
  `llLoadURL`, `llInstantMessage`, `llHTTPRequest`'s URL checks, the link
  permission refusals, …) now charge their second.
- **Double charges fixed**: `llRequestAgentData` (0.2 s), `llTeleportAgentHome`
  (10 s) and `llParcelMediaCommandList` (4 s) each added a `ctx.sleep` of the
  keyword file's own value on top of it.
- **OpenSim's channel-0 chat throttle**: `delays::SayThrottle` per script
  (`m_SayShoutCount`, `m_lastSayShoutCheck`, kept through a reset): the 12th and
  24th of 25 lines sleep 2 s, and past the count every `llSay` / `llShout` on
  any channel sleeps until a channel-0 line opens a new window. XEngine sleeps
  *before* the line goes out: `ScriptCtx::delay_first` makes the VM push the
  arguments back and make the call again on waking (`DECIDES_SLEEP_FIRST`);
  YEngine says it, then sleeps.
- **Second Life's HTTP throttle corrected** (`vm::http::Throttle`): refused
  requests count in the 25-in-20 s window, and once one is refused the prim is
  shut until 20 s pass without a request; the refused request exactly 20 s back
  still counts, so 24 go through then. A plain window let one through after
  10 s of quiet. The book's "25 again" misread the probe (its `refill` counts
  the first request); it now says 24.
- **Held to the grids**: `vm/tests/throttles.rs` replays `throttle/delays`
  (each call's delay within a frame and a half of the grid's least),
  `throttle/chat` and `throttle/http-window` on all three flavours, and checks
  the delay factor; `sl-fake-grid/tests/scripts.rs`
  `a_script_is_held_as_*_holds_it` runs `llModPow`, `llTextBox` and a channel-0
  flood on the fake grid end to end per flavour.

## Done with test-lsl-differential-opensim (2026-11-07)

- The acceptance as written: [[test-lsl-differential-opensim]] ran the fake
  flavours through the probe runner, and `throttle/delays`, `throttle/chat`,
  `throttle/http-window` and `throttle/rez` agree in `compare-probes` between
  each grid and its fake flavour.
- Parcel access lists: a `Parcel` carries its pass and ban lists
  (`AccessEntry`, an agent and an expiry, 0 for never; the fake grid's from
  its world's access lists), so OpenSim's `llAddToLandPassList` /
  `llAddToLandBanList` keep an entry that never expires or outlasts the new
  one and return before their 0.1 s
  (`opensim_keeps_a_longer_ban_and_takes_no_sleep_for_it`).

## Left open

- `osNpcSay`, `osNpcSayTo` and `osNpcShout` pass the same throttle in OpenSim
  (`ThrottleSay`): [[server-lsl-lib-ossl-avatars]].
