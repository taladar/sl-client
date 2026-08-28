---
id: repl-audit-format-registry-parity
title: 15 commands the REPL formatter can print cannot be parsed back
topic: repl
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/repl.md](../context/repl.md).

`sl-repl/src/format.rs` (`command_name`, a 350-arm match with no `_` arm) and
`sl-repl/src/registry.rs` (`all_specs`) are two hand-maintained tables that must
agree, and nothing asserted they did. They had drifted: fifteen commands had a
`format.rs` arm and **zero** `registry.rs` specs, and `upload_script_agent` /
`upload_script_task` both rendered as the ambiguous, unparsable `upload_script`.
A recorded transcript naming any of them could not be replayed.

**Fixed.**

- `sl-repl/tests/table_parity.rs` asserts the bijection in both directions. The
  registry side is read at runtime through `Registry::specs`; `command_name` has
  no runtime enumeration (it maps a value to a name, and there is no way to
  conjure one of every `Command` variant), so it is scanned out of the source
  text — with the extraction itself checked against the `Command::` arm count so
  a reformatting cannot quietly shrink the comparison. All four drift classes
  were verified to fail the tests before the fixes landed.
- The sixteen missing specs were added (`query_inventory_folders`,
  `accept_group_invitation`, `decline_group_invitation`,
  `resend_cached_objects`, `deed_objects_to_group`,
  `copy_inventory_from_notecard`, `set_region_debug`, `set_region_terrain`,
  `set_estate_info`, `request_region_terrain_download`,
  `request_region_terrain_upload`, `save_inventory_asset`,
  `set_render_materials`, `set_diagnostics`, `set_chat_log_config`), and
  `command_name` now discriminates `UploadScript` by its `location` so each of
  the two entries is named for itself.

**`CommandSpec::usage` has a reader.** A `help` / `?` meta command
(`MetaCommand::Help`, `Registry::help`, `sl_repl::help_lines`) lists every
command or one command's usage, wired identically into both binaries. A test
asserts every usage names every field its build closure reads — which caught the
`<duration_secs>` / `"duration"` drift the audit named, plus 40 more usage
strings that described a value's shape without naming the keyword it sets.

**Build-closure coverage.** A generator derives a plausible argument line from
each spec's own usage: 300 of the 352 build functions now run to completion in
the test suite (up from the ~96 that had any test at all), each asserted to be
named by the formatter as its own spec, and every failure asserted to be an
*argument* error rather than an internal one. `chat_log_args.rs` and `meta.rs`
gained the test modules they lacked.

**Argument-grammar consolidation.**

- One `args::parse_components::<T, N>` replaces the three copies of the
  `<a,b,c>` bracketed-tuple parser (`f32x3`, `f32x4`, `f64x3`).
- One colour grammar: `color_or_white` now accepts **both** the 8-hex-digit and
  the comma-separated `r,g,b[,a]` spellings, so the two incompatible parsers
  collapse without breaking either existing call site.
- One `parse_named_bool` behind the three labelled-boolean enum parsers, which
  now fall back to `args::parse_bool` — so `on`/`off`, missing from all three,
  work everywhere. (Their `always_run` / `detach_all` arms were dead: `norm`
  strips underscores before the match.)
- The keyword-only positional convention is documented as `args::KEYWORD_ONLY`
  and made real: `Args::raw` skips the positional lookup at or above it, instead
  of relying on no line ever carrying a hundred positional tokens.
