---
id: viewer-lsl-lsp-server
title: LSL language server — lsp-server, document sync, symbols
topic: viewer
status: done
origin: user request (2026-07); split from viewer-lsl-language-server
blocked_by: [viewer-lsl-parser-tree, protocol-lsl-syntax]
refs: [viewer-script-mirror-download]
---

Context: [context/viewer.md](../context/viewer.md).

Speak **LSP**, so a power user gets LSL intelligence in nvim, VS Code, Helix or
anything else instead of a viewer's built-in text box. Nothing in the SL
ecosystem does this: the existing editor extensions carry a hardcoded function
list scraped from the wiki, which rots as Linden Lab adds functions and knows
nothing of OpenSim's OSSL. We are in an unusually good position because we hold
the two things a language server needs and an editor plugin cannot get — the
grid's **symbol table** ([[protocol-lsl-syntax]]: every function with return
type, ordered typed arguments, tooltips and energy/sleep costs; every constant;
events; deprecated and god-mode flags) and the **compiler, via the grid**.

This task is the server foundation: **document synchronisation** and **symbols**
(the grid library from [[protocol-lsl-syntax]] plus the user's own globals,
functions, states and locals from the parse tree, [[viewer-lsl-parser-tree]]).
Diagnostics and navigation build on it in
[[viewer-lsl-lsp-diagnostics-nav]].

**Use `lsp-server`** (rust-analyzer's: synchronous, no async runtime forced on
Bevy, and it has an **in-memory transport** — so the *same* server code runs
embedded in the viewer and as a standalone binary). `tower-lsp` is dead (no
commits since early 2024); `async-lsp` is the fallback if tower middleware is
ever wanted.

The buffer→inventory-item mapping (agent inventory vs. a script inside a prim,
which needs object id *and* item id) is **shared** with
[[viewer-script-mirror-download]] — do not invent a second one.

**Interop, not competition:** Linden Lab shipped an official viewer↔editor
protocol in 2026 (`sl-vscode-plugin`, MIT) — JSON-RPC 2.0 over WebSocket, with
the *viewer* dialling out to the editor, carrying compile diagnostics and even
live `llOwnerSay` / runtime errors. An LSP server and that protocol are
complementary (theirs moves *the viewer's* events to an editor, ours gives *any*
editor language intelligence).

Deps: [[protocol-lsl-syntax]] (the symbol table — without it there is nothing to
complete against).
