---
id: viewer-notification-button-ids-untyped
title: Notification button ids are bare strings, so a wrong one fails silently
topic: viewer
status: bugs
origin: live check of viewer-media-prim-browser's Remove (2026-09-27)
refs: [viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

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
