---
id: viewer-rlv-command-intake
title: RLV — the owner-say command intake and the reply/notify chat path
topic: viewer
status: done
origin: gap found wiring viewer-rlv-debug-settings-commands (2026-09-07)
refs: [viewer-rlv-command-parser, viewer-rlv-queries, viewer-rlv-notify,
  viewer-rlv-debug-settings-commands, viewer-rlva-floaters-toggles]
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

**Nothing an object says reaches the RLV engine yet.** The whole engine is
built — the parser, the state machine, the locks, the queries, the enforcement
façade both ways, the extension commands — and the only thing that has ever
fed it a command is the RLVa console, with the agent itself as the issuer. A
worn collar cannot restrain this viewer, because the door is not cut.

The door has two halves, and every `sl-rlv` task so far has had to write "not
verified live: no consumer yet" because neither exists.

## In: the owner-say gate

Route arriving chat into [[viewer-rlv-command-parser]] under the reference's
own conditions, and swallow what it takes:

- the chat is `CHAT_TYPE_OWNER` on channel `0` (`llOwnerSay` / `llRegionSayTo`),
  which is what makes it an object and not an avatar;
- the line starts with `@` (`sl_rlv::is_rlv_line`), and is then *not* shown in
  the chat log — the viewer eats it;
- each command is applied with the issuing object's key, so
  `RlvState::clear_object` on detach lifts exactly what that object held. That
  key is also what the attachment bookkeeping
  (`RlvState::set_object_attachment`) needs, so the Restrictions floater can
  say where a restriction came from;
- `RestrainedLoveDebug` echoes every processed command into the RLVa console,
  which is the setting's whole purpose and today has nothing to echo
  ([[viewer-rlva-floaters-toggles]]).

### Who is allowed to speak — read this before tightening the gate

The obvious guess is that RLV only listens to **attachments**, and that an
in-world object has to go through a relay. That is the *content* ecosystem's
shape, not the viewer's rule, and building the gate from the guess would break
every in-world RLV device. What the reference actually tests
(`llviewermessage.cpp:3142`) is:

```text
RLV enabled && message starts with '@' && length > 3 && CHAT_TYPE_OWNER &&
    ( no chatter object || !isAttachment || !isTempAttachment
      || RLVaEnableTemporaryAttachments )
```

That trailing clause is an **or**-chain, so it excludes exactly one case: a
*temporary* attachment while `RLVaEnableTemporaryAttachments` is off. A rezzed
in-world prim is not excluded at all — a bed, a cage or a cuff-post the agent
owns commands the viewer directly, and always has.

**The security boundary is ownership, and the simulator draws it, not the
viewer.** `CHAT_TYPE_OWNER` is what `llOwnerSay` produces, and the simulator
delivers it only to the object's owner. Somebody else's furniture physically
cannot reach this agent's chat as owner-say, so there is nothing for the viewer
to check — which is why the reference does not re-check ownership here, and why
this task must not invent a check that pretends to add safety.

**That ownership boundary is exactly why relays exist.** A club's poseball is
owned by the club, so it cannot speak RLV to a visitor. Instead it asks a
scripted **relay** the visitor wears — which the visitor owns — on the RLV
relay channel (`-1812221819`), and the relay re-issues the commands as its
own `llOwnerSay`. The consent (ask / auto / off, per-object permission) lives
in the relay script, where the user installed it. All of that is content: the
viewer only ever sees the relay's own owner-say and cannot tell it from any
other worn object, so **there is nothing to implement here for relays** — but
the gate must stay loose enough that a relay, and an owned in-world object,
both get through.

Two things the reference does gate on, which this task does own:

- **blocked objects** (`RlvHandler::isBlockedObject`, `m_BlockedObjects`): a
  temporary attachment that could not be resolved is remembered by name, and
  every command from it is refused with `RLV_RET_FAILED_BLOCKED` — except a
  remove or a `@clear`, which are always let through so a block can never
  strand a restriction. `RlvOutcome` has no variant for this yet.
- **expiry for something that never detaches.** An in-world object has no
  detach event to fire `RlvState::clear_object`, so the reference garbage
  collects every 30 seconds: an object that once existed in the object list and
  now does not is cleared at once, and one that never resolved is cleared after
  20 misses (about ten minutes). Without this, a prim that was derezzed or left
  behind in another region restrains the agent forever — which is the one
  failure mode of this whole family a user cannot get out of.

## Out: the chat-back path

Three producers build a line to chat and nothing sends it:

- `RlvState::answer` — the `@get*` reply, **shouted** on the channel the query
  named ([[viewer-rlv-queries]]);
- `RlvState::take_notifications` — the `@notify` subscribers' lines
  ([[viewer-rlv-notify]]);
- `RlvState::run_extension` — the `@getdebug_*` answer
  ([[viewer-rlv-debug-settings-commands]]).

All three hand back an `RlvReply { channel, message }`, already truncated and
channel-checked; what is missing is the one system that drains them into the
session's chat send. It is one seam for all three, which is why it belongs
here rather than in each of them.

