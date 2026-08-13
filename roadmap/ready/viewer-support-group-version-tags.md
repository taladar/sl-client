---
id: viewer-support-group-version-tags
title: Support-group chat version tags (send + display)
topic: viewer
status: ready
origin: User request during viewer-about-floater planning (2026-08-13)
blocked_by: [viewer-about-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm helps support-group staff by tagging each message a user sends
in a *designated* support or testing group with the sender's viewer
details, in-band, as ordinary message text. Port both sides:

**Transmission side.** When sending group chat in a designated group,
insert a plain-text tag at the front of the message (after a leading
`/me` if present). The reference tag is a parenthesised prefix
concatenating, without separators unless noted: the address size, an
OS letter (`W`/`L`/`M`), an AVX2 marker (plus or minus), a space, the
version, a space, a skin letter, a text-mode `T`, an RLV `*`, and an
`os` suffix on OpenSim builds — e.g. `(64L+ 7.1.9 d*) `. Support
groups get the short `Major.Minor.Patch` version (or
`pre-Release`/`Unofficial`/`Streaming` for non-release channels),
testing groups the full build version. Ours should carry the viewer
name/version (build metadata from `viewer-about-floater`), an OS
letter, and a grid-variant tag; keep the shape recognisable to support
staff reading mixed-viewer chat. Gate it behind a preferences toggle
(reference: separate support/testing chat-prefix settings).

**Group designation.** Firestorm downloads its support/testing group
UUID sets from its FSData service (`fsdata.cpp`, `mSupportGroup`); we
have no such service, so the group-UUID lists must be a user-editable
preferences setting, shipping the known Firestorm support-group UUIDs
as defaults.

**Display side.** Tags are in-band text, so other users' versions show
automatically in group chat. This task additionally covers recognising
the tag pattern in incoming group-chat messages for optional UI
affordances (e.g. highlight or tooltip on the parsed viewer/version)
and degrading gracefully when a message carries no tag.

Reference (Firestorm, read-only): `fsfloaterim.cpp` send path (~lines
420-535: tag assembly + support/testing branches), `fsdata.cpp`
(`isSupportGroup` / `isTestingGroup`, group sets).

Builds on: build-time viewer version metadata from
`viewer-about-floater`; group chat send/receive; preferences UI.
