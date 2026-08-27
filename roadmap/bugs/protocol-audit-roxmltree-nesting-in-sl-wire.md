---
id: protocol-audit-roxmltree-nesting-in-sl-wire
title: sl-wire's own XML parsing has no nesting guard
topic: protocol
status: bugs
origin: found while fixing protocol-audit-llsd-recursion-depth-cap (2026-08-27)
points: 3
refs: [protocol-audit-llsd-recursion-depth-cap, protocol-audit-wire-error-contract]
---

Context: [context/protocol.md](../context/protocol.md).

**roxmltree's element parsing recurses and overflows the stack** somewhere
between 1000 and 2000 levels of nesting — measured, not assumed: parsing
`<array>` repeated 2000 times aborts the process inside
`roxmltree::Document::parse`, before any caller is handed a tree.

Its own guards do not cover this. The `depth` field in its parser bounds
*entity references* (the billion-laughs case, limit 10); `nodes_limit` bounds
node **count** and defaults to `u32::MAX`, and a deep-but-narrow document has
one node per level, so it never approaches either.

[[protocol-audit-llsd-recursion-depth-cap]] closed this for `sl-llsd` by
pre-scanning the document's nesting before calling roxmltree. But `sl-wire`
parses XML through roxmltree **directly** in about a dozen places — the
XML-RPC login response and the CAPS bodies among them (they are the 13 public
functions returning `Result<_, roxmltree::Error>`). Those paths still have the
exposure, and the login response arrives before the session exists.

Scope: route `sl-wire`'s XML entry points through the same pre-scan — either by
lifting `sl_llsd::nesting_within` to a shared helper and calling it, or by
giving `sl-wire` one guarded `parse_xml` that every caller uses. Worth doing
alongside [[protocol-audit-wire-error-contract]], which is already going to
touch every one of those signatures.

Worth reporting upstream as well: a recursive-descent XML parser with no depth
bound is a denial of service for anything parsing untrusted documents.