Doing this is also what makes every `RlvQuerySource` / `RlvActionSource` /
`RlvExtSource` implementation *observable*: today they are exercised only by
unit tests and the console, and the "not verified live" note on half the RLV
family clears the moment a scripted object can talk to the viewer.

Reference (Firestorm, read-only): the chat hook in `llviewermessage.cpp`
(~L3140, the `CHAT_TYPE_OWNER` case), `RlvHandler::processCommand` and the
garbage collector / blocked-object list in `rlvhandler.cpp`,
`RlvUtil::sendChatReply` in `rlvcommon.cpp`.

## Done

The door is cut, both ways.

**In.** `sl-viewer-rlv::intake` is the owner-say gate. Every arriving
`ChatReceived` that `sl_viewer_world_api::rlv::swallows_owner_say` claims —
`CHAT_TYPE_OWNER`, starting with `@`, RLV on — is fed to the state machine with
the **speaking object's key** as the issuer, so `clear_object` on its
disappearance lifts exactly what it held. Where the object sits on the avatar is
resolved from the world mirror and cached on the state machine the first time it
is known (`object_attachment` chases the speaking prim up its linkset to the
attachment root, because a bare `@detach=n` locks the *attachment*), which is
what lets the Restrictions floater say where a restriction came from and the
locks layer know what it locks.

**Swallowed in one place.** The predicate lives in the world-API tier and all
four surfaces that could leak the line ask it: the chat overlay, the Nearby
transcript, and — through the new `ChatLogConfig::swallow_rlv_commands` and
`swallows_rlv_command` — the on-disk transcript. The chat-log flag follows the
`RestrainedLove` master switch, pushed at login, on a preferences OK, and on a
flip of the switch itself (`push_chat_log_config_on_rlv_toggle`), because the
RLVa menu owns that switch and the preferences tab does not.

**Out.** `RlvSession` grew a bounded **reply queue**. All three producers now
hand back the same `RlvReply` — `RlvNotification` gained a `From` conversion
that truncates like the other two — and one system, `drain_rlv_replies`,
shouts them on the channel each names, refusing channel `0` out loud rather
than saying a script's answer into local chat.

**Queries are answered, honestly and by halves.** `RlvQuery::needs_source` is
new in `sl-rlv` and splits the language at *facts*: `@version` / `@versionnew` /
`@versionnum`, `@getstatus` / `@getstatusall`, `@getcommand` and the
`@getcam_*` limits read nothing outside the state machine and are answered in
full — which is what makes a collar's opening handshake succeed and the whole
family reachable. Everything that reads the avatar, the inventory or the camera
is **not** answered, and says so on the console instead. `RlvNoFacts` is the
source that arrangement is passed (and is what the crate's own doctests are
written against now, replacing a hand-rolled twenty-method stub).

**Expiry.** `RlvObjectWatch` walks the restricting objects against the world
mirror every two seconds. An object that was streamed and is now gone is
collected after three strikes; one that was never streamed gets the reference's
own ten minutes. Two deliberate divergences, both forced by this viewer's
shape:

- the reference collects on a **30-second** timer because it walks the whole
  object list; this walks the handful that hold restrictions, so it can look
  far more often — and must, because thirty seconds of restraint after the
  collar came off is thirty seconds nobody agreed to;
- the reference expires a once-seen-now-missing object **at once**. This viewer
  purges its object mirror *wholesale* on a distant teleport, so that rule
  alone would strip every restriction on every teleport. `world_reset` forgives
  every watched object a minute of absence, and the three strikes cover an
  ordinary re-stream hiccup.

`RestrainedLoveDebug` finally has something to echo: every command an object
processed, on the same INFO / ERR streams the console uses, honouring
`RLVaDebugHideUnsetDuplicate`. One divergence there: with the setting on the
reference stops swallowing and rewrites the line into a nearby-chat entry
(`<object> executes: @…`); here the echo goes to the RLVa console, which is
where somebody watching RLV traffic already is, and the chat surfaces stay clean
either way. `@setrot` from an owner-say turns the avatar through the same
`AvatarControls::forced_heading` the console writes.

## The master switch moves mid-session, and says so

The reference needs a **restart** for `RestrainedLove` to take effect: its
toggle raises a `GenericAlert` reading "RLVa will be enabled after you restart"
and the menu item wears a `(pending restart)` suffix until you do. Here the
switch takes effect at once, which is friendlier and leaves the user in a state
the reference cannot reach — so both directions now raise a notification, since
each has a consequence that otherwise arrives looking like a bug:

- **on** (`RLVaToggledOn`) — a device asks whether the viewer speaks RLV *when
  it is attached*. Anything worn while the switch was off asked, was told no,
  and will not ask again: it has to be re-attached, however correctly the
  viewer now behaves;
- **off** (`RLVaToggledOff`) — **everything held is released**
  (`RlvSession::release_all`: every object's restrictions, exceptions, locks,
  modifier slots and `@notify` subscriptions, and any unsent reply), and the
  intake stops taking owner-say lines, so the `@`-commands a still-worn device
  keeps issuing stop being swallowed and start appearing in chat as ordinary
  text.

