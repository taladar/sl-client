---
id: repl-c2
title: Context + placeholders
topic: repl
status: done
origin: SL_REPL_ROAD_MAP.md — Phase C — shared library `sl-repl`
---

Context: [context/repl.md](../context/repl.md).

**C2. Context + placeholders.** `context.rs`: `ReplContext` +
`SessionContext` (`apply_event`), forward resolution
(`$self/$session/$circuit/$region/$parcel/$lastobj/$cap:Name/$var` → literal
at **dispatch time**), reverse **symbolizer**, and `info!` **binding lines**
on change. `PendingCommand::resolve(&ctx)`. Tests: resolve from stub ctx;
unresolvable errors.
