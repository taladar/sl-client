---
id: viewer-build-floater-interaction-tests
title: The build floater — reflect, edit, commit
topic: viewer
status: done
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

## Done (2026-09-05)

`sl-client-bevy-viewer/src/build_floater_test.rs` — 18 tests over a new
fixture fold, `world_test::world_app_with_build_tools`: the world fold
(CPU pick resolver) under a real UI, with the floater, its five
per-aspect editor tabs, the Create tool and the link/undo shortcuts on
top. Every test streams a prim in as a grid would, selects it with a
**click in the world**, and asserts through the window.

Two arrangements are load-bearing and documented in the module:

- **The window is parked to fit.** 420 × 640 does not fit an 800 × 600
  fixture viewport, and a control laid out past the edge is one no
  pointer can click, so `open_build_floater` parks it through the
  manager's own `FloaterGeometry` restore path.
- **The prim is framed beside the window.** A 420-wide window covers the
  viewport centre wherever it sits, so the gizmo tests' "aim at the
  fixture, click the middle" would click the window.
  `frame_beside_the_window` searches a couple of camera poses and returns
  where the prim *actually* landed, refusing any pose that is not
  clickable.

Both helpers that drive the window **verify what they did** rather than
assume it: `show_tab` checks the strip's own `active` (and walks the
overflow arrows first — five tabs do not fit 420 px), and `click_field`
checks who holds focus (and wheels the page first). That was not
gold-plating: the first run's four failures were a tab strip that had
silently never switched and a page whose field was below the fold, both
reported as bugs in the wrong module.

Covered: the nine transform fields filling from a selection and emptying
with it; an incoming `ObjectUpdated` moving the numbers; the summary
line's segments (including the link-number one only in
edit-linked-parts mode); typing a position and committing exactly one
`UpdateObject` carrying only a position, plus the local echo; a letter
refused by the float filter; the grid unit committing into
`EditToolState` and clamping; the tool radio and `EditToolState.tool`
both ways over all five tools; each of the four toggles with its glyph;
`Ctrl+B` opening and closing (and the close clearing the selection);
every tab showing its own page with its editor docked; the linked-part
prev/next walking and wrapping; and the sub-suites — `SetObjectName`,
`SetObjectShape`, `SetObjectImage`, the Texture tab following a face
selection, `Ctrl+L` / `Ctrl+Shift+L`, and a Create-tool click rezzing at
the surface it struck.

Three production changes the tests forced, each small and each a real
defect:

- the nine transform fields shared three element names, so
  `build-pos:field` named three nodes and no lookup (test, gallery, or
  future skin rule) could tell X from Z — now one name per axis, which is
  what `TextInputSpec::element`'s own doc example already showed;
- the Texture tab's "which faces will this hit" line carried no `Name`,
  so the one line telling the user how wide an edit will be was
  unaddressable — now `build-tex-info:Faces`;
- `i18n::install_untranslated`, the string half of a headless harness:
  the three resources a `Translator` needs plus the label-resolving
  system, with no bundles, so every key resolves to itself. Without it a
  system taking `Translator` fails parameter validation, and every
  `Translated` label keeps the empty text it spawned with.

Deliberately not covered, and why:

- **Wording.** The fold has no bundles, so a line's *segments* are
  asserted and never a translation. `tests/locale_bundles.rs` owns the
  latter.
- **The input-context guard.** `InputContextPlugin` is not in the fold,
  so `InputContext` stays `World` and "a chord typed into a field is not
  a shortcut" is unasserted. Adding the plugin (it needs
  `CursorGrabAllowed` and a window `CursorOptions`) would buy that one
  test; worth doing when something else needs the same fold.
- **`edit_material`.** Its plugin is in the fold, but its PBR channels
  need a material asset the fixture does not fetch; the Texture tab's
  legacy channel stands for the tab here.
