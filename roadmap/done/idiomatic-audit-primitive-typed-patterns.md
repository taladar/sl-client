---
id: idiomatic-audit-primitive-typed-patterns
title: Audit for stringly / integer / boolean typed patterns copied from the reference
topic: idiomatic
status: done
origin: follow-up to viewer-notification-button-ids-untyped (2026-09-27)
refs: [viewer-notification-button-ids-untyped]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## Done (2026-10-03)

Audited the whole workspace in five slices (protocol / session, asset and
codec crates, viewer UI infrastructure, two halves of the viewer feature
crates), cross-checking both ends of every string or code vocabulary that
could be diffed mechanically (menu and pie actions vs handler arms, condition
keys vs pushes, template names vs the catalogue, `.arg` keys vs `[KEY]`
placeholders, sort tokens vs comparators, setting registrations vs reads,
flag and code values vs the reference headers). Every claimed silent failure
below was re-verified against the code and the reference before filing.

**Silent failures demonstrated — filed as bugs:**

- [[viewer-build-phantom-flag-wrong-bit]] — the Build floater's Phantom bit
  is `FLAGS_OBJECT_ANY_OWNER` (bit 4, not 10): Physical / Temporary toggles
  make owned objects phantom. The worst find; a fifth copy of `PrimFlags`.
- [[protocol-parcel-info-reply-flags-misread]] — group-owned read as for-sale.
- [[protocol-classified-query-wrong-flag-space]] — classified search sends
  `DirFind` maturity bits.
- [[protocol-region-flag-deny-ageunverified-value]] — wrong bit, not yet read.
- [[protocol-voice-accept-on-direct-session-becomes-conference]] — the
  `from_group` bool encoding of a session kind.
- [[protocol-login-options-max-agent-groups-unrequested]] — comment and
  option filter disagree; needs an aditi check.
- [[repl-unknown-keyword-arguments-ignored]] — `knd=objectpay` sends a gift.
- [[viewer-bespoke-card-responses-lose-the-button]] — every bespoke card
  click recorded as "no choice"; script dialog button names collide.
- [[viewer-default-next-owner-mask-diverges]] — no `PERM_MOVE`, three masks.
- [[prim-path-curve-byte-ignores-reference-mask]] — exact byte vs `& 0xf0`.
- [[protocol-agent-list-voice-transition-lossy]] — text-only member
  announced as leaving.

**Latent (all agree today; one rename or typo from silent) — filed as ideas:**

- [[idiomatic-typed-ui-actions-and-menu-conditions]] — the largest class:
  `UiAction` / menu / pie action strings with `_ => {}` handlers, condition
  keys, accelerator strings.
- [[idiomatic-typed-setting-keys]] — `&str` setting names, per-read types,
  enum-valued settings stored as raw strings / ints / list indices.
- [[idiomatic-notification-template-names-typed]] — `ShowNotification`
  names and bespoke card templates (the `TemplateRef` machinery extends).
- [[idiomatic-prim-flags-and-object-codes]] — one `PrimFlags`, object codes.
- [[idiomatic-permissions-type-everywhere]] — raw `u32` masks, group powers.
- [[idiomatic-protocol-codes-past-decode]] — integers past decode.
- [[idiomatic-protocol-string-vocabularies]] — estate methods, event-queue
  names, voice types, upload state, login options, maturity codes.
- [[idiomatic-table-sort-column-enums]] — sort tokens with catch-all arms.
- [[idiomatic-lsl-event-id]] — LSL events by name in the runtime.
- [[idiomatic-avatar-typed-ids]] — global colours, visual params, texture
  slots, built-in animations.
- [[idiomatic-opaque-bool-parameters]] — correlated and swappable bools.
- [[idiomatic-index-and-sentinel-choices]] — combo indices, nil / 0 / -1
  sentinels, cache kind strings.

**Checked and clean** (not filed): sculpt type / flag bits, alpha-mode codes,
OpenSim prim flag names, permission bit values, inventory type names,
bump / shiny / fullbright masks, `ParcelFlags` / `RegionFlags` /
`EstateFlags` consumers, every live menu action has a handler, every
condition key is pushed, every raised template and `.arg` key exists, the
bake plan's mask / tint ids against `avatar_lad.xml`, LSP position handling.

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
