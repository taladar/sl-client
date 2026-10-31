---
id: server-lsl-lib-money-permissions
title: Library tranche — script permissions, money and payment
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, protocol-sim-script-messages,
  gridspec-lsl-money-permissions]
refs: [server-lsl-lib-avatar-control, server-fake-grid-script-engine-wiring]
---

Context: [context/lsl.md](../context/lsl.md).

Two surfaces that share a gate: a script may only take an avatar's
money, controls, camera, animations or attachments after the avatar has
said yes.

**Permissions.** `llRequestPermissions`, `llGetPermissions`,
`llGetPermissionsKey`, and the
`run_time_permissions(integer perm)` event.
`SimSession::send_script_question` exists, the client answers with
`ScriptAnswerYes`, the grid already surfaces
`ServerEvent::ScriptPermissionAnswer` and keeps a grant mirror
(`SimSession::script_grant`), and `sl-proto` already classifies each
flag's client responsibility (`PermissionRole`). So the plumbing is
there end to end; what is missing is a script to be its subject and the
**enforcement**: a granted flag is checked before every gated call, a
grant is revoked on `llReleaseControls`, on `RevokePermissions`, on a
state change and when the avatar leaves the region, and
`PERMISSION_DEBIT` is *never* auto-granted.

The fake grid's policy decision, to be written down: it enforces
permissions properly (so a script that forgets to ask fails the way it
would on a real grid), and the *answer* comes from a real client — a
human in the viewer, a conformance case driving its own session, or a
scripted avatar with a grant policy
([[server-fake-grid-scripted-avatars]]). The grid does not fabricate a
grant on the script's behalf; a question nobody answers stays
unanswered, exactly as it would on a live grid, which is a state content
has to cope with and a test should be able to produce.

**Money.** `llGiveMoney`, `llTransferLindenDollars`,
`llSetPayPrice`, `llRequestPermissions(PERMISSION_DEBIT)`, the
`money(key id, integer amount)` event and
`transaction_result(key id, integer success, string data)`. The grid
already has an economy: `economy_policy` with prices and an
`EconomyEvent` stream, `RequestPayPrice` and
`send_pay_price_reply`, `ObjectBuy`, `ParcelBought`, and a balance.
`SimSession::send_money_balance_reply` exists now
(`sl-proto/src/sim_session.rs`, audit 2026-10-15). Missing: the
`MoneyTransferRequest` decode on the simulator side
([[protocol-sim-script-messages]]), and the routing of a payment made to
an object into its scripts' `money` event.

A scripted vendor is the natural acceptance fixture and is exactly the
kind of content the fake grid exists to test offline.

Acceptance: a vendor prim sets a pay price, is paid from the viewer,
raises `money` with the right amount, and the payer's balance and the
economy event stream agree; a script calling `llGiveMoney` without
`PERMISSION_DEBIT` fails and says so on `DEBUG_CHANNEL`; and a grant is
shown to lapse on a state change.

## Measured (gridspec-lsl-money-permissions, 2026-10-26)

`book/src/gridspec/lsl.md` § Script permissions, money and payment is the
model per flavour. The points the library has to get right:

- **Lapses.** A grant survives a state change, the owner leaving the
  region, attaching and a take-and-rez (`on_rez`, script not reset — XEngine
  alone resets it); it is cleared by `llResetScript`; `llReleaseControls`
  and a detach drop `TAKE_CONTROLS` only; `RevokePermissions` drops exactly
  the bits it names, of the object or of every object in the region, and
  `0xFFFFFFFF` clears the granter too. No lapse runs `run_time_permissions`.
- **Debit.** Unpermitted `llGiveMoney` returns 0 and complains on
  `DEBUG_CHANNEL` on Second Life (silent on OpenSim); `llTransferLindenDollars`
  always returns a handle and answers `transaction_result` with
  `MISSING_PERMISSION_DEBIT`, `INVALID_DESTINATION` (`NULL_KEY`, SL),
  `LINDENDOLLAR_ENTITYDOESNOTEXIST`, `INVALID_AMOUNT` (≤ 0),
  `LINDENDOLLAR_INSUFFICIENTFUNDS`, or success with `"<dest>, <amount>"`;
  the money moves as type `5009`. Second Life refuses to ask a non-owner for
  `PERMISSION_DEBIT` (`DEBUG_CHANNEL`, no question); OpenSim asks.
