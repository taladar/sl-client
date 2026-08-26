---
id: idiomatic-audit-dead-forward-api
title: Decoded-but-never-read fields and write-only state across the workspace
topic: idiomatic
status: ready
origin: static code audit (2026-08-26)
points: 3
refs: [repl-audit-format-registry-parity, viewer-audit-preferences-restart-note]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

The project's rule is: make a faithful-but-unused field load-bearing, or drop
it. A sweep found these, each with a doc comment that justifies it as
forward-looking:

- `sl-mesh/src/decode.rs:153` — `Submesh::normalized_scale` is decoded and
  **never read**; every use outside the crate is a construction site writing
  `[1.0, 1.0, 1.0]`. Its own doc admits positions "are *not* pre-multiplied by
  it";
- `sl-texture/src/decode.rs:41` — `DecodedImage::aux` is decoded and consumed
  only by `downsample` carrying it forward. Its doc says it is "kept for later
  material use", i.e. the stated justification *is* that it is unused. Either
  wire it into the bake/material path or drop it and the `decode_multicomponent`
  branch that fills it;
- `sl-mesh/src/decode.rs:223` — `MeshSkin::pelvis_offset` has no reader.
  `sl-avatar/src/skin.rs:31-36` claims it plus `alt_inverse_bind_matrix` and
  `lock_scale_if_joint_position` are "consumed upstream"; two of the three are
  true, `pelvis_offset` is only ever constructed as `None`;
- `sl-proto/src/sim_session.rs:2356` — `circuit_code` is write-only (written at
  `:7540`, never read, no accessor, no reply carries it), and `channel_version`
  (`:2348`) is write-once at construction with no setter — a constant occupying
  a field;
- `sl-msg-template/src/ast.rs:76` — `is_deprecated()` and `MessageDef.flags`
  are referenced only by the crate's own unit test; `build.rs` never reads
  `flags`. The template carries 17 `UDPDeprecated`, 5 `Deprecated` and 4
  `UDPBlackListed` messages, all code-generated unfiltered. Generating them is
  right; never surfacing the flag is not;
- `sl-repl/src/registry.rs:127` — `CommandSpec::usage`, see
  [[repl-audit-format-registry-parity]];
- `sl-asset-sched/src/gate.rs:41` — `capacity: AtomicUsize` is written only in
  `new` and never stored to: a constant dressed as an atomic;
- `sl-viewer-search/src/search.rs:389` — a leading icon column, "unused for now;
  kept for reference-column parity";
- the six never-raised restart notification templates, see
  [[viewer-audit-preferences-restart-note]];
- `Error::Clap` is unconstructible in all three REPL/survey binaries (all use
  `Parser::parse()`, which exits the process), and five `AbuseReport` fields are
  hard-zeroed and unreachable from the REPL grammar (`registry.rs:385-402`).

Two dead defensive branches worth deleting while there:
`sl-wire/src/region_handle.rs:38-42`
(`u32::try_from(self.0 >> 32).unwrap_or(u32::MAX)` — a `u64 >> 32` always fits)
and `sl-notecard/src/decode.rs:429` (`byte & 0x7f` is always `<= 0x7f`, so
`embedded_char` always returns `Some`, and the `if let` with no `else` reads as
though malformed v1 markers are dropped).
