---
id: repl-c3
title: Formatters
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase C — shared library `sl-repl`
---

Context: [context/repl.md](../context/repl.md).

**C3. Formatters.** `format.rs`:
`format_event(&Event,&dyn ReplContext)` and
`format_command(&Command,&dyn ReplContext)` render `<name><fields>` where the
name is the snake-case event name / the registry command spelling (`im`,
not `InstantMessage`) and the fields are the variant's `Debug` form with every
binding-backed literal symbolized through the context — a token scanner offers
each bare `[0-9A-Za-z-]` run (UUIDs, integers, names) and each whole quoted
string (cap URLs, var values) to `ReplContext::symbolize`, so `$self` /
`$region` / `$cap:Name` replace the volatile ids for clean cross-run diffs.
`format_diagnostic(&Diagnostic)` renders **literally** (one header line per
variant + a marked hexdump for `DecodeFailed`); `hexdump(bytes, mark)` is a
classic offset/hex/ASCII dump that brackets the byte at `failed_offset`
(`[ab]` vs ` ab `, equal-width so rows stay aligned) and notes an
at/past-end mark on a trailing line. Compile-time exhaustiveness: the two enum
renderers match every variant via `event_name`/`command_name` (no `_` arm —
191 commands + 113 events), so a new variant fails to compile until named. 6
unit tests; clippy-clean under the restriction lints.
