---
id: permission-b5
title: Add the lifecycle test suite
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**B5 (from A8). Add the lifecycle test suite.** In
    `sl-proto/tests/lifecycle.rs` (and one round-trip in
    `sl-proto/tests/sim_session.rs`), add the cross-cutting reset/recording
    cases from § Test & verification strategy reference, built only from the
    existing helpers (`established`, `server_message`, `drain` /
    `drain_events`, `object_update[_in]`, the `enable_neighbour_b` +
    `CrossedRegion` + `AgentMovementComplete` crossing fixture, `KillObject`,
    `DisableSimulator` from `sim_b()`, `sim.send_script_control_change`) — no
    new harness. Cover, at minimum, the rows of the reference table: grant /
    deny-as-explicit-`Denied` / never-asked-as-`NeverAsked` (B2.5) /
    re-grant-replaces, the animation-only revoke, the teleport reset (in-world
    cleared, attachment kept — **both halves**, now unblocked by B1.5), the
    neighbour-crossing keep-all, the circuit-retired and `KillObject` scoped
    drops, the controls Take/Release fold incl. the count model and the
    pass-to-agent split, release-on-send, and the `script_permission_state`
    snapshot (grants + denials + controls). Add at least one **two-store**
    integration case (a grant **and** a taken control surviving / clearing
    across the same teleport) — the behaviour no single task owns. Assert the
    conservative-mirror invariants (a revoke clears only the honoured bits, a
    teleport clears only in-world grants never controls). Depends on B1–B4 (it
    exercises the whole surface). The earlier attachment-detection gate is
    **lifted** (B1.5 resolved Open-question #1): write the
    attachment-kept-on-teleport assertion in full, no `// TODO`. Run the full
    `cargo test -p sl-proto`; clippy-clean (restriction lints) and `cargo fmt`
    (+ rumdl on this file) before commit. **Done 2026-06-26.** Every
    reference-table row had already landed as a focused unit test in its own
    B2/B2.5/B3/B4 commit (`answer_records_grant_*`, `re_grant_replaces_*`,
    `revoke_clears_only_honoured_*`,
    `teleport_drops_inworld_grants_keeps_attachment`,
    `neighbour_crossing_keeps_grants`,
    `disable_simulator_drops_child_circuit_grants`, `kill_object_drops_grant`,
    `never_asked_denied_and_granted_are_distinct`,
    `teleport_drops_inworld_denial_keeps_attachment_denial`, the
    `taken_controls_*` set,
    `release_script_controls_clears_taken_but_keeps_grant`,
    `script_permission_state_bundles_grants_and_controls`, and the
    `sim_session.rs` round-trip
    `taken_controls_tracker_folds_sim_control_change`). As planned, B5's
    distinct addition is the **two-store integration** case —
    `teleport_resets_grants_across_both_permission_stores` — which drives a
    grant *and* a taken control through one teleport and asserts the
    conservative-mirror invariant that the teleport drops in-world grants but
    leaves the (agent-global) taken-controls tracker untouched. Full
    `cargo test -p sl-proto` green (325 lifecycle + 63 sim_session), clippy
    clean.
