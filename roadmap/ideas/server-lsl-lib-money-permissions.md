---
id: server-lsl-lib-money-permissions
title: Library tranche — script permissions, money and payment
topic: server
status: ideas
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
