# sl-rlv

Pure decoder and restriction state machine for the Second Life / OpenSim
**RLV / RLVa** `@`-command chat protocol — the language a worn attachment speaks
to control the viewer. It is the RLV counterpart of `sl-prim` and `sl-anim`:
**Bevy-free and I/O-free**, with no session and no grid, so a headless
RLV-compliant client can use exactly this, and it is unit-testable to the
letter.

RLV is not a wire protocol. The carrier is ordinary **owner-say chat**
(`CHAT_TYPE_OWNER` on channel `0`) from an object the agent owns — nothing new
on the wire, which is why it works on any grid with no server support. A message
is an RLV command line when it starts with `@`; the viewer swallows it so it
never reaches the chat log. The payload is a **comma-separated list** of
commands, each `behaviour[:option]=param`, lower-cased.

The crate is two layers. The **language decoder** turns a chat line into a typed
command stream; the **restriction state machine** (`RlvState`) holds what those
commands mean. What it deliberately does not do is *obey* anything: it never
detaches an attachment, hides a name tag or answers a query. A `=force` action
or a `=<channel>` query comes back from `RlvState::apply` as
`RlvOutcome::NotAStateChange` for the consumer to dispatch.

## Decoding

- `is_rlv_line(line)` tests the `@` prefix.
- `parse_chat_line(line)` strips the `@`, splits the payload on `,` (dropping
  empty fields), and decodes each field independently, returning a `Result` per
  field so one malformed command does not sink its neighbours.
- `RlvCommand::parse_field(field)` decodes a single `behaviour[:option]=param`
  field (a leading `@` is tolerated).

Each `RlvCommand` carries:

- the raw lower-cased `keyword` (so an unrecognised or newer-than-us behaviour
  round-trips as `RlvBehaviour::Unknown` without losing its spelling),
- the canonical `behaviour` — one entry of `ERlvBehaviour`,
- `strict` — whether the `_sec` suffix was present on a behaviour that supports
  it (`@recvim_sec=n`),
- `modifier` — the local behaviour modifier the command addressed, if any (see
  below),
- the `option` between behaviour and `=` (a UUID, exception, modifier or path,
  left raw for the consumer to interpret),
- the classified `param` (`RlvParam`):
  - `Add` / `Remove` — `n` / `add` and `y` / `rem` toggle a restriction,
  - `Force` — `force` runs an action now (`@sit:<uuid>=force`),
  - `Reply { channel }` — a number makes it a query answered on that channel
    (`@version=2222`, `@getoutfit=1234`),
  - `Clear { filter }` — `@clear[=<filter>]` drops restrictions,
- and the dictionary `entry` it resolved through, whose flags say whether the
  spelling that arrived is a synonym, extended, experimental or deprecated.

## The keyword is only half the key

A behaviour is identified by the pair `(keyword, param kind)`, not the keyword
alone — the reference keys `m_String2InfoMap` on exactly that pair, with `add`
and `rem` collapsed into one `RLV_TYPE_ADDREM`. The dictionary therefore has one
`RlvEntry` row per pair, and a keyword used with a kind it was not declared for
resolves to `RlvBehaviour::Unknown` while keeping its spelling:

- `@tpto:128/128/25=force` teleports; `@tpto=n` is nothing — there is no such
  restriction.
- `@showloc=n` blocks the location; `@showloc=force` is nothing.
- `@sit` is declared for both, so `@sit=n` and `@sit=force` are both real.
- `@clear=n` classifies as `Add` (the param precedence puts `n` first), which
  then asks for a `clear` *restriction* — so the behaviour is `Unknown`, as in
  the reference.

