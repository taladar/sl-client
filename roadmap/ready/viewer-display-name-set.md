---
id: viewer-display-name-set
title: Set own display name
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g3, viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

The small "Change Display Name" dialog: show the current display name, take
the new value (entered twice, as the reference does), submit it over the
display-names CAPS ([[api-g3]] — `SetDisplayName`), and surface the outcome —
success updates every name shown locally via the existing display-name cache
push; failure reports the server reason (rate limit: SL allows a change only
once per week; validation errors).

Reference (Firestorm, read-only): `llfloaterdisplayname`,
`llavatarnamecache`.

Builds on: the `api-g3` display-names CAPS pairing and the name cache.
