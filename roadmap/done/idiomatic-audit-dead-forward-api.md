---
id: idiomatic-audit-dead-forward-api
title: Decoded-but-never-read fields and write-only state across the workspace
topic: idiomatic
status: done
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

**Done.** Each item became load-bearing or went away; two of the audit's claims
had been overtaken by later work and are recorded as such.

**Made load-bearing.**

- **`DecodedImage::aux` now *is* the clothing-morph mask.** A Second Life server
  bake is a 5-component `R G B alpha M` image whose fifth channel is the
  clothing-coverage mask; the reference decodes it separately
  (`decodeChannels(aux, .., 4, ..)`) and feeds it to `applyMorphMask` as a
  one-component image. `part_clothing_mask` sampled the RGBA *alpha* instead —
  composited transparency where the reference reads coverage. The new
  `morph_mask_texture` picks the aux plane at stride 1 when the decode kept one
  and falls back to the alpha at stride 4 when it did not (a 4-component bake,
  an OpenSim region, Firestorm's null-aux path), which is the case that masks
  nothing and lets the morphs flare fully.
- **`MeshSkin::pelvis_offset` became the avatar's pelvis fixup (R1).** A rigged
  mesh uploaded with a pelvis offset asks its wearer to stand that much higher
  (`addPelvisFixup` / `hasPelvisFixup`, applied in
  `LLVOAvatar::getRenderPosition`). `JointOverrides` gained `pelvis_fixup`,
  filled by `joint_position_overrides` from the same alternate-bind gate the
  joint positions ride, merged the reference's way (a later mesh's fixup wins, a
  rig with none leaves the standing one alone) and folded into
  `root_drop_from_metrics` beside `hover`, which is the same Z shift of the
  planted body. `JointOverrides::is_empty` now counts the fixup, so a rig that
  deviates no joint but carries an offset is no longer discarded on the way into
  the per-mesh store.
- **The template's deprecation flags reach the generated API.**
  `MessageDef::status()` (a new `MessageStatus`, harshest flag wins, with
  `is_deprecated` rewritten on top of it) is read by `sl-wire`'s `build.rs`,
  which emits `Message::STATUS` per message plus `AnyMessage::status()` and a
  `message_status(id)` companion to `message_name`.
  `Diagnostic::UnhandledMessage` carries and renders it: an unmodelled message
  the grid itself marks `UDPDeprecated` is expected traffic the client is right
  to ignore (Second Life moved it to CAPS), while an unmodelled *current*
  message is a gap — the two used to look identical in the log.
- **`SimSession::set_channel_version` / `channel_version()`.** The
  `AgentMovementComplete` field is what a viewer shows as the region's server
  build, so a server built on `SimSession` now names itself; `sl-fake-grid`
  reports `sl-fake-grid <version>`, which also means a capture taken against it
  cannot be mistaken for one taken against a real simulator.
- **The five hard-zeroed `AbuseReport` fields are reachable.** `details`,
  `version_string`, `screenshot_id`, `abuse_region_id` and `check_flags` are now
  keyword-only arguments (`args::KEYWORD_ONLY + 0..4`) of the shared
  `abuse_report_from_args`, so both `send_abuse_report` and
  `send_abuse_report_caps` can set them; both usage strings name them.

**Dropped or de-atomised.**

- `Error::Clap` / `ClapError` deleted from all three REPL/survey binaries.
  `Parser::parse()` exits the process, so the variant was unconstructible.
- `PriorityGate::capacity` is a plain `usize`. It was never stored to after
  construction, so the atomic bought nothing but a `load` at each read.

**Corrected, not removed.**

- `Submesh::normalized_scale` **is** read — by `sl_mesh::encode`, which writes
  it back, so dropping it would silently reset an uploader's normalisation on a
  re-upload. The reference's only *other* reader un-normalises positions before
  MikkTSpace and re-normalises the tangents after; this viewer transports no
  vertex tangents by design (it reconstructs the basis per fragment from
  screen-space derivatives), so nothing on the render path wants it. The doc now
  says that instead of apologising, and a round-trip test pins a non-identity
  value (together with a non-`None` `pelvis_offset`).
- `sim_session.circuit_code` gained a reader since the audit: a second
  `UseCircuitCode` on a live circuit is checked against the bound triple.
  Nothing to do.
- `sl-viewer-search`'s "leading icon column" doc comment described something
  that never existed — the function it sits on is the ordinary text-column
  helper, used by all fifteen columns of all six tables. Reworded.
- `CommandSpec::usage` was given its `help` reader by
  [[repl-audit-format-registry-parity]].

**Still open elsewhere.** The six never-raised restart notification templates
stay with [[viewer-audit-preferences-restart-note]], which owns them.

**Both dead defensive branches are gone.** `RegionHandle::global_coordinates`
and `from_global` are byte-array splits/joins (`to_be_bytes` /
`from_be_bytes`) — total, `const`, and each other's exact inverse — and
`from_grid`/`grid_coordinates` lost their `checked_mul`/`checked_div` fallbacks
the same way (`saturating_mul`, `wrapping_div`), so a grid index past
`u32::MAX / 256` saturates rather than wrapping into another region's handle.
The notecard v1 text decode no longer reads as dropping malformed markers:
`byte & 0x7f` cannot leave the embedded code-point range, and
`v1_maps_every_high_bit_byte` pins that for all 128 high-bit bytes.
