---
id: viewer-region-options-estate
title: Region / Estate floater — estate tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
refs: [gridspec-aditi-test-land]
---

In progress (2026-07-28): the About Region **Estate**, **Covenant**, and
**Access** tabs are built. Estate access / limit / voice / teleport flags are
editable via the new `Command::SetEstateInfo` (`estatechangeinfo`, typed
`EstateFlags`); the four access lists (managers / allowed / allowed-groups /
banned) use the reusable `ui_table` widget with Add (avatar picker) + per-row
Remove over `UpdateEstateAccess`; covenant + estate identity are read-only (the
estate tab falls back to the covenant reply when `getinfo` is denied to a
non-manager). **Allowed Groups → Add** landed with
[[viewer-region-estate-group-picker]] (2026-09-12), which also made the rows
ask for the names of groups the agent is not in. **Not yet closed:** the write
paths are unverified on a live grid (needs an estate-owner login).

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater **estate** tab: covenant, access / allowed residents
and groups, estate managers, ban list, and region restart / sun controls — all
driven over the estate `EstateOwnerMessage`. Adds a tab to the floater shell
from [[viewer-region-options-debug]].

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`,
`llestateinfomodel`; the estate `EstateOwnerMessage`.

Builds on: `protocol-14` estate / region.

Deps: [[viewer-region-options-debug]].

## Parity-audit addendum (2026-08-19)

Parity-audit addition: the Covenant tab's **set-covenant** flow (drop a
notecard onto the covenant panel to set it) and the **Reset** button
(clear back to no covenant). Ours renders the covenant read-only, and
sl-proto has no `estatechangecovenantid` EstateOwnerMessage command yet
— the write path needs adding to sl-proto first (grep ChangeCovenant:
none in `sl-proto/src/command.rs`).

## From gridspec-estate (2026-10-17)

The Access tab reads the lists over `EstateAccess` and shows a ban's last
login, date and banner. The estate tab's message button now sends the
estate-wide `instantmessage` (`Command::SendEstateMessage`), as the reference's
does; the region-wide "Send message to region" (`simulatormessage`,
`Command::SendRegionMessage`) has no button yet. `Command::SetEstateCovenant`
now exists in sl-proto for the Covenant tab's drop-a-notecard and Reset.

## From gridspec-region-info (2026-10-17)

`EstateFlags` is merged into `RegionFlags` (the reference's one flag space);
the estate draft is a `RegionFlags`.

## Done (2026-11-05)

- **Covenant: set and Reset.** The Covenant tab is a drop target for one
  notecard (a new generic `ItemDropTarget` / `ItemDropped` pair in
  `sl-viewer-intents`, resolved by the inventory drag after the notecard
  editor's own target), taken only while the agent may manage the estate.
  `EstateChangeCovenant` is asked first; on OK the notecard is read through
  its item (`FetchInventoryItemAsset`), as `loadInvItem` reads it, and only a
  body that reads is set (`SetEstateCovenant`) and shown at once — a marked
  Linden notecard has to decode (`ProblemImportingEstateCovenant`), plain
  text is a version-0 notecard and passes, and a refused read names the
  reference's notice (`MissingNotecardAssetID` / `NotAllowedToViewNotecard` /
  `UnableToLoadNotecardAsset`). **Reset** asks the same confirmation and
  sends the nil id. Nothing is sent when the covenant already is the one
  asked for (`sendChangeCovenantID`), nor after the agent has left the region
  the prompt was about.
- **The covenant is read over the estate transfer** now
  (`FetchEstateCovenantAsset`), as `getEstateAsset` reads it; the tab used to
  ask `ViewerAsset`, which Second Life refuses for every notecard. A refused
  read says so instead of "loading" for good, and the body is read once per
  covenant rather than on every summary.
- **Send Message To Region…** on the Region tab: the `MessageRegion` prompt,
  then `SendRegionMessage` with what was typed.

Left as it was: the estate tab's message stays an inline field beside its
button rather than the reference's `MessageEstate` prompt, and Reset follows
the managers' gate (the reference also enables it for a god without estate
rights). The Second Life write paths are [[gridspec-aditi-test-land]]'s.

### Verified

`cargo test --release -p sl-viewer-places -p sl-viewer-inventory
-p sl-viewer-intents` — the covenant read rule and the refusal notices, and
the one-item drop rule (one item of the type, enabled, else refused).

`cargo test --release -p sl-client-bevy-viewer --test e2e_windows covenant` —
one fake-grid stage through the real binary: an estate whose covenant is set
shows the stock covenant body read over the estate transfer; the seeded
notecard dragged from the inventory onto the tab is confirmed, and the grid
hears `estatechangecovenantid` with that notecard's asset while the tab shows
its body; Reset, confirmed, sends the nil id and the tab says there is none;
and the Region tab's prompt sends the typed text as `simulatormessage`'s last
parameter.
