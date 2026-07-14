---
id: viewer-lsl-language-server
title: LSL language server (LSP) — real tooling for external editors
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [protocol-lsl-syntax]
---

Context: [context/viewer.md](../context/viewer.md).

Speak **LSP**, so a power user gets completion, hover docs, go-to-definition and
*real compile diagnostics* for LSL in nvim, VS Code, Helix or anything else —
instead of being confined to a viewer's built-in text box.

Nothing in the SL ecosystem does this. The existing editor extensions carry a
hardcoded function list scraped from the wiki, which rots as Linden Lab adds
functions and knows nothing of OpenSim's OSSL. **We are in an unusually good
position to do it properly, because we hold the two things a language server
needs and an editor plugin cannot get:**

- **The symbol table, from the grid itself.** [[protocol-lsl-syntax]] fetches
  the `LSLSyntax` capability: every function with its return type, ordered typed
  arguments, tooltips and **energy / sleep costs**; every constant with type and
  value; events; deprecated and god-mode flags. That is completion, signature
  help and hover documentation, *correct for the grid you are connected to* —
  including OSSL on OpenSim, automatically.
- **The compiler, via the grid.** Uploading a script returns a structured
  `ScriptCompileError` (already parsed into line, column and message —
  `sl-proto/src/types/script.rs`). Those map straight onto LSP diagnostics, with
  the "energy/sleep" data available as inlay hints if we want them.

## The catch worth designing around

**There is no compile-without-save on SL.** Compilation happens *as part of the
upload*: the simulator stores the source and reports whether it compiled. So
"diagnostics on save" has a **side effect on the grid** — you cannot silently
type-check a buffer. Decide the semantics explicitly rather than discovering
them:

- diagnostics only on an explicit save/compile action, not on every keystroke;
- make it obvious *which* in-world script a buffer maps to before it is
  overwritten;
- a client-side parser could give cheap syntax-only diagnostics with no grid
  round-trip (LSL is small: C-like, state machines, no generics), leaving the
  authoritative semantic errors to the grid compiler. Worth considering, not
  required for v1.

## Shape

**Use `lsp-server`** (rust-analyzer's: synchronous, no async runtime forced on
Bevy, and it has an **in-memory transport** — so the *same* server code runs
embedded in the viewer and as a standalone binary). Note `tower-lsp` is dead
(no commits since early 2024); `async-lsp` is the fallback if tower middleware
is ever wanted.

The buffer→inventory-item mapping (agent inventory vs. a script inside a prim,
which needs object id *and* item id) is shared with
[[viewer-script-external-workflow]] — do not invent a second one.

**Interop, not competition:** Linden Lab shipped an official viewer↔editor
protocol in 2026 (`sl-vscode-plugin`, MIT) — JSON-RPC 2.0 over WebSocket, with
the *viewer* dialling out to the editor, carrying compile diagnostics and even
live `llOwnerSay` / runtime errors. Firestorm has not adopted it. Speaking it
costs little and hands existing VS Code users our viewer; an LSP server and that
protocol are complementary (theirs moves *the viewer's* events to an editor,
ours gives *any* editor language intelligence).

The one LSP nobody else can build: the existing LSL language server bakes in a
static function list, so it cannot see OpenSim's OSSL. Ours takes its symbols
from the connected grid.

Beyond the basics, the grid data makes some genuinely nice features cheap:
warn on `deprecated` functions, flag god-mode-only calls, surface sleep/energy
cost inline (LSL performance is dominated by exactly these), and complete event
names within the right `state` block.

Deps: [[protocol-lsl-syntax]] (the symbol table — without it there is nothing to
complete against).
