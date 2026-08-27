---
id: protocol-audit-roxmltree-nesting-in-sl-wire
title: sl-wire's own XML parsing has no nesting guard
topic: protocol
status: done
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

## Fixed (2026-08-27)

The pre-scan moved out of `sl-llsd`'s LLSD-XML codec and into its own module,
`sl-llsd/src/xml.rs`, which exports one guarded entry point —
`parse_guarded_xml` — plus `xml_nesting_within` for callers that want the
predicate alone. `parse_llsd_xml` now goes through it like everyone else, so
there is a single place the bound is enforced and a single place to change it.

**Every `roxmltree::Document::parse` in the workspace now goes through that
entry point.** The sweep found ten, not the dozen the exposure note estimated:
the other public functions returning `Result<_, roxmltree::Error>` were already
safe, because they decode through `parse_llsd_xml` rather than parsing
themselves.

- `sl-wire` (6): `xmlrpc::method_name`, `xmlrpc::parse_method_call`,
  `xmlrpc::parse_method_response`, `grid_info::parse_grid_info_xml`,
  `login::parse_login_response` and `login::parse_login_request` — the login
  response being the body that arrives before a session exists.
- `sl-avatar` (4): `MorphMasks::from_xml`, `VisualParams::from_xml`,
  `Skeleton::from_xml` and `AttachmentPoints::from_xml`. Not on the network
  path — these are the `character/` assets `SL_VIEWER_ASSETS` points at — but
  `parse_raw_bone` and `flatten` recurse per bone level on top of roxmltree's
  own recursion, so the same guard bounds all three at once, and leaving one
  unguarded parser in the workspace invites the next one.

`xmlrpc::value_to_llsd` also grew its own depth bound, on the same reasoning as
`sl-llsd`'s `node_to_llsd`: it takes a *node*, not a body, so being public it
cannot assume the document came through the guard. Past `MAX_NESTING_DEPTH` the
walk yields `Undef` rather than recursing.

Eleven new tests. Eight of them nest 4_000–12_000 levels and **abort the test
binary** rather than failing if the guard is removed, which is the property
worth pinning; the `NodesLimitReached` they assert can only come from our
pre-scan, since roxmltree's own `nodes_limit` defaults to `u32::MAX`. The two
scan tests that already existed moved to the new module with the code.

Not done: reporting it upstream. That stays a judgement call for a maintainer
to make, and the workaround costs one byte scan.
