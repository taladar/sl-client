---
id: protocol-lsl-syntax
title: LSLSyntax capability — fetch, cache and decode the grid's language definition
topic: protocol
status: done
origin: user request (2026-07)
---

Context: [context/protocol.md](../context/protocol.md).

The grid tells us the scripting language it actually speaks, and we throw it
away. `SimulatorFeatures` already decodes **`lsl_syntax_id`**
(`sl-wire/src/sim_features.rs`) — and then nothing consumes it: there is no
`LSLSyntax` capability constant, it is not in `REQUESTED_CAPABILITIES`, and
there is no fetcher, cache or schema type. This is the one real protocol gap
behind every scripting feature.

What the capability gives us: a plain HTTP GET on the `LSLSyntax` cap returns an
LLSD document (`llsd-lsl-syntax-version: 2`) with five groups — **`functions`,
`constants`, `events`, `controls`, `types`**. Each function carries its return
type, its **ordered arguments with types and per-argument tooltips**, a
description, and its **energy and sleep costs**; each constant its type, value
and tooltip. Entries may be flagged `deprecated` or `god-mode`.

That single fetch is worth an enormous amount downstream: syntax highlighting,
hover tooltips, autocomplete, an insert-function list, and the symbol table a
language server needs — all of it *current for the grid you are on*, rather than
a hardcoded list that rots as Linden Lab adds functions.

**And it works on OpenSim.** OpenSim's `SimulatorFeaturesModule` registers the
same cap (enabled by default) and serves `bin/ScriptSyntax.xml` in the same
schema — **already containing the OSSL `os*` functions**. So implementing this
gets OSSL support for free, where Firestorm needs a hardcoded fallback list
(`scriptlibrary_ossl.xml`) to fake it.

Scope:

- Add the `LSLSyntax` cap constant and request it; fetch on `SimulatorFeatures`
  arrival and whenever `lsl_syntax_id` **changes** (it changes per region /
  grid).
- **Cache keyed by the syntax id** — that is what the id is for; the document is
  large and changes rarely. Persist it so a restart is free. (Caution: OpenSim's
  id is a *hardcoded UUID in the file*, not a content hash, so a grid operator
  editing the file without bumping the id will serve stale content to a
  content-addressed cache. Consider validating cheaply.)
- Decode into a typed keyword/symbol table (functions with args + costs,
  constants with types and values, events, controls, types), gated on
  `llsd-lsl-syntax-version == 2` — refuse an unknown version rather than
  parsing it wrongly.
- Fall back gracefully when a grid serves no syntax (older OpenSim, or the
  feature disabled): a shipped default list, or simply no identifier
  highlighting.

Consumed by [[viewer-lsl-editor-widget]] (highlighting, tooltips, the insert
list) and [[viewer-lsl-lsp-server]] (completion and hover for external
editors).

Reference (Firestorm, read-only): `llsyntaxid.cpp` (the
`SimulatorFeatures.LSLSyntaxId` → cap → cache-by-uuid flow, and the
version gate), `llkeywords.cpp` (`processTokensGroup` — how the five groups
become tokens, and how the tooltip strings are synthesised).

Builds on: `api-g14` (`SimulatorFeatures`, which already decodes the id).
