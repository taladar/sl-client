---
id: test-fake-grid-circuit-code-as-i4-crashes-viewers
title: Login response sends circuit_code as <i4>, which overflows S32 and kills Firestorm
topic: test
status: bugs
origin: first Firestorm cross-check harness run (2026-09-01)
points: 1
refs: [test-firestorm-crosscheck-runner, test-fake-grid-xmlrpc-int-width]
---

Context: [context/testing.md](../context/testing.md).

`sl-fake-grid` serialises `circuit_code` as an XML-RPC `<i4>`
(`sl-wire/src/login.rs:1928`, `push_int_member(out, "circuit_code",
i64::from(success.circuit_code.get()))`). `circuit_code` is a **u32** and is
randomly minted, so it exceeds `S32_MAX` roughly half the time. `<i4>` is
signed 32-bit by the XML-RPC spec, so those values are out of spec.

Firestorm dies on them. Observed on the very first harness run, with
`circuit_code = 3337137618`:

```text
std::stoi("3337137618") -> std::out_of_range -> uncaught -> terminate -> SIGSEGV
  LLXMLNode::fromXMLRPCValue      (indra/llxml/llxmlnode.cpp:3434)
  LLXMLRPCTransaction::parseResponse (indra/newview/llxmlrpctransaction.cpp:297)
```

The viewer never reaches `STATE_WORLD_INIT`; it crashes in
`STATE_LOGIN_PROCESS_RESPONSE`, so this blocks every cross-viewer comparison
run until fixed.

**Send it as `<string>`**, which is what real grids do and what the reference
viewer expects: `llstartup.cpp:4916` reads it back with
`response["circuit_code"].asString()` and then `strtoul()` — unsigned, from
text. This workspace's own reader already accepts either, because
`parse_parsed` takes the element's text and `parse::<T>()`s it
(`login.rs:1676`), so only the writer is wrong. That asymmetry — a reader that
accepts text and a writer that emits `<i4>` — is why the round-trip tests
never caught it.

A round-trip test cannot catch this class of bug on its own; add a case that
mints a `circuit_code` above `S32_MAX` and asserts the serialised form is not
`<i4>`. See [[test-fake-grid-xmlrpc-int-width]] for the other fields with the
same shape.
