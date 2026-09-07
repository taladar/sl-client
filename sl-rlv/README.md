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

The crate is six layers. The **language decoder** turns a chat line into a
typed command stream; the **restriction state machine** (`RlvState`) holds what
those commands mean; the **query layer** (`RlvState::answer`) builds the line a
`@get*` question is answered with; the **lock model** (`RlvLocks`) answers
the one question a yes/no restriction cannot — not "is detaching blocked" but
"may *this* come off"; the **enforcement façade** (`RlvActions`) is the one
predicate every call site asks before it acts, and the one filter arriving chat
and IMs pass back through; and the **extension commands**
(`RlvState::run_extension`) are the ones the behaviour dictionary never claimed.
What it deliberately does not do is *obey* anything: it never detaches an
attachment and never hides a name tag. A `=force` action comes back from
`RlvState::apply` as `RlvOutcome::NotAStateChange` for the consumer to
dispatch.

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

## Answering queries

A command whose param is a **number** is a question: `@getoutfit=2222` means
"chat what I am wearing on channel 2222". `RlvState::answer(issuer, &command,
&source)` produces the `RlvReply` to shout, and `RlvQuery::classify` on its own
decodes the question and its options if a consumer wants to dispatch them
itself.

The split is drawn at *facts*, not at formatting. Everything a script sees is
built in this crate and is unit-tested: the `@getattach` bit string with its
leading zero, the frozen `@getoutfit` slot order, the comma-joined name lists,
the `|32` wear digits of `@getinvworn`, the `@getstatus` leading separator, the
reply-channel rules and the 1023-byte chat cap. Four families need nothing
outside — `@version*`, `@getstatus` / `@getstatusall`, `@getcommand` and the
`@getcam_*` limits, which read back a modifier slot. Everything the crate cannot
know it asks an `RlvQuerySource` for: what is attached and worn, what may still
be attached or taken off, what the agent is sitting on, the active group, the
hover height, the camera, and the `#RLV` shared-inventory tree.

Two reference details are load-bearing. A query that **fails** is still
answered, with an empty string, because a script that asked a question and heard
nothing would wait forever; the one case with no reply at all is a channel a
reply may not go on. And the answer is *shouted*, so it is truncated at 1023
bytes rather than split — `split_chat` exists for `@redirchat`, not for queries.

`RlvImQuery` covers the other, much smaller surface: `@stopim`, `@version`,
`@list` and `@except` sent by **instant message** from a person rather than by
chat from an object.

## Locks

`@fly=n` is a yes/no the whole viewer asks about. The wear restrictions are not:
`@detach=n` locks *this object* on, `@remattach:chest=n` locks *one attachment
point*, `@addoutfit:gloves=n` locks *one clothing layer*, and `@detachallthis=n`
locks *a folder and everything under it*. Every wear and detach path therefore
has to ask about the thing in front of it, and `RlvLocks::of(&state)` is what it
asks.

The four registries are **derived**, not maintained. Everything in them is
already in the held-command list, so there is no second copy of the truth to
drift: an object detaching drops its locks because `RlvState::clear_object`
dropped its commands, with nothing else to remember. What cannot be derived
comes from an `RlvLockSource` the consumer implements — which attachments hang
off a point, which folder an item came from — plus one fact cached on the state
machine by `RlvState::set_object_attachment`: where the *issuing* object is
worn, because a bare `@detach=n` means "this object" and `@detach=y` routinely
arrives after the object is already gone.

`can_attach`, `can_detach`, `can_wear` and `can_remove` are the predicates every
wear path must consult, and are the honest implementation of the four `can_*`
methods on `RlvQuerySource`. Folder locks resolve by walking up the inventory
tree: a `PERM_DENY` lock locks a folder outright, a `PERM_ALLOW` lock from some
object makes every later lock *from that same object* stop counting (which is
how `@detachthis_except` works, and why a second collar's lock is not exempted
by the first one's exception), a node-scoped lock counts only on the folder
asked about, and `@unsharedunwear` locks the whole inventory and then punches
`#RLV` back out of it. Folded folders (`.(chest)`, `.(nostrip)`) are their
parent for locking, and the `nostrip` naming convention exempts an item from
being taken off by a command at all — no object issued it and nothing lifts it.

## The re-attach watchdog

A lock is only a rule; the simulator does not know about it, so a user can
detach a locked attachment anyway. `RlvAttachmentWatchdog` is what makes the
lock real: it notices and puts it back. Three things get undone — a locked
attachment that came off (after waiting for the simulator to save its asset back
into inventory, and forcing it after 15 seconds if that never arrives), a wear
that landed on an add-locked point (restored to exactly what was there when the
wear was *asked for*, which is why `on_wear_requested` exists), and a replace
onto a point holding something locked (refused outright, or — with
`RLVaWearReplaceUnlocked` — allowed to displace only what was free to go).

It decides and does not act: `RlvWatchdogAction` says what to send. Time comes
in as a plain seconds count on every call, so the whole machine is testable
without a clock.

One deliberate divergence lives here. `RlvAttachmentPoint::group` answers what
the point anatomically is; Firestorm derives it from a hard-coded index table
that reads joint group `8` as the HUD group, but since the extended attachment
points (tail, wings, jaw, …) were added that group is *them* and the HUD points
are group `9`, which the table does not know — so upstream's
`@getattachnames:hud` names the extended points and never a HUD surface.

## Extension commands

Three commands are in no dictionary: `@getdebug_<setting>=<channel>`,
`@setdebug_<setting>:<value>=force` and `@setrot:<radians>=force`. They arrive
as unknown keywords, and the reference picks them up afterwards through a chain
of registered handlers. `RlvState::run_extension` is that last stop — the
consumer sends it a command `apply` handed back as `NotAStateChange`, and gets
`None` if it is not one of the three either.

The debug settings are a closed **allowlist** of six rows
(`RLV_DEBUG_SETTINGS`), for the obvious reason: an object that could write any
debug setting could do anything the viewer can. Each row carries what may be
read, what may be written, and whether it is a *pseudo* setting — a computed
fact with no stored value. The parsing (C++'s prefix-stopping `operator>>`, its
closed list of boolean words), the formatting, and every rule about who may
touch what are here; the values come from an `RlvExtSource` the consumer
implements. `@setdebug=n` gives one object the writable rows, and
`is_debug_setting_locked` is what a settings editor asks so the user cannot
edit underneath it.

Three reference quirks are kept deliberately. The pseudo `AvatarSex` write is a
lie a script tells itself — stored verbatim, never reaching a settings store,
read back as the spelling that went in. `@setrot` is registered for the reply
dispatch too and checks neither, so `@setrot:1.5=2222` really does turn the
avatar and answers nothing. And a read of a setting the viewer does not have
still *succeeds*, with an empty answer, because a script that asked a question
must not be left waiting.

The grammar, the classification and the state machine follow Firestorm's
`rlvhandler.cpp`, `rlvhelper.cpp`, `rlvmodifiers.h`, `rlvextensions.cpp` and
`rlvdefines.h`
(`ERlvBehaviour`, `ERlvParamType`, `ERlvBehaviourModifier`, `RLV_CMD_PREFIX`),
reimplemented idiomatically rather than copied. Channel-0 owner-say gating is
the caller's job — this crate decodes a line it is handed.