The release is what makes "off" mean off. The reference reaches exactly this
state by **restarting**, which is what its own switch demands; applying the
switch at once without releasing would invent a state the reference cannot
reach — still restrained by a collar, with the RLVa windows that could show it
greyed out because RLV is off. The unsent replies go with it: a `@notify`
subscriber being told its restriction was lifted, by a viewer that has just
stopped speaking RLV at all, is a message about a conversation that has ended,
and the reference's restart likewise tells nobody. The **console transcript
survives**, because it is the log of what happened rather than part of the
state.

Two catalogue entries with no reference counterpart — there was nothing to warn
about there, because nothing had happened yet. `Alert` rather than `AlertModal`
(it must be acknowledged, but it reports something already done rather than
blocking on a decision) and `unique`, so flipping twice leaves one card.
`observe_master_switch` is the pure half: the **first** look of a session is
never a toggle, so logging in with RLV already on greets nobody with a card
saying it was just turned on.

## Not done — and why

- **The blocked-object list** (`RLV_RET_FAILED_BLOCKED`) is not implemented and
  `RlvOutcome` did not gain a variant for it. Its *only* producer in the
  reference is `RlvHandler::onExperienceAttach` — a temporary attachment rezzed
  under an experience not on the allowed list. This viewer has no
  experience-attach event and no experience allow-list, so nothing could ever
  put an entry in the list: it would be a refusal path with no way to reach it.
  It is [[viewer-rlv-blocked-objects]], blocked on the experience-event stream
  ([[viewer-experience-event-stream]]) that would feed it.
- **The temporary-attachment exclusion** in the gate is not enforced. It is the
  one case the reference's or-chain excludes, and telling a temp attachment
  apart needs the attachment's `AttachItemID` name-value, which this viewer does
  not keep on its tracked objects. `RLVaEnableTemporaryAttachments` defaults to
  **on**, so the default behaviour matches the reference's default exactly; only
  a user who turned it off gets a divergence. It is
  [[viewer-rlv-temp-attachment-gate]], which is ready — the item id is already
  on the wire, the viewer just does not keep it.
- **The `=force` actions** an object commands (`@sit`, `@tpto`, `@attach`,
  `@remoutfit`, …) still come back as not-a-restriction and are reported, not
  performed. That is [[viewer-rlv-enforce-forced-actions]], which is blocked on
  the sit/stand path and the `#RLV` folder tree.
- **The fact-reading queries** are still unanswered — see above. Wiring
  `RlvQuerySource` against the appearance, inventory and camera mirrors is the
  integration [[viewer-rlv-queries]] describes, and part of it needs the `#RLV`
  tree ([[viewer-inventory-folder-tree]]) and the hover height
  ([[viewer-agent-hover-height-ingest]]) that do not exist yet.

## Verified

`cargo clippy --release --all-targets --workspace` clean. `cargo test --release`
over the ten touched crates green: sl-rlv 235 + 12 doctests (including the
`needs_source` split and the factless source's two rules), sl-viewer-rlv 48 —
14 of them the intake's: the restrain / attribute / lift round trip, the
handshake answer, the honest silence on a fact-reading query, the extension read
and the `@notify` report arriving on the one seam, and the expiry arithmetic
including the world-reset grace — sl-viewer-world-api 24, sl-proto 352 + 455 +
77 + 124 + 16, sl-repl 193 + 7, sl-viewer-chat 29, sl-viewer-people 114,
sl-viewer-preferences 69, sl-client-bevy 85 + 4, sl-client-bevy-viewer 294 + 10.

Two knock-ons the new `ChatLogConfig` field caused, both mechanical: the REPL's
`set_chat_log_config` grew a `swallow_rlv_commands=` keyword (its `to_config`
path already layered over the defaults), and the struct crossed clippy's
four-bool line, so it carries an `expect` saying why four independent user
preferences are not a state machine.

A **fake-grid tier** covers the whole surface end to end:
`sl-viewer-rlv/tests/fake_grid_rlv_commands.rs` logs a headless app running the
real `SlClientPlugin` and `RlvIntakePlugin` into an in-process `sl-fake-grid`,
has an object `llOwnerSay` at it over real UDP, and asserts on what comes back
out of the socket — the `@version` handshake shouted on the channel it named
(and shouted, not said), the restrictions attributed to the collar's key, the
`@notify` reports of both the add and the remove, the `@getstatus` answer, and
the honest silence plus console line on `@getattach`. A second test flips the
master switch mid-session: the line said while it was off is neither obeyed nor
swallowed *and is not replayed* when the switch moves, and the next line is —
which is what makes it a test of the gate rather than of an absent intake. Both
were checked against a mutation (the intake plugin removed) and both fail. The
switch test also pins the two toggle notifications and the release: off (the
line is not obeyed, not answered and not swallowed), on (the earlier line is not
replayed, and the next one *is* obeyed and answered), off again (every
restriction released). The release assertion was mutation-checked too. The first
test pins that an ordinary session raises **neither** notification.

Not verified live: no worn object has spoken `@` to this viewer on a real grid
yet.
The two live checks worth doing are a collar's `@version` handshake being
answered on aditi, and a rezzed prim's restriction being released by the expiry
pass after it is derezzed.
