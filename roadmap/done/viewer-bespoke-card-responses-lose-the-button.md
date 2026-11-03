---
id: viewer-bespoke-card-responses-lose-the-button
title: Bespoke notification cards resolve every click as "no choice"
topic: viewer
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  viewer-notification-button-ids-untyped]
---

Context: [context/viewer.md](../context/viewer.md).

## Done (2026-11-03)

Every bespoke card now records the button that was clicked; only a close ×,
an expiry or a dismissal records no choice.

- **One answer set per card**, declared beside the catalogue's button sets in
  `sl-viewer-notifications/src/forms.rs` with the same macro (split so the
  enum half, `answer_enums!`, does not join the catalogue's form check):
  `ScriptPermissionAnswer`, `ExperiencePermissionAnswer`, `LoadUrlAnswer`,
  `OfferAnswer`, `GroupNoticeAnswer` and the script dialog's
  `ScriptDialogAction`. A card offers its buttons with
  `ToastButton::answer(Answer::X, label)` and resolves a click with
  `ResolveNotification::chose(toast, Answer::X)`, so the button shown and the
  answer recorded are one value. `ResolveNotification::dismissed` and
  `answered(toast, Option<A>)` (one observer for a button and the ×) cover
  the rest; the struct is no longer `Copy`.
- **Runtime names**: `ResolveNotification::button`,
  `NotificationResponse::button` and `NotificationRecord::response` are now
  a `ButtonName` (`Cow<'static, str>`), so a script dialog's own button text
  is recorded as it is.
- **Script dialog ambiguity**: Block and Ignore take the reference form's
  names, `Client_Side_Mute` / `Client_Side_Ignore`, so a script's own
  "Ignore" button no longer reads as the card's. A script button is named by
  its label unless that name is taken — by an earlier button with the same
  label, or by one of the card's actions — when its 1-based position is
  appended (`Yes#2`), so every button on a card has a name of its own. Test ids
  (`script-dialog-button:<label>`) are unchanged.
- **Tests**: the script dialog, script permission, experience permission and
  load-URL cards are built in a bare app with the real host resolve and each
  button clicked (`notification_host::{resolve_test_app, click_named}`); the
  five offer kinds are clicked against the resolve they write (their
  `spawn_*_card` now return a `BuiltOffer` the ingest adopts, so a test can
  build one without the channel). The group notice, already right (it passed
  `Some("OK")`), only changed types; its constructors are covered by the host
  tests.

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
