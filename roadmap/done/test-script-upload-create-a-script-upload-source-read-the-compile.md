---
id: test-script-upload
title: create a script, upload source, read the compile
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 9 — Scripting & permissions `[both]`
---

Context: [context/test.md](../context/test.md).

`script-upload` — create a script, upload source, read the compile. `1av`.
    **OpenSim green; SL run deferred to Phase Z** (see the SL-task-write note
    there). Editing a script's source is not a plain asset upload: the viewer
    never compiles LSL/Lua locally — it POSTs the raw source to the
    `UpdateScriptAgent` (agent inventory) or `UpdateScriptTask` (task
    inventory) capability with a requested compile `target`
    (`mono`/`lsl2`/`luau`), and the **simulator compiles synchronously**,
    returning a `compiled` flag and an `errors` array; a script can upload as
    an asset yet fail to compile. The case uses the **task-inventory** path
    (only a task upload compiles on OpenSim — its agent path just stores the
    asset): rez a throwaway cube, create a script **directly in it** with
    [`RezScript`](Command::RezScript) + [`RestoreItem::new_script`] (the
    viewer's object-Contents "New Script" — a null-id/null-asset item the sim
    fills with a default body), fetch the listing for the task item id, then
    [`UploadScript`](Command::UploadScript) **valid** source (asserts
    `compiled == true`, no errors) and **invalid** source (asserts
    `compiled == false`, a non-empty error list, and that the first
    [`ScriptCompileError`] parsed a `line`/`column` — the payoff of the
    structured parse). Green on OpenSim: valid compiles, invalid → 3 errors,
    real XEngine format `(4,20) Error: …` parsed to line 4 col 20. New surface
    (all wired through both runtimes + REPL): `ScriptTarget`
    (`#[non_exhaustive]`, `luau` confirmed from LL viewer source),
    `ScriptLanguage` (the item-flags subtype), `ScriptUploadLocation`,
    `ScriptCompileError`, `Command::UploadScript`/`CreateScript`,
    `Event::ScriptUploaded`, the `UpdateScriptTask` cap, and the parent-aware
    CRC helpers `RestoreItem::for_task_drop`/`new_script`; scripts are removed
    from the generic upload commands at the type level (`UpdatableAssetType`)
    so the compile-blind path can't touch them.