Several rows may name one behaviour, which is what makes **synonyms** work:
`@touchfar=n` and `@fartouch=n` are two rows and one `RlvBehaviour::Fartouch`,
so an object holding both holds one restriction. The deprecated camera shims
(`@camavdist`, `@camdistmin`, `@camtextures`, `@camunlock`) fold onto the modern
`@setcam_*` behaviours the same way, and every force-wear spelling — `@attach`,
`@attachall`, `@addoutfit…`, `@attach…overorreplace` — folds onto the one
`RlvBehaviour::ForceWear` the reference calls `RLV_CMD_FORCEWEAR`.

## The restriction state machine

`RlvState::apply(object, &command)` is the whole entry point. Restrictions are
held **per issuing object** and **reference-counted** across objects: a
behaviour stays in force while any object holds it, so one collar lifting
`@fly=y` does not hand back flight while another still says no. The bookkeeping
runs both ways — `restrictions_of(object)` and `objects_holding(behaviour)` —
and `clear_object(object)` drops everything an object held, which is what a
detach does and what makes the enforcement layer correct.

Not every command counts. `@sendim=n` blocks every IM; `@sendim:<uuid>=add` does
not block anything, it grants an **exception** letting one avatar through. Get
that wrong and the exception reads back as a restriction, so which commands
reference-count is tabulated per behaviour as an `RlvRestrictionRule` (option
arity, what the option means, and whether either form counts).

Whether *another* object's exception counts is the `@permissive` / `_sec`
question, answered by `is_exception`. Read `@permissive` the other way round to
its name: it is a restriction, and holding it forces every exception-carrying
behaviour into strict mode at once. Under strict mode every object holding the
restriction must also have granted the exception, so a second collar cannot poke
a hole in the first one's block.

A handful of restrictions admit only so many holders: `@setcam`, `@setdebug` and
`@setenv` take one object each so two cannot deadlock over the same subsystem,
and `@setsphere` tops out at six effects. Over the limit, `apply` answers
`RlvOutcome::FailedLock` and leaves no trace.

## Behaviour modifiers

Twenty-one **global** modifier slots (`RlvModifier`, `ERlvBehaviourModifier`)
hold the typed knobs a restriction carries: `@fartouch=n` restricts touch range,
`@fartouch:2.5=n` restricts it *and* says how far. Every object writes into the
same slot, and where "in force" is ambiguous the most restrictive value wins —
the smallest touch range, the largest minimum IM distance. `@setcam` overrides
that: the object that takes exclusive control of the camera becomes the primary
object of every camera slot, so its values win outright.

Thirteen **local** modifiers (`RlvLocalModifier`) are per-object values on that
object's own `@setsphere` or `@setoverlay` effect and never compete. A `=force`
command addresses one as `@<behaviour>_<modifier>` — `@setsphere_mode=force`,
`@setoverlay_alpha=force`. These are not behaviours of their own: when the
direct lookup fails, a `=force` keyword is split at its last `_` and retried
against the restriction rows, and the command comes back as the **base**
behaviour with `modifier` set. `@setoverlay_tween=force`, which *is* a behaviour
of its own, is unaffected. Setting one requires the object to already hold the
restriction it belongs to, and lifting that restriction takes its knobs with it.

## Version handshake and `@getcommand`

`version_reply`, `version_num_reply` and `version_impl_num_reply` answer the
handshake an object opens with: RLV 3.4.3 with a 2.9.28 compatibility floor,
implemented by RLVa 2.4.2. `RlvState::known_commands(filter, kind)` answers
`@getcommand`, honouring `set_experimental_commands` — with the RLVa
experimental set switched off, those keywords are not commands at all and
`apply` refuses them.

The grammar, the classification and the state machine follow Firestorm's
`rlvhandler.cpp`, `rlvhelper.cpp`, `rlvmodifiers.h` and `rlvdefines.h`
(`ERlvBehaviour`, `ERlvParamType`, `ERlvBehaviourModifier`, `RLV_CMD_PREFIX`),
reimplemented idiomatically rather than copied. Channel-0 owner-say gating is
the caller's job — this crate decodes a line it is handed.
