---
id: viewer-bespoke-card-responses-lose-the-button
title: Bespoke notification cards resolve every click as "no choice"
topic: viewer
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  viewer-notification-button-ids-untyped]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

`ResolveNotification.button: Option<&'static str>` is documented as `None`
for an expiry or external dismiss, and feeds `NotificationManager::
record_response` and the `NotificationResponse`. The bespoke cards send
`button: None` from every click observer, positive buttons included:
`script_dialog.rs`, `script_permission.rs`, `experience_permission.rs`,
`load_url.rs` (sl-viewer-notices) and `offers_invites.rs` (Accept, Decline,
Block, Teleport, Join — sl-viewer-people). Only `group_notice.rs` passes
`Some("OK")`. Their `ToastButton` names are not tied to the resolve at all,
and the script dialog's names are runtime strings that cannot fit
`&'static str`.

Related: `script_dialog.rs` names script buttons by their label and then
appends `Submit` / `Block` / `Ignore` under those same names, so an LSL dialog
with an "Ignore" button (common) or duplicate labels (allowed) gives an
ambiguous `OfferedButton` readout.

## Effect

History and the automation readout (`sl-viewer-automation/src/probes.rs`,
`response`) say "dismissed" for every Accept / Grant / script button.

## Fix

A per-card answer type, as the catalogue forms now have (`FormAnswer`): e.g.
`enum ScriptDialogButton { Script(index), Submit, Block, Ignore }`,
`OfferAnswer { Accept, Decline, Block }`, … — or resolve by an index into the
card's `ToastButtons`. The response then names what was clicked, and the
bespoke template names join a closed enum (see
[[idiomatic-notification-template-names-typed]]). Unit-test each card's
resolve.
