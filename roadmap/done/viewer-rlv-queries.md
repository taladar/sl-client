---
id: viewer-rlv-queries
title: RLV — answer @get* queries via chat reply
topic: viewer
status: done
origin: user request (2026-07); split from viewer-rlva-enforcement
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

Answer the RLV **queries** — the commands whose param is a number, meaning
"chat the answer back on this channel". The parser
([[viewer-rlv-command-parser]]) already recognises the query form and its
channel; this task gathers the answer from viewer / session state (and the
restriction state, [[viewer-rlv-restriction-state]]) and replies:

- `@version*` — the version handshake (reports 3.4.3 with a 2.9.28
  compatibility floor);
- `@getoutfit`, `@getattach` — worn wearables / attachment points;
- `@getstatus` — the current restriction set for an object;
- `@getinv` / `@getinvworn` — inventory listing / worn-folder state;
- `@getsitid` — the object currently sat on;
- `@getcam_*` — current camera parameters.

Replies go back with `RlvUtil::sendChatReply` on the given channel, **split
across multiple lines** when the answer is long. Some queries (`@version*`,
`@getstatus`) need no viewer state and can be answered from `sl-rlv` directly;
the rest read the relevant viewer/session snapshot.

Reference (Firestorm, read-only): `rlvhandler.cpp` (query dispatch),
`rlvcommon.cpp` (`RlvUtil::sendChatReply`).

## Parity-audit addendum (2026-08-19)

Missing scope found by the audit: `@getstatusall`; `@getcommand`
(dictionary introspection, including filtering on the EXPERIMENTAL /
EXTENDED / DEPRECATED behaviour flags kept by
[[viewer-rlv-restriction-state]]); the names-variant queries
`@getoutfitnames`, `@getattachnames`, `@getaddattachnames`,
`@getremattachnames`, `@getaddoutfitnames`, `@getremoutfitnames`;
`@findfolder` / `@findfolders`; `@getpath` / `@getpathnew` (worn item →
`#RLV` folder path); `@getgroup`; `@getheightoffset`; the `@getcam_*`
family (avdist, avdistmin, avdistmax, fov, fovmin, fovmax, textures);
RLVaEnableIMQuery (answer `@version` handshakes sent via IM); and
multi-line reply splitting via `RlvUtil::sendChatReply`.

## Done (2026-09-06)

`sl-rlv/src/query.rs` holds the query layer: `RlvQuery::classify` decodes a
reply-kind command and every option it can carry, and `RlvState::answer` builds
the `RlvReply { channel, message }` to shout. The split is drawn at **facts**,
not at formatting — every byte a script sees is built and unit-tested in the
crate, and only what the crate cannot know comes from the consumer through the
new `RlvQuerySource` trait.

Answered from the state machine alone: `@version` / `@versionnew`,
`@versionnum[:impl]`, `@getstatus` / `@getstatusall` (filter and separator both
parsed), `@getcommand` (filter, kind and separator), and the `@getcam_*` limits
`avdistmin` / `avdistmax` / `fovmin` / `fovmax` / `textures`, which read back a
modifier slot and answer empty — not the default — when nobody set one.

Answered from `RlvQuerySource`, formatted here: `@getoutfit[:<layer>]` (the
frozen 17-slot RLV order, body parts never hidden by `RLVaHideLockedLayers`),
`@getoutfitnames` / `@getaddoutfitnames` / `@getremoutfitnames`,
`@getattach[:<point>]` (the leading `0` that makes the bit string 1-indexed),
`@getattachnames` / `@getaddattachnames` / `@getremattachnames`, `@getinv`
(hidden and `/`-bearing folder names filtered), `@getinvworn` (the
`|<own><below>` digits, with the root's *below* summed from the children so two
places cannot disagree), `@findfolder` (deepest wins) / `@findfolders`,
`@getpath` / `@getpathnew` (option is a slot, a point, an id or the issuer),
`@getsitid` (nil id when standing), `@getgroup` (`none`, not empty),
`@getheightoffset`, `@getcam_avdist` and `@getcam_fov`.

Also new: `RlvAttachmentPoint` / `RlvWearableSlot` / `RlvAttachGroup` name
tables (the seam to the rest of the workspace is the wire index, so the crate
stays free of `sl-proto`); `is_valid_reply_channel`, `truncate_chat` and
`split_chat`; `RlvImQuery` for the IM surface (`@stopim`, `@version`, `@list`,
`@except`); and `RlvOutcome::Failed` / `RlvOutcome::FailedNoSharedRoot`.

Three reference decisions the tests pin:

- a query that **fails** is still answered, with an empty string, or the script
  waits forever. The only silent case is a channel a reply may not go on;
- the reply is *shouted*, so it is truncated at 1023 bytes, not split.
  `split_chat` is here for `@redirchat` ([[viewer-rlv-enforce-receive-side]]),
  which is the only caller of `sendChatReplySplit`;
- the whole owner-say line is lower-cased before parsing, options included, so
  a `#RLV` folder path arrives lower-case and the consumer has to match folder
  names case-insensitively while answering with their real casing.

One deliberate divergence, documented in the module docs and pinned by
`attach_group_matches_the_point`: `RlvAttachmentPoint::group` answers what the
point anatomically is. Firestorm derives it from `rlvAttachGroupFromIndex`,
which reads joint group `8` as the HUD group — but since the extended points
(tail, wings, jaw, …) were added that group is *them* and the HUD points are
group `9`, which the table does not know, so upstream's `@getattachnames:hud`
names extended points and never a HUD surface.

Not verified live: no consumer yet, same as [[viewer-rlv-notify]]. Wiring the
reply into the session's chat send, and implementing `RlvQuerySource` against
the viewer's appearance and inventory mirrors, belongs with the enforcement
tasks; `@getinv` / `@getinvworn` / `@findfolder` / `@getpath` additionally need
the `#RLV` shared tree that [[viewer-rlv-locks]] builds.
