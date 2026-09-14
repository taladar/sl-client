---
id: viewer-setting-write-rejected-wrong-type
title: A settings write is silently rejected for the wrong integer type
topic: viewer
status: bugs
origin: seen in the log of an OpenSim run while live-verifying
  [[viewer-audit-env-overrides-preferences]] (2026-09-14)
refs: [viewer-audit-env-overrides-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

A live OpenSim session logged, 29 minutes into the run and a few seconds before
exit:

```text
WARN sl_viewer_settings: settings: could not set RenderAvatarComplexityMode:
setting "RenderAvatarComplexityMode" has type U32, not I32
```

The write was **dropped** — `ViewerSettings::set` logs and swallows the error so
a bad write cannot abort a frame — so whatever the user (or a system) asked for
did not happen and nothing in the UI said so.

## What was ruled out

Every code path that names `RenderAvatarComplexityMode` writes it correctly:

- `preferences_graphics::complexity_mode_options` and the phototools row that
  reuses it build `SettingValue::U32` from `ComplexityMode::stored()`;
- the binding layer's slider coercion (`slider_value_as_setting`) and the
  debug-settings editor (`assemble_field_value`) both key the constructed
  variant off the **declared** `SettingKind`, so neither can produce an `I32`
  for a `U32` setting;
- no other `SettingValue::I32` construction in the workspace targets this
  setting.

## The lead

`sl-settings/src/toml_format.rs:309` — a stored value whose setting is **not
registered yet** at load time is parsed *untyped*, and a bare TOML integer
becomes `SettingValue::I32` unconditionally (the `FutureSetting` case the
round-trip test covers). `RenderAvatarComplexityMode` is registered by
`sl-viewer-world-avatar`, so any load that runs before that registration would
hold the value as an `I32` that no later `U32` write or re-apply can match.

That would make the defect a **registration-order** one, not a call-site one,
and it would affect every `U32` setting whose owning module registers late — not
just this one.

## Next step

Establish which write produced it: run with `RUST_BACKTRACE` unavailable here,
so instead add the setting name to the `warn!` call site's context (scope and
caller) or reproduce by loading a `viewer-settings.toml` holding
`RenderAvatarComplexityMode` before the avatar-complexity registration and
writing it afterwards. If the untyped-load lead holds, the fix is for the
untyped parse to be **retyped** when the declaration arrives (or for the load to
be deferred until every module has registered), and a unit test in `sl-settings`
should pin it.
