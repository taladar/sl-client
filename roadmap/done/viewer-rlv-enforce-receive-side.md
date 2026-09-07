---
id: viewer-rlv-enforce-receive-side
title: RLV — receive-side chat/IM filters and redirect
topic: viewer
status: done
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Filter and redirect the **incoming/outgoing chat pipeline** according to the
restriction state ([[viewer-rlv-restriction-state]]). The chat pipeline rewrites
or drops messages, with **per-avatar exceptions**:

- receive filters: `@recvchat` / `@recvchatfrom`, `@recvim` / `@recvimfrom`,
  `@recvemote` — drop or hold what arrives from a blocked source, letting
  through the avatars named as exceptions;
- redirect: `@redirchat` / `@rediremote` — re-route what *you* say to a
  channel instead of open chat.

These sit on the message path (chat overlay + IM), consulting the state machine
per message and honouring the per-avatar exception sets it tracks. Keep the
check at the single filter chokepoint the reference uses so no receive path can
skip it.

Reference (Firestorm, read-only): `rlvhandler.cpp` (receive filters, redirect),
`rlvactions.h`.

## Parity-audit addendum (2026-08-19)

Missing scope found by the audit: the @recvim distance min/max modifiers
(RECVIMDISTMIN/MAX); the auto-accept behaviours `@accepttp` (with
`_sec`), `@accepttprequest` (with `_sec`) and `@acceptpermission`
(auto-accept the corresponding incoming dialogs); `@allowidle`
(suppress the away state); and the chat-filter refinements — OOC
handling (RestrainedLoveCanOOC), the ShowEllipsis "..." placeholder
text, and the RLVaSplitRedirectChat / RLVaShowRedirectChatTyping
behaviour of the @redirchat/@rediremote path.

## Done

`sl-rlv`'s enforcement façade grew its other half. `RlvActions` now answers
what a restriction lets **in** as well as what it lets out, in
`actions/receive.rs` — the same façade, so a chat overlay that already holds
one to ask whether it may say something does not have to build a second thing
to ask whether it may hear the answer. That is the shape the reference has:
`canReceiveIM` sits three functions below `canSendIM` in `rlvactions.cpp`.

**Incoming chat** is one call. `incoming_chat` takes who spoke, what kind of
thing they are (`RlvChatSource`) and how they said it (`RlvChatKind`), and
returns show / replace-with-this / show-nothing. Everything the reference
exempts before it even looks at a restriction is exempt here: the user's own
words, an attachment the agent owns, `llOwnerSay` and `llRegionSayTo` — the two
channels RLV itself travels on, which a `@recvchat` that swallowed them would
cut the collar's own commands off — and a typing indicator, which carries no
words. An object the viewer has not rezzed yet is a null `LLViewerObject*` in
the reference and `RlvChatSource::unknown_object()` here: neither owned nor
worn, so filtered.

The line itself goes through `filter_chat`, the send side's filter called with
`filter_emote = false` exactly as [[viewer-rlv-enforce-send-side]] anticipated
— so a short `/`-prefixed line and an `((OOC))` aside survive coming in for the
same reasons they survive going out, and an arriving emote is never shortened.
An emote is stopped by `@recvemote`, not `@recvchat`, and leaves `"/me ..."`
behind; blanked chat leaves `"..."`. Both vanish outright instead when
`RestrainedLoveShowEllipsis` is off, which is the reference's single choice for
both families.

**IMs** are censored rather than dropped: `incoming_im` returns
`Censor { tell_sender }`, because a user who may not read their IMs should
still know they have them, and the sender should know they are talking to a
wall. Offline messages and an exempt sender (the reference exempts Lindens, so
a restriction cannot cut the user off from support) are never touched.
`can_receive_im` honours the RECVIMDISTMIN/MAX pair the audit named, the same
squared-metre exclusion range as the send side. Group and conference invites
are the one place the two diverge and `incoming_session_invite` keeps that:
a group session is addressed by the group's own id and an invitation that is
not excepted is **declined outright** — joining a session the user may not read
would leak their presence into it — while a conference, addressed by whoever
started it, is only censored.

**Auto-accept.** `@accepttp` / `@accepttprequest` answer a teleport dialog
without asking, for everybody or for one named avatar; the nil id never matches
an exception, which the reference guards because it reaches this with ids read
back out of notification payloads. `@acceptpermission` shares its call site
with the two permissions RLV *refuses* outright, so `script_permission` answers
both from one question, in the reference's order: refuse first, grant second —
the only order that is safe, since a `@acceptpermission` must not hand out the
very attach permission an attachment lock exists to withhold. Taking controls
is always granted, attaching only for a rezzed object the agent owns, and
`notify_script_permission` says when the grant is announced in chat anyway
(somebody else's object taking the agent's controls silently would be a trap).

**Idling.** `@allowidle` does not suppress the away state so much as stop the
viewer volunteering it: `away_timeout_seconds` stretches the configured
`AFKTimeout` to half an hour and `clears_away_on_animation_stop` stops the away
animation ending from being read as "the user came back".

`RlvObject` gained `owned_by_agent` with an `is_owned_by_agent()` that folds in
what the kind already implies — anything worn on this avatar is this avatar's,
anything worn on somebody else is not. The script-permission question is the
first one that has to tell an owned rezzed object from an unowned one, which
`RlvObjectKind` alone could not.

Two pieces of the task body needed nothing new. `@redirchat` / `@rediremote`,
including RLVaSplitRedirectChat and RLVaShowRedirectChatTyping, are outgoing
chat and shipped with [[viewer-rlv-enforce-send-side]]; the OOC and ShowEllipsis
refinements came with the shared filter. The `hidden_generic` censoring of a
teleport-offer IM (`llimprocessing.cpp:2094`) is half `can_receive_im`, which
is now public, and half `@showloc`, which belongs to
[[viewer-rlv-enforce-info-hiding]] along with the rest of the anonymisation
layer.

## Verified

`cargo test --release -p sl-rlv` — 216 tests plus 9 doctests, green;
`cargo clippy --release -p sl-rlv --all-targets` clean.

27 of those tests are new: a blocked line becoming an ellipsis and vanishing
without the setting, an exception still being heard, `@recvchatfrom` blocking
only who it names, an emote stopped by `@recvemote` while `@recvchat` leaves it
alone, an arriving emote never shortened where the send side would have cut it
at the full stop, a short slash command surviving, the agent's own chat and an
owned attachment never filtered while a vendor and an un-rezzed object are, all
three never-filtered chat kinds reaching the command parser, a censored IM
telling its sender, offline and exempt IMs untouched, `@recvimfrom` blocking
what `@recvim` would allow, the distance range as a hole in the block with an
unseen avatar inside an open-ended one and outside a bounded one, a group
invite declined where a conference is censored, a group named as an exception
still joinable, an invite untouched while no `@recvim*` restriction is held,
`@accepttp` for everybody and for one (and never for nobody),
`@accepttprequest` as a separate answer, `@acceptpermission` granting controls
and owned attachments but still asking about somebody else's or an already-worn
one, an attachment lock refusing what it would have granted, `@tploc` refusing a
scripted teleport, the announcement rule, and both halves of `@allowidle`.

Not verified live: still no consumer in the workspace, as with every RLV task
so far.
