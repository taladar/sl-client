---
id: viewer-setting-write-rejected-wrong-type
title: A settings write is silently rejected for the wrong integer type
topic: viewer
status: done
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

## The cause (confirmed)

The lead held. `ViewerSettings::load_with(REGISTRARS)` runs **before the Bevy
app exists** (`run_viewer` needs the stored start location to build the login
request), so the global scope is loaded against `REGISTRARS` alone —
and `avatar_complexity` is **not** in that list: it declares its three settings
from a `Startup` system (`register_complexity_settings`). Ten other modules do
the same (`people`, `offers_invites`, `group_notice`, `group_profile`,
`status_bar`, `media_prim`, `volume_panel`, `derender`, `world_sounds`,
`parcel_audio`), so this was never one setting's problem.

The user's own `viewer-settings.toml` carried the evidence:
`RenderAvatarComplexityMode = 0` under `[render]` with its declared comment
(written at exit, when the setting *was* declared), read back at the next start
as an `I32` because at *load* time it was not. From there:

- `get_u32` could never match it, so `sync_complexity_settings` fell to its
  `unwrap_or(0)` — **the saved mode silently reverted to the default on every
  run**, which is the user-visible half and was invisible only because the saved
  value happened to be `0`;
- the preferences Cancel-revert snapshots `get_override` and writes it back, so
  it wrote the `I32` it had been handed — the logged rejection.

## The fix

`SettingsStore` now reconciles on **declaration** rather than requiring an
order no feature can be asked to guarantee: `retype_untyped_overrides` re-reads
any override of a name at the type the arriving declaration fixes, dropping it
only if it genuinely cannot be read (the same outcome a declared load gives a
value that no longer fits). `SettingValue::retyped_as` performs exactly the
conversions the TOML reader would have made from the same literals — an integer
literal as a signed/unsigned integer or a float, an all-integer array as a float
array, and the three- and four-element array types interchangeably within their
length.

Two smaller things came with it:

- `toml_format::infer` dropped an undeclared integer past `i32::MAX` outright;
  it now keeps one as a `U32`, so a large unsigned setting declared late is
  recoverable rather than lost at load.
- the rejected-write `warn!` names the scope, so a future wrong-typed write says
  where it came from.

Three tests in `sl-settings` pin it (late declaration reclaims its value,
including past `i32::MAX` and for the array types; an unreadable value is
dropped; the account scope is reached too).

Live-verified on the local OpenSim with a scratch config holding
`RenderAvatarComplexityMode = 2`: the Preferences ▸ Graphics combo read the
saved option, the applier logged `mode: OnlyShowFriends` (it would have logged
`ByComplexity` before), and closing Preferences without OK produced no
rejection.
