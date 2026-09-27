---
id: idiomatic-audit-primitive-typed-patterns
title: Audit for stringly / integer / boolean typed patterns copied from the reference
topic: idiomatic
status: ideas
origin: follow-up to viewer-notification-button-ids-untyped (2026-09-27)
refs: [viewer-notification-button-ids-untyped]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

[[viewer-notification-button-ids-untyped]] found notification button ids kept
as bare strings — the reference viewer's `LLNotification` shape, ported
faithfully — and a wrong string failed silently. Porting from C++ carries that
risk everywhere: the reference encodes meaning in `std::string`, `S32` / `U8`
and `bool` because C++ makes the typed alternative expensive, and a faithful
port inherits the encoding along with the behaviour. Rust makes the typed
version cheap, so each such spot is a class of silent bug we chose to keep.

The earlier passes covered keys and correlation ids (bare `Uuid` → typed keys,
`TransactionId` & co.) and a few state machines (`TeleportPhase`, `SitState`).
This audit is the rest: **meaning carried by a primitive**, in the viewer and
protocol crates alike.

## What to look for

- **Stringly typed** — a `&str` / `String` that is really one of a closed set:
  ids compared against literals (button names, action ids, element ids, setting
  names, template names, menu entries, `UiAction` strings), `match` arms on
  string literals, string-keyed maps whose keys are a fixed vocabulary.
- **Integer typed** — an `i32` / `u8` / `u32` that is an enum or a bitfield in
  disguise: wire codes kept as integers past the decode boundary (`controls: 0 /
  1`, media / permission / flag bitfields handled with hand-written masks,
  combo / radio / tab indices standing in for the choice they select), magic
  constants compared at several sites.
- **Boolean typed** — `bool` parameters whose meaning is invisible at the call
  site (`f(x, true, false)`), several correlated bools that are really one
  state (the `SitState` lesson), and bools that are a two-variant enum with a
  name.

## How

Per finding, record: where, what the primitive actually means, which reference
type it was ported from, whether a mismatch fails silently today, and the
typed replacement (enum, `bitflags`, newtype, typestate). Rank by *silent*
failure first — a comparison that can never match, a bit read from the wrong
field — over mere readability. Keep the wire / persisted representation at the
boundary (decode into the type, encode out of it), as the key newtypes do.

File each worthwhile fix as its own task (bugs where a silent failure is
demonstrated, ideas otherwise); this task is the audit and its findings list,
not the refactors. Exclude what a decision already settled: typed CAPS
capability tokens were considered and deferred (the `CAP_*` constants are
centralised, so typos cannot really happen).