- **Payments.** Every script in the prim with a `money` event in its current
  state hears a payment; one is taken even when none can. Second Life
  accepts any amount above 0 while the price field shows, only a button's
  amount while it is hidden (anything else dropped silently), and none at
  all, with an alert, when every amount is hidden. The stock OpenSim module
  takes nothing.
- **Pay buttons.** `PAY_DEFAULT` buttons answer 1, 5, 10, 20 on Second Life
  and `-2` on OpenSim; missing buttons are hidden, an empty list keeps the
  defaults on Second Life; a non-integer button is hidden on Second Life and
  throws on OpenSim; a layout of zeros gets no reply on OpenSim; the layout
  survives a reset.

## Done (2026-10-31)

All six functions of the tranche are implemented in `sl-lsl-runtime`
(`library::money_permissions`), with the `run_time_permissions`, `money` and
`transaction_result` events: `llRequestPermissions`, `llGetPermissions`,
`llGetPermissionsKey`, `llGiveMoney`, `llTransferLindenDollars` and
`llSetPayPrice`; coverage is 151 implemented of 524.

- **The grant is the script's** (`vm::Permissions`): kept by a state change,
  cleared by a reset. The avatar-control bits of a wearer or a sitter are
  granted unasked; anything else is a `Question` the host puts
  (`Host::ask_permissions`), answered through `Engine::answer` as it came
  (more than was asked included; an answer nobody asked for is nothing).
  OpenSim names the asked avatar the granter at once with nothing held,
  releases on a mask of 0 or `NULL_KEY`, and refuses an absent avatar with
  `run_time_permissions(0)`; Second Life puts nothing for a mask of 0 and
  refuses, in its words, `PERMISSION_DEBIT` of a non-owner and
  `PERMISSION_CONTROL_CAMERA` of an avatar that neither wears nor sits on the
  object. A viewer's revocation is `Engine::revoke`: the bits named, the
  granter only with every bit, no event.
- **Money**: each function checks the grant, the amount and the payee in its
  flavour's order and words, and hands the host a `Payment`
  (`give_money`, `transfer_money`); the economy answers a transfer with a
  `MoneyRefusal` or not, and `transaction_result` follows the slice, its data
  `"<payee>, <amount>"` on Second Life and `"<payee>,<amount>"` on OpenSim.
  The handle is a fresh key from the region's seeded draw.
- **Pay layouts**: `llSetPayPrice` hands the host the prim's `PayPrice`, with
  Second Life's hidden missing and non-integer buttons and defaults for an
  empty list, and OpenSim's padding, string buttons read as `(integer)`, the
  `InvalidCastException` at a float, vector or rotation (two new
  `Exception`s), and a child prim ignored. `PayPrice::reply` is what a
  `RequestPayPrice` is answered, `PayPrice::takes` which payments Second Life
  takes, and `Engine::paid` raises `money`. OpenSim pads the very list the
  script passed, so its dialect's compiler writes the padding back to a list
  variable passed to the call (`Instr::PadPayButtons`).

**Proof** (`vm/tests/money.rs`): `avatar/permissions`, `money/debit`,
`money/pay-price` and `money/pay` replayed against all three transcripts in
the comms routing region, with a runner that touches, answers, pays and asks
for pay buttons as each `waiting` record asks: every `PROBE` line, every
question, pay-button answer and alert the runner noted, every
`DEBUG_CHANNEL` line, YEngine's owner report and XEngine's exception line,
and what reached the economy. One deviation is named and is not this
tranche's: `avatar/permissions`' three complaints of the unpermitted
`llTakeControls`, `llStartAnimation` and `llSetCameraParams`, functions
rigged out of the probe and left to [[server-lsl-lib-avatar-control]]. The
pay-price probe's description, which carries a mark across a reset, is
carried by the start parameter instead; `llRemoveInventory` is the region
removing the script. `money/lapse` (controls, attaching, dropping, a
teleport, a re-rez) is not replayed; its rows are a unit test, with the
seated and worn grants, the absent avatar, the transfer's handle and result,
the in-place padding and the acceptance's vendor and unpermitted
`llGiveMoney`.

**Not done here**: the viewer leg of the acceptance (a vendor paid from the
viewer, the payer's balance and the economy event stream agreeing) needs the
fake grid to run scripts — [[server-fake-grid-script-engine-wiring]], which
now lists each new `Host` method's wiring; the viewer has no pay dialog
(`viewer-money-economy-ui`).
