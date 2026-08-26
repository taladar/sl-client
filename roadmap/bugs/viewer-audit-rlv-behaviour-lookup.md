---
id: viewer-audit-rlv-behaviour-lookup
title: RLV behaviour lookup is param-type-blind and has no modifier fallback
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-rlv-restriction-state, viewer-audit-rlv-behaviour-table-test]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-rlv` parses untrusted in-world chat, and its behaviour lookup is wider than
the reference's:

- `sl-rlv/src/command.rs:146` — `resolve_behaviour` looks up the keyword alone.
  Firestorm keys `m_String2InfoMap` on `(behaviour, paramType)`
  (`rlvhelper.cpp:446`), so a force-only keyword used as a restriction yields
  `RLV_BHVR_UNKNOWN`. Here `@sit=n` classifies as `Sit`/`Add`. The
  `rlv_behaviours!` table (`behaviour.rs:17`) has no param-type column, so this
  cannot currently be expressed.
- `sl-rlv/src/command.rs:163` — no **local behaviour-modifier** fallback. The
  reference's `getBehaviourInfo` retries `<base>_<modifier>` against the base
  behaviour for FORCE commands (`rlvhelper.cpp:447-453`, e.g.
  `@setcam_fov=force`); every such keyword maps to `Unknown` here.

Verified correct and not to be touched: the lowercase-whole-message,
drop-empty-comma-tokens, `clear`-without-param and `:`-only-after-`=` rules
(`command.rs:97-155`) match `rlvhelper.cpp:760-790` and
`llviewermessage.cpp:3145` exactly.

Note the crate has **no consumer anywhere in the workspace**, which is expected
— [[viewer-rlv-restriction-state]] is the pending enforcement layer. Fixing the
table now means that layer inherits a correct classifier. Pair with
[[viewer-audit-rlv-behaviour-table-test]].
