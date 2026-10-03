---
id: viewer-notification-button-ids-untyped
title: Notification button ids are bare strings, so a wrong one fails silently
topic: viewer
status: done
origin: live check of viewer-media-prim-browser's Remove (2026-09-27)
refs: [viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

## Done (2026-10-03)

Button ids are typed, and a consumer naming the wrong set no longer compiles.

- **Button-set enums** (`sl-viewer-notifications/src/forms.rs`): one enum per
  distinct set of button names — 34 of them for the 97 forms, e.g.
  `OkCancel` (shared by 46 forms, `YES_NO_FORM` among them), `YesNoCancel`,
  `YesNo`, `OkOnly`, `CreateCancel`, `OfferCancel`, …. A `form_answers!`
  macro gives each a `const fn name` (the reference's string, misspellings
  included) and a `FormAnswer` impl; every form table now takes its button
  names from the enum (`name: OkCancel::Ok.name()`) instead of a literal, so
  the persisted / wire name is produced in one place and the open-notification
  file stays readable unchanged.
- **`TemplateRef<A>`**: a consumer declares the template it handles as
  `const DELETE_MEDIA: TemplateRef<OkCancel> = TemplateRef::new("DeleteMedia")`.
  `new` is a `const fn` that scans the catalogue and asserts the template
  exists and its form's names are exactly `A::NAMES`, so a typo'd template
  name or the original bug (`TemplateRef<YesNo>` for `DeleteMedia`) is a
  compile error naming the item. Two `compile_fail` doctests pin both cases.
- **`NotificationResponse::answer(template) -> Option<A>`** /
  **`is_for(template)`**: `None` for another template's response or a
  dismissal; a button the form does not have (only possible from a host bug)
  is logged as an error, not read silently as "no answer".
- **Every consumer ported** (13 files across edit, people, environment,
  places, asset-editors, preferences): handlers match `OkCancel::Ok`,
  `YesNoCancel::Yes`, `CreateCancel::Create`, …; raises use
  `ShowNotification::new(TEMPLATE.name())`; the per-file `*_BUTTON: &str`
  constants are gone. Two tests that only checked by hand that a template
  carried the button names routed on (`settings_editor`, `asset_editor`) were
  dropped: `TemplateRef::new` now makes that check at compile time.
- A catalogue test holds every form to unique names and exactly one
  button-set enum, so every template can be named by a `TemplateRef`.

Porting found no further wrong comparisons: the remaining ones already used
the right names (`DeleteItems`' buttons really are `Yes` / `No`).

Not in scope, deliberately: bespoke cards that build their own
`ToastButtons` (group notice, script dialog, offers) — their names are not
catalogue forms — and the automation / e2e layer, which answers a live card
by button name across a process boundary.

## Observation

The Texture tab's media **Remove** did nothing: the `DeleteMedia` question
was answered Yes and no removal was sent. The handler waited for
`response.button == Some("Yes")`, but `DeleteMedia` uses `YES_NO_FORM`,
whose Yes button is *named* `"OK"` (only its `label_key` says Yes). The string
compiled, matched nothing, and the answer was dropped with no error or log.

## Cause

A `NotificationButton::name` is a `&'static str`, and
`NotificationResponse::button` hands it back as `Option<&'static str>`.
Nothing ties the literal a consumer compares against to the buttons its
template's form actually has, so a typo, a form change or a wrong guess about a
form's naming is never caught — at compile time or at run time. It is not a
translation problem (the name is a stable id; only the label is translated),
it is a typing one.

About 27 such literal comparisons sit in ~15 files today (`rg
'Some\("(OK|Cancel|Yes|No|…)"\)'`), e.g. `edit_land`'s `LandDivideWarning`
handling and `edit_media`'s two confirmations.

## Fix

Make the button id a type the compiler checks against the form:

- give each form (or each button set) an enum of its buttons —
  `YesNo::{Yes, No}` for `YES_NO_FORM`, `OkCancel::{Ok, Cancel}`, … — and have
  the form table built from it, so the wire / persisted name stays the
  reference's string but is produced in one place;
- make `NotificationResponse` carry that typed id (or a typed accessor such as
  `response.answer::<YesNo>()` that returns `None` for a response to a
  different form), so a handler matches on `YesNo::Yes` and a mismatched form
  is a compile error or an explicit `None`, never a silent non-match;
- port every consumer, and keep the persisted open-notification file readable
  (it stores names).

Done when no consumer compares a button against a string literal.
