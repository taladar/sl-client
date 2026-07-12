---
id: repl-c1
title: Crate scaffold + parser + registry (full command coverage)
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase C — shared library `sl-repl`
---

Context: [context/repl.md](../context/repl.md).

**C1. Crate scaffold + parser + registry (full command coverage).** New
member `sl-repl` (dep `sl-proto`). `parse.rs`
(`ReplAction { Meta | Command(PendingCommand) }`, meta incl.
`Sleep/Comment/Set/Unset/Vars`), `registry.rs` (one entry per `Command`
variant, `build: fn(&Args,&dyn ReplContext)->Result<Command,_>` called at
dispatch), `args.rs`, `meta.rs`. Unit tests round-trip a line per arg-type.
Added `context.rs` with the minimal `ReplContext` trait + a `NoContext`
stub (the session-aware `SessionContext`, forward resolution, and reverse
symbolizer land with C2; build fns are written against the trait so they need
no change). All **191** `Command` variants are registered; **190** build a
command, and **`send`** (an arbitrary `Box<AnyMessage>`) returns
`ReplError::NotSupported` — it can't be expressed as a text line and points
the user at the specific commands instead. Grammar: positional + `key=value`
tokens, double-quoted strings, `<x,y,z>`/`<x,y,z,s>` vectors/rotations, hex
byte blobs, comma lists with `:`-separated records; every enum accepts a
lowercase name (underscores optional) and/or its numeric wire code.
Multi-field-`Vec` commands (`update_group_roles`, `modify_material_params`,
`set_object_media`, `send_voice_signaling`) accept a single element via
keyword fields; multi-element forms are intentionally **not** offered (decided
2026-06-26) because each per-element payload embeds free text / JSON / URLs /
SDP strings containing the `,` and `:` separators the unescaped `vec_records`
parser splits on, so a record syntax would break on realistic input — the
full `Vec` stays reachable through the typed `Session` API and the tokio/bevy
drivers. (Contrast the `set_object_extra_params` `render_material` faces+ids
lists, which tokenize cleanly and *are* exposed.) 39 unit tests; clippy-clean
under the restriction lints.
