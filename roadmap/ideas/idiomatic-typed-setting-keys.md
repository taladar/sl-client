---
id: idiomatic-typed-setting-keys
title: Typed setting keys and enum-valued settings
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

Settings are read by `&str` name with the type chosen at each read
(`get_bool` / `get_u32` / `get_i32` …), each returning `Result` that ~80
consumers turn into a default with `unwrap_or`; `SettingBinding::global /
account(name)` repeats the name and picks the scope separately from the
registration. So an unregistered name, a type mismatch or a default that
disagrees with the registration reads as the default with no error. None
mismatches today (checked for ~150 resolvable registrations).

Enum-valued settings keep their vocabulary as raw strings / integers on both
sides: `RadarAlertOutput` `"chat"` / `"toast"`; `MediaSoundsEarLocation`
`U32(0|1)` written by preferences and read as `stored == 1` (the `EarMode`
enum exists, unconnected); double-click actions with different codes for the
world and the minimap; chat font size; shadow detail; `RenderTonemapType`;
maturity `"PG"` / `"M"` / `"A"` with four helper functions beside
`sl_proto::Maturity`; the UI-language combo values duplicated with
`UiLocale::from_setting`; snapshot / panorama formats persisted as list
indices (reordering `FORMATS` changes saved preferences); radar age alert
`-1` = off; map / minimap layer toggles whose action, setting name and default
are repeated 5-7 times per layer; setting-name literals duplicating existing
`SETTING_*` constants (`scene_dump.rs`, the colour-row label map).

## How

A `Setting<T> { name, scope, section, default }` constant owned by the feature,
used by registration, preferences binding and reads (`get(key) -> T`); enum
settings implement a `SettingEnum` (`from_setting` / `to_setting`, as
`WorldDoubleClickAction` / `ComplexityMode` already half do) so preferences
combos and features share the type. `MapLayer` / `MinimapLayer` enums yielding
setting, default, action and condition.
