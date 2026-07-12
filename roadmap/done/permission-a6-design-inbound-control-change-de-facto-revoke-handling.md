---
id: permission-a6
title: Design inbound control-change & de-facto revoke handling
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A6. Design inbound control-change & de-facto revoke handling.** Specify
    how `ScriptControlChange` (already `Event::ScriptControlChange`) updates
    the taken-controls registry (a `Take` adds, a `Release` removes), and
    record that a `ScriptControlChange(release)` is the only revoke the sim
    pushes (no inbound `RevokePermissions`). Decide whether a client-sent
    `release_script_controls` / `RevokePermissions` updates the mirror
    immediately. **Done — see § Inbound control-change reference (from A6) +
    task B3 in § Phase B.** Decided: the
    taken-controls tracker is a **session-global** `TakenControls` field (no
    holder/object attribution — `ScriptControlChange` carries no object id),
    modelled exactly as the viewer's per-control-bit
    **counts split by `PassToAgent`** (Firestorm `mControlsTakenCount` /
    `mControlsTakenPassedOnCount`): a `Take` block increments each named bit's
    count, a `Release` block saturating-decrements it (removed at 0), so two
    scripts taking the same control survive one releasing — the count model is
    required, a single `ControlFlags` union would not suffice. The inbound
    `ScriptControlChange` handler folds every block into the tracker
    **in addition to** still emitting `Event::ScriptControlChange` (the driver
    routes inputs). `ScriptControlChange(release)` is recorded as the **only**
    revoke the sim pushes (verified: no inbound `RevokePermissions`; OpenSim's
    detach / script release echo a release `ScriptControlChange`, see §
    Inbound control-change reference). Client-sent updates:
    `release_script_controls` clears the tracker to empty
    **immediately on send** (B3; does not wait for the echo — OpenSim's echo
    is `Controls = 0xFFFFFFFF, PassToAgent = false`, which would miss the
    passed-on counts, so clear-on-send is the robust choice and the later
    echo's clamped decrement is a harmless no-op); `RevokePermissions` does
    **not** touch the tracker (it honours only the animation bits, never
    controls — verified in OpenSim's `HandleRevokePermissions`). The
    A3-reserved `script_controls()` accessor return type is finalized as a
    public `ScriptControlsInfo` view (two `ControlFlags` unions, `taken` /
    `passed_to_agent`; counts stay private).

## Inbound control-change reference (from A6)

The *taken-controls* tracker — the second of the two permission stores (the
grant registry is A2; this one is agent-global). It mirrors which movement
controls scripts are currently holding, fed by the inbound `ScriptControlChange`
and cleared by `release_script_controls`. The simulator stays authoritative;
this is an API-convenience mirror.

**Why it is session-global and count-based (the protocol constraint).**
`ScriptControlChange` (wire `Low 189`) carries **no object/holder id** — only a
`Data` array of `{ TakeControls: BOOL, Controls: U32, PassToAgent: BOOL }` (see
§ Protocol reality). So taken controls cannot be attributed to a holder and do
**not** live in the per-script grant registry (A2); they are a separate
session-global field. Firestorm tracks them as a **per-control-bit count**, in
two arrays split by `PassToAgent` — `mControlsTakenCount` (consumed; the avatar
does not move from the input) and `mControlsTakenPassedOnCount` (also passed to
the agent) — incremented on a take block, decremented (clamped at 0) on a
release block (`LLAgent::processScriptControlChange`). The count model is
**required**, not cosmetic: two scripts may take the same control bit, and one
releasing must not clear it for the other; a single `ControlFlags` union would
lose that. The mirror reproduces this exactly.

**The field** (on `Session`, in `session.rs`, beside `script_grants` / `sit` /
`teleport`, private, reached only through the accessor):

    taken_controls: TakenControls

    struct TakenControls {
        /// Per-control-bit take count for controls the script *consumes*
        /// (PassToAgent = false; the avatar does not move from the input).
        consumed: BTreeMap<u32, u32>,    // single-bit mask -> count
        /// Per-control-bit take count for controls *also* passed to the agent
        /// (PassToAgent = true).
        passed_on: BTreeMap<u32, u32>,
    }

- Keyed by the **single-bit mask** (`u32`, e.g. `0x1`, `0x2`, …), not by
  `ControlFlags` (which derives no `Ord`) — `BTreeMap` keeps the crate's
  deterministic-iteration convention and a sparse map (entries removed at 0) so
  "is this control taken" ≡ "key present". The split mirrors the viewer's two
  arrays; an `iter_bits(controls: ControlFlags) -> impl Iterator<Item = u32>`
  helper (yield each set bit as its own mask) replaces the viewer's
  `for i in 0..TOTAL_CONTROLS { if controls & (1<<i) }` loop without raw
  indexing (clippy restriction lints).

**Inbound update — folding `ScriptControlChange`.** The existing handler
(`session/methods.rs:2676`) already parses each block into a `ScriptControl` and
emits `Event::ScriptControlChange(controls)`. A6 adds, *in the same handler*
(keeping it the single update site), a fold of each block into `taken_controls`
**before** pushing the event (the event is still emitted unchanged — the driver
routes the actual inputs; A7 confirms the inbound event surface is unchanged):

- pick the map by `block.pass_to_agent` (`passed_on` if set, else `consumed`);
- for each set bit in `block.controls` (`iter_bits`): on
  `ScriptControlAction::Take`, `*entry.or_insert(0) += 1`; on `Release`,
  decrement and **remove** the key when it reaches 0 (saturating — never go
  negative, matching the viewer's clamp; a release for an untracked bit is a
  no-op).

A take and a release block can arrive in the same message; they are applied in
order. A `ScriptControlChange` never touches `script_grants` — "permission
granted" (registry) and "controls currently taken" (this tracker) stay separate
(the `TAKE_CONTROLS` grant persists across a release; the script may re-take).

**De-facto revoke — the only revoke the sim pushes.** There is **no** inbound
`RevokePermissions`; a `ScriptControlChange(release)` is the *only* control
revoke the simulator pushes, and the tracker self-corrects from it. Verified in
OpenSim: a per-script release / detach
(`ScenePresence.UnRegisterControlEvents- ToScript`, reached via
`UnRegisterSeatControls` on detach) sends **two** release blocks for that
script's controls — `PassToAgent = false` *and* `true` — so both maps decrement;
this is the same `ScriptControlChange(release)` echo A5 relies on to
self-correct the tracker on detach (no dedicated detach hook needed).

**Client-sent updates to the mirror.**

- **`release_script_controls` (`ForceScriptControlRelease`)** — clears the
  tracker to empty (`consumed` *and* `passed_on`) **immediately on send**, after
  queuing the message, without waiting for the echo (the A3 policy; B3 carries
  it). Clearing on send is the *robust* choice, not merely eager: OpenSim's
  `HandleForceReleaseControls` echoes a single release block with
  `Controls = int.MaxValue (0xFFFFFFFF), PassToAgent = false`, which by the fold
  rule above would decrement only `consumed` and **miss** `passed_on` — so
  trusting the echo alone would leak passed-on counts. Clearing both maps on
  send fixes that, and the later echo's clamped decrement from an already-empty
  map is a harmless no-op. Does **not** touch `script_grants` (the
  `TAKE_CONTROLS` grant persists).
- **`RevokePermissions`** — does **not** touch `taken_controls` at all. The
  command is object-scoped and the sim honours only `TRIGGER_ANIMATION |
  OVERRIDE_ANIMATIONS` (verified in OpenSim's `HandleRevokePermissions`, which
  early-returns unless an animation bit is set and never clears controls);
  `TAKE_CONTROLS` is not revocable this way. Its only mirror effect is on
  `script_grants` (A3/B2).

**Reset on region-leave signals** — none. Per § Client-mirror reset reference
(A5), the tracker is **not** cleared on real teleport, neighbour crossing, or
`DisableSimulator`: it is agent-global (unattributable to the left-behind
in-world holder) and the viewer keeps `mControlsTakenCount` across a teleport
(reset only in its constructor, mutated only in `processScriptControlChange`).
It clears **only** via the inbound release echo (above) and the explicit
`release_script_controls` send.

**The accessor (finalizing A3's reservation).** A3 reserved
`script_controls(&self) -> …`; A6 fixes the return type as a small public view
(the internal `TakenControls` / its counts stay private):

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ScriptControlsInfo {
        /// Controls scripts hold and *consume* (avatar does not move from
        /// them) — union of `consumed` bits with count > 0.
        pub taken: ControlFlags,
        /// Controls scripts hold that are *also* passed to the agent — union
        /// of `passed_on` bits with count > 0.
        pub passed_to_agent: ControlFlags,
    }

    Session::script_controls(&self) -> ScriptControlsInfo

Two `ControlFlags` unions (fold each map's keys with `|`), mirroring the
viewer's two arrays without exposing the per-bit counts. No new `Event` is
emitted on a client-sent clear — `release_script_controls` is a local call the
driver made; the inbound path already emits `Event::ScriptControlChange` for
every change.
