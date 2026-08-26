---
id: viewer-audit-preferences-restart-note
title: There is no restart-note idiom, so 'restart required' is baked into labels
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 3
refs: [idiomatic-audit-dead-forward-api, viewer-audit-notifications-crate-split]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-preferences/src/preferences_graphics.rs:40` states it plainly: "the
shell has no restart-note idiom". So the note is baked into each label instead
of being a row attribute — and the two crates phrase it differently:
`preferences_network_cache.rs`'s FTL `preferences-row-*` strings say "(restart
required)" while `preferences_graphics.rs` says "(takes effect after restart)".

The catalogue already carries the machinery: six restart/deferred-setting
notification templates exist with FTL strings and are **never raised by any
code** (`sl-viewer-notifications/src/lib.rs:11336`, `:11406`, `:12008`, plus
`ChangeSkin`, `ChangeLanguage`, `CacheWillBeMoved`).

Scope: add a `restart_required` attribute to the preference-row builders that
renders one consistent note and raises the matching catalogue template, then
strip the phrase from the individual labels. That also retires six pieces of
dead forward-looking API — see [[idiomatic-audit-dead-forward-api]].
