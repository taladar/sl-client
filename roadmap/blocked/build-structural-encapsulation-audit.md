---
id: build-structural-encapsulation-audit
title: Audit the workspace for structural and encapsulation improvements the crate split exposed
topic: viewer
status: blocked
origin: crate-split work (2026-08) — patterns the visibility pass kept surfacing
points: 8
refs: [build-split-viewer-crate]
blocked_by: [build-split-viewer-crate]
---

Splitting the viewer turned a style question into a mechanical one: the
compiler names every item used across a new boundary, so each crate's API
arrives as a list rather than a judgement. That list kept pointing at the same
handful of structural problems, and most were worked around rather than fixed
because the split commits were already large. This task is the pass that fixes
them, once the split is done and the boundaries have stopped moving.

## The flagship: plugins instead of exported systems

Sixty component types in `sl-viewer-world` are `pub` for one reason — they
appear in the signature of a system the viewer registers. `SunDisc`,
`WaterOcean`, `AvatarAnchor`, `TagContent` and the rest are the world's own
vocabulary, and nothing outside the crate names them.

The reason they had to be exported is that the viewer's `lib.rs` schedules the
systems itself, ordering them against each other and against systems in other
crates (`drive_water.after(position_camera)`,
`update_underwater_fog.after(drive_water)`). If each module shipped a `Plugin`
that declared its own ordering, the systems could be private and the
components with them.

The measurement to make first: of the ~620 items the split promoted, how many
are reachable only through an exported system signature? That is the size of
the prize, and it is also the honest test of whether module plugins are worth
the scheduling rework.

## The recurring patterns

Each of these was found in one place and fixed there. None was searched for
across the workspace.

- **A field that was only safe because it had one caller.** `MuteModel`
  exposed a `requested` latch the request system set by hand; it became
  `claim_request`, which tests and marks in one step so two requesters cannot
  race a duplicate onto the wire. `PresenceState` had seven such fields; the
  wire-edge pair became `take_away_edge` / `take_dnd_edge` for the same
  reason — a separated read and mark is how the advertised state silently
  stops matching the real one.
- **Read-then-mark that should be one step.** The general form of the above.
  Worth grepping for: a `self.advertised_x = self.x` or `self.pending = false`
  that a caller performs after acting.
- **A data model carrying presentation state.** `FriendsModel` held the
  People floater's persisted column sort, which is why the most-read state in
  the viewer could not move below the features. Lifting the sort into its own
  resource was what unblocked it.
- **Vocabulary separated from the state defined in terms of it.**
  `EditToolState` had a `GridFrame` field and a grid-unit default living in
  another module; `MatModeState`'s fields are indices whose meaning sat in
  `MATMEDIA_*` / `MATTYPE_*` constants elsewhere. State that cannot describe
  itself cannot move.
- **A predicate stated in the wrong tier.** `shows_autoresponse` — the rule
  that either autorespond mode counts — lived in `presence`, so a name tag had
  to reach up into a feature module to ask.

## Approach

1. Measure the plugin prize (above) before committing to the rework.
2. Grep for the read-then-mark and one-caller-field shapes; they are
   recognisable and the fix is local.
3. Re-run the visibility pass with the widener's `unreachable_pub` signal
   after each change: an item that can go back to `pub(crate)` is the proof
   the encapsulation improved.
4. Do not treat this as a rename pass. `module_name_repetitions` is expected
   crate-wide in five crates already and renaming to satisfy it would churn
   every call site for a style rule this codebase does not follow.

## Why after the split, not during

Every one of these is a design change to code whose boundaries are still
moving. Doing them inside a split commit mixes a mechanical move with a
semantic change, which is what makes such commits unreviewable — and the
scheduling rework in particular would have to be redone when `world` splits
four ways (step 18 of [[build-split-viewer-crate]]).
