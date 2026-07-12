---
id: permission-b3
title: The complete taken-controls tracker
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B3 (from A6/A3). The complete taken-controls tracker — state, fold,
    accessor and the release-on-send clear.** Self-contained: the field is
    written by the fold and read by the accessor in the same unit, so nothing
    is dead. **Done 2026-06-26** — added the private `TakenControls` struct
    (two `BTreeMap<u32, u32>` maps `consumed` / `passed_on`, single-bit-mask
    key → take count) and the `taken_controls` field on `Session`
    (session.rs), the free `iter_bits` helper and the private
    `note_taken_controls` fold method, the inbound fold in the
    `AnyMessage::ScriptControlChange` handler (before the unchanged event
    push), the public `ScriptControlsInfo` view (types/script.rs, re-exported
    via `types.rs` / `lib.rs`) and the `Session::script_controls()` accessor,
    and the clear-both-maps-on-send in `release_script_controls`. Four
    `lifecycle.rs` tests (take/release, the count model, the pass-to-agent
    split, release-on-send keeps the grant) plus one `sim_session.rs`
    round-trip folding the real server-built block. Builds, clippy-clean
    (restriction lints), `cargo test --workspace` green.
    **Two adaptations vs the literal plan (no behavioural change):** (1)
    `iter_bits` is a **free function** (not a method) since it needs no
    `self`, and it clears the isolated low bit with `remaining &= !bit` rather
    than `remaining &= remaining - 1` (the `- 1` trips the
    `arithmetic_side_effects` restriction lint; `& !bit` is equivalent and
    lint-clean). (2) the per-block fold is factored into a small private
    `note_taken_controls(action, controls, pass_to_agent)` helper called from
    the handler, keeping the handler readable; behaviour is exactly the
    planned increment / saturating-decrement-and-remove-at-zero. Per
    § Inbound control-change reference:
    - **State** (`sl-proto/src/session.rs`): private `TakenControls` struct
    (two `BTreeMap<u32, u32>` fields `consumed` / `passed_on`, single-bit-mask
    key → take count) and the field `taken_controls: TakenControls` beside
    `script_grants` / `sit` / `teleport` (init empty in the constructor at
    `methods.rs:138`). Add a private
    `iter_bits(controls: ControlFlags) -> impl Iterator<Item = u32>` helper
    (yield each set bit as its own mask — no raw indexing, clippy-clean).
    - **Inbound fold.** In the existing `AnyMessage::ScriptControlChange`
    handler (`session/methods.rs:2676`), fold each block into `taken_controls`
    **before** the existing `Event::ScriptControlChange` push (the event still
    emits unchanged): select the map by `pass_to_agent`; for each set bit, on
    `Take` increment, on `Release` saturating-decrement and remove the key at
    0. Do **not** touch `script_grants`.
    - **Accessor.** Add the public `#[derive(Clone, Copy)] ScriptControlsInfo`
    view (`taken` / `passed_to_agent`, each a `ControlFlags` union of its
    map's keys) and `Session::script_controls(&self) -> ScriptControlsInfo`
    (folds the counts' keys with `|`; counts stay private).
    - **Release-on-send.** Have `release_script_controls`
    (`session/methods.rs`) clear **both** maps (`consumed` and `passed_on`) to
    empty after queuing `ForceScriptControlRelease`, *without* touching
    `script_grants` (`TAKE_CONTROLS` grant persists). Clear on send, not on
    the echo (OpenSim's echo is `Controls = 0xFFFFFFFF, PassToAgent = false`
    and would miss `passed_on`). Its signature is unchanged, so no runtime
    caller breaks.
    - **No resets.** The tracker is untouched by the B2 region-leave signals
    (A5): it self-corrects only on the inbound release echo (this handler) and
    the explicit `release_script_controls` send.
    Depends on B2 (the `Session` field neighbourhood). Tests
    (`sl-proto/tests/lifecycle.rs` / `sim_session.rs`, mirroring the
    `SimSession::send_script_control_change` path, `sim_session.rs:1856`): a
    `Take` → `script_controls().taken` contains them; a matching `Release` →
    empty; two takes of a bit then a release → still taken (count model); a
    take with `PassToAgent = true` → lands in `passed_to_agent`, not `taken`;
    `release_script_controls` after a take → controls empty, the
    `TAKE_CONTROLS` grant in `script_grants` unchanged.
