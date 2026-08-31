---
id: viewer-build-floater-interaction-tests
title: The build floater — reflect, edit, commit
topic: viewer
status: ready
origin: user request (2026-07) — build floater / gizmos named the priority
points: 8
blocked_by: [viewer-ui-keyboard-text-harness, viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

The build/edit floater is the densest UI element and the second half of
the edit tool — the user's other named priority. Using the world fixture
plus the keyboard harness:

- selection → `sync_numeric_fields` reflects position/rotation/scale into
  the fields;
- typing into a numeric field and committing (`commit_numeric_fields`)
  emits the right `SlCommand`, and garbage is rejected by the
  `TextInputKind` filters;
- tool radios ↔ `EditToolState.tool` stay bidirectionally in sync;
- the toggles (`edit_linked`, `stretch_both`, `snap`) flip on click;
- Ctrl+B toggles the floater;
- tab pages (`BuildTabPages`) switch and dock the per-aspect editors;
- link-part navigation cycles; `update_selection_summary` counts.

Then extend across the per-aspect editors as sub-suites: `edit_params`
(prim parameter fields → `SlCommand`; incoming `SlEvent` updates →
fields), `edit_texture`/`edit_material` (face selection reflected, edits
emitted), `edit_link` (link/unlink commands), `edit_create` (create/rez
tool → rez command with the right ray). Reuse
`spawn_build_tools_specimen` where a pure-UI cell suffices.
