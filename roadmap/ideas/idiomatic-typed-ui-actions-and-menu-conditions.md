---
id: idiomatic-typed-ui-actions-and-menu-conditions
title: Typed actions and conditions for menus, pies and UiAction dispatch
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

`UiAction { element, action: &'static str }`, `MenuCommand.action` / its
condition fields, `PieAction.action` / `when`, `MenuConditions` /
`PieConditions(Vec<&'static str>)` and the accelerator strings are all
strings. Handlers `match` literals and end in `_ => {}`: the menu bar
(~49 arms plus the `presence` / `auto_reject` "claimers" that take
`action: &str` and return "was it mine"), the four context-menu pies, the
inventory context and gear menus, minimap, world map, radar, nearby media,
media controls, web floater, blocked list, parcel audio, My Environments
(which turns its typed `MyEnvironmentsButton` back into `"rename"` /
`"delete"` to string-match it again). Accessible names are a third copy keyed
by the same strings (unknown → `""`). Load-bearing naming conventions:
`starts_with("toggle-")`, `"attach-point-N"` parsed back with `strip_prefix`,
`MARK_COLORS` keyed by action string across minimap and radar,
`permanent = action == "derender-blacklist"`.

Today every producer has a handler (the auditors diffed both sides), but
"greyed as `UNIMPLEMENTED`" and "has a handler" are independent facts, radar's
`every_menu_action_has_a_handler` test checks a hand-kept list, and a
misspelled condition key greys an entry forever. `Accelerator::parse` failing
makes a drawn shortcut do nothing (only the menu bar pins its accelerators).

## How

One action enum per menu / element (`#[derive]` `as_str` / `FromStr` for the
automation and persisted names), declared in the menu tables and matched
exhaustively — or make `MenuDef<A>` / `PieMenuDef<A>` / `UiAction<A>` generic.
Conditions as a per-menu enum plus `When::{Always, If(C), Unimplemented}`
(folding the `"never"` / `"unimplemented"` sentinels). `MenuCommand::accel`
as a `const fn` that parses at compile time into `Accelerator` with a
`Modifiers` bitflags. `TopMenuWindow` enum yielding a window's action,
floater id and condition key. Large: split per crate when worked.
