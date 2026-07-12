---
id: idiomatic-p7-01
title: Name-lookup ids the Phase-5 sweeps missed:
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

A fresh audit after the Phases 1–6 sweeps found three remaining classes of the
same gaps, pursued under three user decisions (via `AskUserQuestion`): (A) raw
ids still in user-facing APIs become typed newtypes — **new ones kept
client-local in `sl-proto`** per the standing rule; (B) **maximal** in-band-nil/
`0`-sentinel → `Option`, with a documented exception list; (C) silently-masked
decode sites become **always a hard `WireError`** on a present-but-malformed
value (absence stays `Option`/default, never an error).

**A — domain-id newtypes (DONE):**

Name-lookup ids the Phase-5 sweeps missed: `AvatarName.id` → `AgentKey`,
    `GroupName.id` → `GroupKey`, `DisplayName.id` (sl-wire) → `AgentKey`.
    Codec wraps at the boundary (LLSD/UDP byte-identical); `DisplayName` lost
    its derived `Default` (no `AgentKey::default`) → equivalent manual impl.
    (commit "Phase 7 A1")
