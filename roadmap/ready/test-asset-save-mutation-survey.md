---
id: test-asset-save-mutation-survey
title: Does a grid hand back the asset you saved?
topic: test
status: ready
origin: scoping test-fake-grid-asset-round-trip (2026-09-05)
points: 3
refs:
  [
    test-fake-grid-asset-round-trip,
    test-notecard-create-update,
    test-asset-upload,
    test-script-upload,
    test-baked-texture-upload,
    protocol-audit-notecard-fidelity,
  ]
---

Context: [context/testing.md](../context/testing.md).

Nobody knows, and the one place that looked was careful not to claim it.
[[test-notecard-create-update]] re-fetches the body it has just written and
compares the **length**, recording `roundtrip: match | mismatch | skipped` as
a *metric* rather than asserting it. That is the entire state of the
workspace's knowledge about whether a save comes back as it went in.

It matters because a fake grid has to pick a behaviour, and the wrong pick is
worse than none: a grid that always echoes the upload back lets a viewer bug
through — an editor that trusts its own in-memory copy after a save and never
re-reads is indistinguishable from a correct one against an echo. The cases a
real grid *mutates* are the ones with teeth, so which those are is a finding
the fake grid gets built from, not a detail settled inside it.

Classes where a difference is plausible enough to go looking:

- **Notecards with embedded inventory.** `LLEmbeddedItems` holds real
  inventory items; a simulator has to re-issue their ids and rewrite their
  permissions on save. Both existing notecard cases write `count 0`, which
  dodges exactly this. See [[protocol-audit-notecard-fidelity]] for the
  container format.
- **LLSD and JSON documents — settings (EEP), materials (GLTF).** Anything a
  simulator parses in order to validate it, it also re-serialises: key order,
  float formatting and whitespace all move. Byte equality here would be the
  surprise rather than the rule.
- **Scripts.** The source may well come back verbatim, but the save also
  produces a *second* asset — the compiled bytecode — that the completion
  reply never names and a viewer never fetches, so "the round trip" is the
  wrong shape for the class before it is even measured.
- **Textures and mesh over `NewFileAgentInventory`.** Second Life validates
  and may transcode; a mesh is picked apart for its LODs and physics hull and
  priced, which is at least an opportunity to rewrite it.
- **Anything carrying permissions inside the asset** — wearables, objects,
  notecard embeds. Next-owner masks are applied on transfer, and for these
  classes that is a mutation of the *bytes*, not only of the item.
- **A take.** Not a round trip at all: the grid authors the object asset from
  live state, choosing position, `task_id`, permissions and the contents
  serial. Listed so the survey records it as "authored, not echoed" rather
  than leaving the next reader to wonder.

The measurement is the ordinary conformance shape — upload a known body,
re-fetch the id the grid returned, compare — run against **both** live grids,
because this is precisely the kind of thing they diverge on and
`asset-upload` already found one divergence in the same area (Second Life
refuses a notecard through `NewFileAgentInventory` at all). Where the bytes
differ, the record wants the *shape* of the difference — re-serialised,
re-issued ids, extra fields, truncated — not just a boolean, because that is
what the fake grid has to reproduce.

This is a live-grid task by construction: the fake grid cannot answer it,
since what the fake grid should do is the output. The aditi half runs under
the usual cooldown rules.

Acceptance: a record per savable asset class, on OpenSim and on aditi,
saying whether a re-fetch returns the uploaded bytes and — where it does not
— what changed; and [[test-fake-grid-asset-round-trip]] can state its own
acceptance in terms of it instead of assuming an echo.
