---
id: idiomatic-p2-01
title: Camera::new (types/session.rs):
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 2 — Constructor invariants (low invasiveness, caller-facing)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`Camera::new` (`types/session.rs`): axes must be unit-length and
    orthonormal but it was unchecked. Did the maximal version of both options:
    the old `new` became the `const` `new_unchecked` (the codec-boundary
    constructor — the inbound `AgentUpdate` decode in `sim_session.rs` keeps
    whatever basis the peer sent, so it must reconstruct verbatim, not
    reject), and a *new* validating `Camera::new` returns
    `Result<Self, CameraError>` checking each axis unit-length, the three
    mutually orthogonal, and `at × left = up` (right-handed) — all within a
    small `f32` tolerance (`AXIS_TOLERANCE = 1e-3`). New public `CameraError`
    enum (`NotUnitLength`/`NotOrthogonal`/`NotRightHanded`, `thiserror`),
    re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`.
    `looking_at`/`region_center` now build via `new_unchecked` (their bases
    are already valid by construction). The REPL `build_camera`
    (`sl-repl/src/registry.rs`) uses the validating `new`, mapping a
    `CameraError` to `ReplError::InvalidArg`. Added module-level
    `dot`/`length` helpers (dedup'd with the test module). +4 unit tests
    (accepts a valid basis; rejects non-unit / non-orthogonal / left-handed).
    Wire bytes unchanged (decode path still uses the unchecked constructor).
