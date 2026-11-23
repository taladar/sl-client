---
id: viewer-own-avatar-broken-after-teleport
title: Own avatar looks broken after a teleport
topic: viewer
status: done
origin: live aditi testing (2026-08-10, ground-probe session)
refs: [viewer-perf-avatar-ground-probe]
---

Context: [context/viewer.md](../context/viewer.md).

## Cause and fix (2026-11-23)

The own outfit's rigged meshes waited behind the rest of the region.
`apply_rigged_attachments` spends the per-frame `MeshUploadBudget` after the
object systems, stopped as soon as it was empty, and took pending meshes in
query order. While a busy region streamed in, the own attachments had arrived
and were seated within about 2 s but were not built for up to 35 s. A mesh-body
wearer's base body is hidden, so the avatar was partly or wholly missing until
then, with its name tag still drawn.

The own avatar's rigged meshes now build first and are not held to the budget.

Measured on aditi over 14 world-map teleports between Mauve and Ahern, as the
time from arrival until at least 40 of the outfit's 51 shown parts were drawn:

| | slowest | arrivals over 10 s | avatar missing in the 30 s frame |
| --- | --- | --- | --- |
| before | 34.7 s | 3 | 1 |
| after | 11.0 s | 1 | 0 |

The report predates these measurements and described the avatar as incomplete
rather than missing. A partly built outfit is the same fault seen earlier in
its load, but the original session cannot be replayed to prove it was this.

A separate stall found in these runs is
[[viewer-teleport-back-gets-no-answer]].

## Original report

Observed live on aditi: the **own** avatar looked **incomplete** after a
**cross-region teleport** (to a region with sit targets). Before the
teleport, in the initial region, the avatar and its **feet looked fine**
(standing). After the teleport the **height/position looked correct** — so
it is a **completeness** problem (missing parts / appearance), not a
position, ground-clamp or feet problem. Filed as a follow-up during the
ground-probe work.

## To confirm (exact symptom)

"Incomplete" — pin down which parts on the next repro:

- Missing body parts / attachments, un-baked (grey / default) skin,
  half-rezzed mesh, collapsed or T-pose skeleton, wrong scale?
- Does it recover on its own after a while (a slow re-bake / re-stream), or
  stay broken until a re-log?
- Own avatar only, or nearby avatars too?
- A screenshot at the broken state.

## Likely pre-existing (appearance re-establish after handover)

Height, position and feet were all fine — only **completeness** was wrong.
That points at the **appearance / bake / attachment re-establishment** after
the region handover, not the ground-probe or seated changes (which only
affect feet/pose/position, all of which looked correct). Most likely the
own avatar's COF / bake / attachments are not fully re-requested (or the
re-stream is incomplete) on the destination sim after a cross-region
teleport / `world_reset`.

Seen on a dev build carrying **uncommitted** ground-probe changes (the
avian path — **off** in this run — and the **seated fix**), so rule those
out cheaply: the seated fix would show as suppressed **foot IK** (feet), and
feet/height were fine, so it is an unlikely cause. Still worth a one-line
check that `AvatarState::is_seated(own)` is reset on `world_reset` (a
stale-true would suppress the own avatar's probe), but the symptom does not
match it.

**Isolate:** reproduce on committed `master` (no ground-probe changes at
all). If it repros there — almost certainly will, given the symptom — it is
a pre-existing teleport/appearance bug. Machinery to check: the scene purge
on `world_reset` (`sl-viewer-world-api/src/world_scoped.rs` and the
`WorldScoped` impls it collects), the own-avatar re-stream after handover
([[viewer-perf-avatar-ground-probe]] is unrelated), and the destination
sim's re-request of the bake/appearance and attachments (COF).
