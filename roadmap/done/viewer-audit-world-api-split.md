---
id: viewer-audit-world-api-split
title: sl-viewer-world-api is a 6892-line god-module and the workspace's shared-types dump
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 13
refs: [viewer-audit-world-api-query-tests]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-api/src/lib.rs` was one file: 7925 lines by the time this was
picked up, 168 `pub` items, 25 `Resource`s, **zero submodules and zero traits**,
5 tests.

What it genuinely abstracts is `WorldPhase` — a 10-variant `SystemSet` — and
that *is* honoured: every variant has both producers and consumers across the
layers. Everything else was shared concrete state.

Worse, it carried whole domains with nothing to do with the world:
`FriendsModel`, `GroupsModel`, `MuteModel`, `PresenceState`, and the notecard /
script / conference / browser open events. Their consumers are
`sl-viewer-people`, `sl-viewer-notices`, `sl-viewer-inventory` and
`sl-viewer-edit` — against **four total** from the five world crates. So 14
unrelated crates depended on the world layer to reach them.

## What was done

1. **Split along the banner sections the file already carried.** They were
   ready-made module boundaries nobody had taken: `edit_selection`, `settings`,
   `world_state`, `object_flags`, `world_vocabulary`, `phases`, `terrain`,
   `object_graph`, `object_components` (plus `social`, `intents`,
   `drag_drop` and `map_tracking`, which then left the crate). The crate root
   re-exports every module flat, so no downstream path changed for this step.
   The two sections that were really two subjects were separated on the way:
   `terrain` came out of "Ordering phases", and the five tests that had
   accumulated in one `mod tests` moved to the modules they exercise.
   Verified by a normalised token diff against `git show HEAD:` — nothing but
   `use` / `mod` / test-scaffolding lines added, and nothing lost.

2. **Lifted the social and intent halves out**, into two new crates rather than
   one, because they are two subjects:
   - `sl-viewer-social` — the mute list, the buddy list, the group
     memberships, the away / Do Not Disturb state and the map tracking.
   - `sl-viewer-intents` — the block / friend / profile / picker / conversation
     requests and the drag, drop and open-editor vocabulary.

   Both depend on `bevy` and `sl-client-bevy` and on nothing else in the
   viewer, and neither is reachable through `sl-viewer-world-api` any more.
   Two crates (`sl-viewer-environment`, `sl-viewer-search`) dropped their
   world-layer dependency outright; the rest keep one only for state that is
   genuinely about the world. Two items that had drifted into the intent
   section on the way past — `AgentRegionPosition` and
   `SETTING_AUTORESPONSE_ITEM` — went back to `world_state` and `settings`.

3. **The layering is now visible in the source** of the four world crates: the
   `pub(crate) use <lower crate>::<module>` alias blocks are gone (68 aliases,
   579 call sites), so a call site says `sl_viewer_kit::coords` or
   `sl_viewer_world_api::ObjectState` rather than a local-looking
   `crate::coords`, and a new reach-across has to be written out instead of
   inherited from the top of `lib.rs`. The 50 stale `crate::<module>` *prose*
   references left over from the original crate extraction — modules that had
   moved to the binary, to `sl-viewer-edit`, to `sl-viewer-map` and so on —
   were repointed at the same time.

   Every module in the three crates is `pub mod` with a flat `pub use` at the
   root, so the module docs are published (the payoff of the split) while
   `sl_viewer_world_api::ObjectState` keeps working.

## What was deliberately left

- The blanket `#![expect(clippy::module_name_repetitions)]`. Its stated reason
  still holds — the alternative is renaming `objects::ObjectState` and friends
  — and the same `expect` is house style across `sl-wire`, `sl-client-bevy`,
  `sl-marketplace` and `sl-viewer-platform`.
- The same alias blocks in the *feature* crates, and especially the 222 in
  `sl-client-bevy-viewer`. Those belong with
  [[viewer-audit-binary-module-extraction]], which moves the modules they
  alias; de-aliasing them here would be ~830 more call sites rewritten twice.
