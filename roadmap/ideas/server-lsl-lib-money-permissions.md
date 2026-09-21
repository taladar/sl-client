---
id: server-lsl-lib-money-permissions
title: Library tranche — script permissions, money and payment
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, protocol-sim-script-messages]
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
Missing: the `MoneyTransferRequest` decode and the
`send_money_balance_reply` sender, both in
[[protocol-sim-script-messages]], and the routing of a payment made to
an object into its scripts' `money` event.

A scripted vendor is the natural acceptance fixture and is exactly the
kind of content the fake grid exists to test offline.

Acceptance: a vendor prim sets a pay price, is paid from the viewer,
raises `money` with the right amount, and the payer's balance and the
economy event stream agree; a script calling `llGiveMoney` without
`PERMISSION_DEBIT` fails and says so on `DEBUG_CHANNEL`; and a grant is
shown to lapse on a state change.
