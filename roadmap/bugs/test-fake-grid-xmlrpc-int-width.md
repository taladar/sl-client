---
id: test-fake-grid-xmlrpc-int-width
title: Audit every login field emitted as <i4> for values that do not fit S32
topic: test
status: bugs
origin: first Firestorm cross-check harness run (2026-09-01)
points: 2
refs: [test-fake-grid-circuit-code-as-i4-crashes-viewers]
---

Context: [context/testing.md](../context/testing.md).

`push_int_member` (`sl-wire/src/login.rs:2186`) writes `<i4>`, which is
**signed 32-bit** per the XML-RPC spec. It is called with `i64` from 19 sites,
several of which carry values that are not S32:

- **`circuit_code`** (`:1928`) — u32, random, over `S32_MAX` about half the
  time. Already crashing Firestorm; tracked in
  [[test-fake-grid-circuit-code-as-i4-crashes-viewers]].
- **`seconds_since_epoch`** (`:1999`) — passes `S32_MAX` on 2038-01-19. Not
  urgent, but it is the same defect and it will not announce itself when it
  arrives; a test that pins a far-future clock would.
- `region_x` / `region_y` (`:1950`, `:1953`) — u32 grid coordinates in metres.
  Safe for any plausible grid, but the type permits it.

The rest (`sim_port`, `http_port`, `region_size_*`, `max-agent-groups`,
`classified_fee`, `directory_fee`, `category_id`, `type_default`, `version`,
`buddy_rights_*`, `address_size`, `last_exec_*`) are genuinely small.

Two things worth doing:

1. Make the overflow impossible to write rather than merely absent. Either
   have `push_int_member` take an `i32`, so a u32 field cannot be passed
   without an explicit decision at the call site, or have it fall back to
   `<string>` when the value does not fit — but the explicit-type route is
   better, because it forces each field to be classified once instead of
   silently changing shape at runtime depending on the value.
2. Decide the rule for u32-valued fields generally: real grids send them as
   `<string>`, and the reference viewer parses them with `strtoul` from text.
   "Wire-compatible with what the reference viewer expects" is the standard
   here, not "valid XML-RPC" — those happen to agree, but the first is the one
   that matters.

The general lesson for the fake grid: its reader accepts more shapes than its
writer emits, so a round-trip test proves nothing about interop. Where a field
has a known on-the-wire shape, assert the shape, not just the round trip.
