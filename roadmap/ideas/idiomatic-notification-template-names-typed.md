---
id: idiomatic-notification-template-names-typed
title: Compile-time-checked notification template names and bespoke card templates
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  viewer-notification-button-ids-untyped,
  viewer-bespoke-card-responses-lose-the-button]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`ShowNotification::new(&'static str)` is checked only at runtime (an unknown
name is logged and dropped by the host) across hundreds of raise sites, and
`.arg("KEY", …)` keys are never checked against the message's `[KEY]`
placeholders (a wrong key shows a literal `[KEY]`). All match today. Nine
bespoke card "templates" (`ScriptDialog`, `ScriptQuestion`,
`ScriptQuestionCaution`, `LoadWebPage`, `ScriptQuestionExperience`,
`GroupNotice`, `UserGiveItem`, `TeleportOffered`, `OfferFriendship`) share the
same `&'static str` slot without being catalogue entries — `probes.rs`'
`template(record.template)` quietly finds nothing for them, and the e2e tests
re-type them as literals.

## How

The `const` catalogue lookup behind `TemplateRef` already exists: a
`TemplateName::new("X")` const constructor (or `TemplateRef<NoForm>` for
toast-only templates) taken by `ShowNotification::new`; `enum ToastTemplate {
Catalogue(TemplateName), Bespoke(BespokeCard) }` for the record / response /
readout; optionally a per-template placeholder list checked by a catalogue
test against the English bundle. Mechanical over many files.
