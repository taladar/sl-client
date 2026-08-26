---
id: repl-audit-format-registry-parity
title: 15 commands the REPL formatter can print cannot be parsed back
topic: repl
status: bugs
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/repl.md](../context/repl.md).

`sl-repl/src/format.rs:548-901` (`command_name`, a 350-arm match with no `_`
arm) and `sl-repl/src/registry.rs:1794-5712` (`all_specs`, 337 spec literals)
are two hand-maintained tables that must agree, and **nothing asserts they do**.

They already disagree: `resend_cached_objects`, `save_inventory_asset`,
`set_estate_info`, `set_render_materials`, `deed_objects_to_group`,
`upload_script` and nine more have exactly one `format.rs` arm and **zero**
`registry.rs` specs. Conversely `upload_script_agent` and `upload_script_task`
(`registry.rs:5071`, `:5091`) both render as the ambiguous, unparsable
`upload_script`.

Consequence: a recorded transcript containing any of those commands cannot be
replayed.

Scope: a test that walks both tables and asserts a bijection, then fix the 15+2
divergences. Two related gaps in the same pair of files:

- `CommandSpec::usage` (`registry.rs:127`, `:136`) is **write-only** — its only
  reader is the `Debug` impl, and there is no `help` / `?` meta command
  (`MetaCommand` at `meta.rs:19-37` is only `Comment` / `Sleep` / `Set` /
  `Unset` / `Vars`). 337 usage strings with nothing checking them against the
  build closures, and they have already drifted: `:2790` advertises
  `<duration_secs>` while `:2795` reads `args.req_parse(ctx, "duration", 3,
  "i32")`;
- roughly **241 of the 337 build closures never run in any test** (spot-verified
  zero for `set_object_name`, `update_parcel`, `viewer_effect`, `logout`,
  `mute`); `chat_log_args.rs` and `meta.rs` have no test module at all.

Argument parsing in the same crate is also inconsistent and worth a pass: three
copies of the `<a,b,c>` tuple parser (`args.rs:124` f32x3, `args.rs:151` f32x4,
`registry.rs:254` f64x3); **two incompatible colour grammars in one file**
(`registry.rs:942` 8 hex digits vs `:1538` comma-separated `u8`, same concept,
same default, one call site each); three ad-hoc boolean parsers (`:839`, `:850`,
`:861`) that drop the `on`/`off` spellings `args.rs:198` accepts; and 111
undocumented positional indices >= 100 used as keyword-only sentinels, explained
nowhere.
