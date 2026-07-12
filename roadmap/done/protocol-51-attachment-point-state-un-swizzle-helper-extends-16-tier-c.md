---
id: protocol-51
title: Attachment-point state un-swizzle helper (extends #16, Tier C)
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**51. Attachment-point `state` un-swizzle helper (extends #16, Tier C). ✅
Done.** The object `state` byte is passed through verbatim (no data loss), but
for an *attachment* OpenSim/SL send a **swizzled** attachment-point value
(`((st & 0xf0) >> 4) + ((st & 0x0f) << 4)`, OpenSim `LLClientView.cs`
~7208/7454/7730) in that same byte. A consumer reading `Object::state` as the
attachment point gets the wrong value unless it un-swizzles. Added two
documented accessors. **`Object::attachment_point_id() -> Option<u8>`** reverses
the nibble-swap — the reference viewer's `ATTACHMENT_ID_FROM_STATE`
(`indra_constants.h`, the macro `((st & 0xf0) >> 4) | ((st & 0x0f) << 4)`) — and
strips the transient `ATTACHMENT_ADD` (`0x80`) bit, returning the plain
attachment-point id (`1` = chest, `6` = right hand, `35` = HUD center 1).
**`Object::attachment_point() -> Option<AttachmentPoint>`** decodes that id into
the shared **`sl_types::attachment::AttachmentPoint`** enum (via its
`from_repr`, whose discriminants already match the wire ids), giving a named
point — covering both avatar points (`AttachmentPoint::Avatar`, e.g. chest,
right hand) and HUD points (`AttachmentPoint::Hud`, e.g. top-left, center) in
one value. Both return `None` for anything that is not an attachment, mirroring
the viewer's `LLVOVolume::isAttachment` (`mAttachmentState != 0`): a plain prim
(`state == 0`) and trees/grass (whose `state` byte instead carries the species,
so they are excluded by `pcode`). The typed form also returns `None` for any
id the enum does not yet name — those remain reachable via the lossless
`attachment_point_id`. The raw
`Object::state` field now carries a doc note pointing at the accessors and
explaining the per-`pcode` meanings. Backed by a small
`const fn attachment_point_from_state` helper and an `ATTACHMENT_ADD` constant;
available through both runtimes via the re-exported `Object` type with no
further wiring (`AttachmentPoint` is reached from `sl-types` directly, as #38's
geometry types are). Covered by three new `types.rs` unit tests
(`attachment_point_unswizzles_state_nibbles` — chest/right-hand nibble swaps,
the raw id, and the `ATTACHMENT_ADD`-strip case as both id and enum;
`attachment_point_decodes_hud_points` — a HUD id surfaces as both the raw id and
a typed `AttachmentPoint::Hud`; and `attachment_point_none_for_non_attachments`
— plain prim, tree, grass). *Unit-tested only: the transform is a deterministic
bit-swizzle cross-checked against both the OpenSim encoder and the viewer
decoder, operating on the `state` byte that #16/#50 already surface correctly;
a live exercise would need to attach an inventory object to the avatar (an
attach flow this headless client does not drive). Test: local OpenSim
(rez/attach an object).*

**Server-side protocol support (2026-06-18) — #52–#65, Tier F.** Everything
above is the *client* direction of the protocol: the workspace encodes what a
viewer sends and decodes what a simulator sends. Tier F adds the **server** side
so `sl-wire`/`sl-proto` can act as the *other* peer — a complete
bidirectional protocol library plus a sans-I/O skeleton per grid server role.
The generated LLUDP message codec is already symmetric (`build.rs` emits both
`encode_body` and `decode_body` for all 483 messages, and the
framing/ack/zerocode layer is direction-agnostic), so Tier F is *not* about
that layer. The work is the one-directional **hand-written sub-codecs** —
every bespoke binary blob and CAPS/LLSD payload currently has only the client
direction — plus the per-role state skeletons that have no equivalent today.
Each item is the literal inverse of an existing decoder/encoder (the existing
direction is the spec), validated by round-trip tests with that counterpart as
the oracle. The grid is several distinct servers, so the skeleton is split by
role (login server vs. simulator vs. the CAPS/grid services) rather than one
monolithic "server". Story points and the "Test" column follow the same
convention as the other tiers; most items are
unit round-trip tests (no live grid), and the `SimSession` is exercised by an
in-memory loopback against the existing client `Session`.
