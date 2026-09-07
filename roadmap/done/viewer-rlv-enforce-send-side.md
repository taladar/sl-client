---
id: viewer-rlv-enforce-send-side
title: RLV — enforce send-side blocks at the Session boundary
topic: viewer
status: done
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Refuse the forbidden **outgoing** commands — the session, not the renderer. This
is the family that a **headless** `sl-client` bot must honour too, which is the
argument for putting the state model in a crate
([[viewer-rlv-restriction-state]]) and the choke points at the command boundary
rather than in Bevy systems. An
RLV-compliant viewer **must not offer a bypass**, so the check belongs at the
lowest choke point available — the `Session` command surface — never
re-implemented per call site.

The behaviours (`ERlvBehaviour`) each map to a command `Session` (or the
viewer's input path) must refuse to issue:

- chat: `@sendchat`, `@sendim` / `@sendimto`, `@sendchannel`,
  `@chatshout` / `@chatnormal` / `@chatwhisper`, `@emote`;
- teleport: `@tplm` / `@tploc` / `@tplure` / `@tprequest`;
- posture and attachments: `@sit` / `@unsit`,
  `@detach` / `@remoutfit` / `@addattach`;
- world interaction: `@rez`, `@edit`, `@touchall`, `@fly`, `@setgroup`, …

Mirror the reference façade shape exactly: a restriction is asked about at the
choke point via one predicate (`RlvActions::canX()` / `hasBehaviour()`), called
from all over `llviewer*`. Copy that — ask [[viewer-rlv-restriction-state]] at
the choke point.

Reference (Firestorm, read-only): `rlvactions.h` (`RlvActions::canX()` /
`hasBehaviour()`), `rlvhandler.cpp`.

## Parity-audit addendum (2026-08-19)

The audit's command-by-command mapping puts the following send-side
dictionary commands in this task's scope beyond the subset the body
names: the movement family `@jump`, `@alwaysrun`, `@temprun`; economy
`@buy`, `@pay`, `@share` (with `_sec`); the `@interact` blanket block;
the full touch granularity `@touchworld`, `@touchthis`, `@touchme`,
`@touchattach`, `@touchattachself`, `@touchattachother`, `@touchhud`,
and `@fartouch` (plus its `@touchfar` synonym) with the FARTOUCHDIST
distance modifier; `@sendgesture`; `@sittp`, `@standtp` and `@tplocal`
with the SITTPDIST / TPLOCALDIST distance modifiers;
`@sendchannel_except`; and the sendim / startim distance min/max
modifiers (SENDIMDISTMIN/MAX, STARTIMDISTMIN/MAX). The typed modifier
slots themselves come from [[viewer-rlv-restriction-state]].

## Done

`sl-rlv` grew `actions.rs` — `RlvActions`, the reference's `RlvActions` façade,
which is now the only thing a chat bar, a session or a build tool has to
consult. It borrows an `RlvState` and an `RlvActionSource` and answers one
predicate per question, so no call site can spell a restriction its own way.

**What the façade needs from outside** is small and synchronous, the way
`RlvQuerySource` is for the query layer: where the agent is, where an avatar is,
which link tree an object belongs to, whether the agent is sitting, whether an
IM session is already open, and which command is executing right now. The last
of those is what stops a restriction blocking the very command carrying it out
— an object holding `@fly=n` may still `@fly=force`, and one holding `@sittp=n`
may still `@sit:<uuid>=force` across the region. Four viewer settings
(`RestrainedLoveCanOOC`, `RestrainedLoveShowEllipsis`, `RLVaSplitRedirectChat`,
`RLVaShowRedirectChatTyping`) come with the reference's defaults so a headless
bot only has to answer the world questions.

**The object a question is about** is passed in as an `RlvObject` — id, link
root, kind, position, whether it is a prim — because this crate cannot walk a
scene. The kind (`World` / `AttachmentSelf` / `AttachmentOther { wearer }` /
`Hud`) is the three questions the reference asks an `LLViewerObject`, collapsed
into the four cases its branches actually distinguish. Keeping id *and* root
apart is load-bearing: touch and edit restrictions apply linkset-wide and match
on the root, while `@fartouch` measures to the clicked prim plus the pick
offset.

**Coverage**, against the task body and the parity addendum: chat (`@sendchat`,
`@emote`, `@sendchannel` / `@sendchannel_except`, `@chatwhisper` /
`@chatnormal` / `@chatshout`, `@sendim` / `@sendimto`, `@startim` /
`@startimto` with the SENDIM/STARTIM distance pairs, `@sendgesture`,
`@redirchat` / `@rediremote`); teleport (`@tplm`, `@tploc`, `@tplure`,
`@tprequest`, `@tplocal` and `@sittp` with their distance modifiers, plus the
`@standtp` interaction); movement (`@fly`, `@jump`, `@alwaysrun`, `@temprun`);
posture (`@sit`, `@unsit`); world interaction (`@rez`, `@edit` / `@editobj` /
`@editattach` / `@editworld`, `@interact`, the full touch granularity
`@touchall` / `@touchthis` / `@touchworld` / `@touchattach` /
`@touchattachself` / `@touchattachother` / `@touchhud` / `@touchme` with
`@fartouch` and FARTOUCHDIST); and economy (`@buy`, `@pay`, `@share`,
`@setgroup`). The `@detach` / `@remoutfit` / `@addattach` family the body lists
is not re-exposed here: "may *this* come off" is `RlvLocks`
([[viewer-rlv-locks]]), and a second way to ask it would be a second answer to
drift.

**Three decisions are not yes/no**, and they live at the same choke point
because the reference puts them there. `check_chat_volume` clamps one step per
restriction, so with `@chatnormal` in force a shout comes out whispered.
`filter_chat` is `RlvHandler::filterChat`: an emote is truncated to its first
sentence or twenty characters unless `@emote` allows it whole, but blanked
outright if it carries punctuation that could smuggle words through; a short
`/`-prefixed line survives so gesture triggers still fire; `((OOC))` survives
while the setting allows it; everything else becomes `"..."` or nothing.
`redirect_chat` is `redirectChatOrEmote`, including the rule that only chat
`@sendchat` *would have swallowed* is redirected — which is why
`RlvFilteredChat` reports "blanked" separately from the text it returns, since a
truncated emote is rewritten but not blanked. `outgoing_chat` composes all three
in the reference's order and is the single call a chat bar makes.

`filter_chat` is deliberately shared rather than send-only: the receive side
(`@recvchat`) calls it with `filter_emote = false`, which is the one filter
[[viewer-rlv-enforce-receive-side]] will reuse rather than reimplement.

One thing the reference reads as a bug and is not: `rlvCheckAvatarIMDistance`
compares a squared distance against the RECVIM/SENDIM/STARTIM distance
modifiers. Those slots hold squared metres — `rlvhandler.cpp:2260` writes
`nDistMin * nDistMin`, and this crate's state machine already stored them that
way — so the comparison is right and `within_im_range` skips the square root
too. The `@sittp` / `@tplocal` / `@fartouch` radii are *not* squared in storage,
and are squared at the comparison instead; both conventions are followed where
they apply.

## Verified

`cargo test --release -p sl-rlv` — 189 tests plus 8 doctests, green;
`cargo clippy --release -p sl-rlv --all-targets` clean.

52 of those tests are new and cover the façade: a restriction not blocking its
own issuer while a second holder still blocks everyone, the `@sittp` radius and
its modifier, a forced sit ignoring it, `@fartouch` measuring to the pick offset
rather than the object centre, `@touchall` sparing the HUD, `@touchme` from a
child prim re-opening its whole linkset while another is still shut out,
`@touchthis` and `@touchattachother` naming what they block, `@sendchannel` and
its inverse (including that naming a channel alone restricts nothing), IM
exceptions, `@sendimto` blocking what `@sendim` would allow, the exclusion range
as a hole in the block with an unseen avatar treated as infinitely far, an open
session surviving `@startim`, each volume clamp, every branch of the chat filter
(OOC, short slash command, emote truncation at the full stop, `@emote` letting
one through whole, smuggled punctuation, an arriving emote never shortened),
redirect to a named channel, a clean emote *not* being redirected, `@rediremote`
taking the emotes instead, a redirect to a `@sendchannel`-blocked channel still
swallowing the line, long-line splitting, the debug channel counting as public
chat, teleport home surviving `@tplm` alone, a locked seat blocking every
teleport, `@tplocal` capped at one region and ignoring height, the three
`RlvCheckType` answers, `@editattach` vs `@editworld`, `@interact` sparing the
HUD and not short-circuiting a null target, and an unrestricted state allowing
all of it.

Every behaviour keyword the task body and the parity addendum name is
referenced by the module — checked mechanically against the two lists rather
than by eye.

Not verified live: still no consumer in the workspace, as with every RLV task so
far. Driving this against a real scripted object comes with the first
integration.
