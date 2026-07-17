---
id: viewer-ui-radial-menu
title: Radial (pie) menu widget
topic: viewer
status: done
origin: noticed as a missing fundamental while reviewing viewer-ui-widget-scaffold (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-ui-context-menu, viewer-object-context-menu, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The **general mechanism** for putting a radial menu on screen and picking an
entry from it — not any particular menu's entries. Nothing upstream has one:
`bevy_ui_widgets` ships `MenuPopup` / `MenuItem` / `MenuButton`, all of which
assume a **line** layout, so this is ours to build on `bevy_ui`. The pie menu is
also one of the most recognisably *Second Life* pieces of interaction there is —
a viewer without one feels wrong to a long-time resident — and it is muscle
memory, so the geometry and the gesture have to match rather than merely
approximate.

The widget, and only the widget:

- **Angular stability is the invariant — design everything else around it.**
  A pie's entire advantage over a line menu is that you learn it with your hand:
  "touch is a flick north" becomes muscle memory, and you stop reading the menu
  at all. That only holds if a slice's compass position is a property of **the
  entry**, never of its index in whatever subset happens to be showing. So an
  entry is **pinned** to a position, and one that is absent leaves its slice
  **empty** — it must never shift its neighbours round to close the gap, because
  that silently re-teaches every angle the user already knows. Assigning slices
  in list order is the obvious implementation and it is the wrong one.
- **Selection by angle, not by hit-test.** The slice under the pointer is
  computed from the *angle* of the mouse vector from the centre, not by testing
  a rectangle — that is the other half of what makes a pie fast: every slice is
  an equal-sized angular target, and flicking in a direction is enough, at any
  distance. A **dead zone** around the centre selects nothing, so opening the
  menu and releasing without moving cancels.
- **Eight slices**, the reference's `PIE_MAX_SLICES`, at the compass points,
  with the slice centres rotated by half a slice so they align to the axes.
- **Autohide: keep the idea, fix the implementation.** The idea is a
  **chain of mutually exclusive candidates for one position** — one slice holds
  "Sit" or "Stand" depending on state (`piemenu.cpp`'s own comment:
  *"this is useful for Sit/Stand toggles"*), so the angle does not move either
  way. Chains are opt-in per run of entries (`start_autohide` marks the head,
  `autohide` the continuations).

  **This is where we should beat the reference, which does not hold the line
  here.** For an ordinary hidden slice it is stable —
  *"pie slices never really disappear"*, so the slot survives and simply renders
  blank. But an autohide chain is not: its losing members `continue` **without**
  incrementing the slot counter (`num++` sits at the bottom of the loop body,
  `piemenu.cpp:474`), so the number of slots a chain occupies depends on
  run-time state — one when a member wins, several when none does — and
  **every entry after it rotates**. That is precisely the muscle-memory breakage
  the feature was meant to avoid.

  Ours should make the position **explicit and declared** rather than derived
  from a running counter over a list: an entry names its compass point, an
  autohide chain names *one* compass point and resolves to at most one entry
  within it, and a slot with no winner stays empty. Then no state anywhere can
  move an angle, which is a property the counter-based design can only
  approximate.
- **Nested sub-pies.** A slice may open another pie in place
  (`PieMenu::appendContextSubMenu`), so the mechanism is recursive.
- **Do not reproduce `More >` chaining.** Eight slots is a hard budget, and the
  reference spends the overflow on a slice literally labelled `More >` that
  opens another pie — which itself has a `More >`. `menu_pie_object.xml` chains
  **three of them**, so some object actions sit four pies deep, and
  `menu_pie_avatar_other.xml` has two.

  This is the angular-stability problem again, one level up: `More >` is
  *arbitrary overflow*, so which page an entry lands on is a function of how
  many entries happen to exist, and adding one anywhere can push everything
  after it to a different depth. It is also unlearnable by construction — a
  slice that says `More` tells your hand nothing, so the muscle memory the pie
  exists for stops at the first hop.

  **The direction to take: nest by *meaning*, never by overflow.** Eight will
  not be enough for every target — that much the reference is right about — so
  nesting is unavoidable; what is avoidable is nesting *arbitrarily*. A slice
  reading `Land >` or `Manage >` is a stable, learnable grouping whose contents
  a user can predict and whose position never moves; `More >` is a confession
  that the entries outgrew the budget and tells the hand nothing. The same
  recursive mechanism serves both — the difference is entirely in how the entry
  tree is authored, which makes this a constraint on
  [[viewer-object-context-menu]] as much as on this widget:
  **every sub-pie must be nameable**, and if a grouping cannot be given an
  honest name, that is a sign the grouping is overflow rather than structure.

  Worth prototyping alongside, as the one option that raises the budget rather
  than nesting it: **concentric rings** — angle picks the direction, distance
  from the centre picks the ring, in one uninterrupted gesture with no paging.
  Angular stability survives, because a direction still always means the same
  family. It composes with named sub-pies rather than competing with them.
- **Placement — the hard part, and harder than for a line menu.** A line menu
  needs clearance in *one* quadrant and can simply flip or slide when it runs
  out ([[viewer-ui-context-menu]]). A pie is **centred** on the spawn point and
  needs a full `radius` of clearance in *every* direction, so a click anywhere
  within 96 px of an edge — or in a corner, where two edges bite at once — has
  nowhere to put the circle. Clipping is not an option: a clipped slice is an
  unreachable entry.

  The reference's answer is worth copying and is not obvious: it
  **clamps the centre** inward until the whole circle fits, and then
  **warps the mouse pointer to the clamped centre** (`PieMenu::show` →
  `LLUI::setMousePositionLocal`). The warp is not a nicety — selection is by
  angle *from the centre*, so a centre that is not under the pointer makes every
  angle a lie, and the menu would open with a slice already "chosen" in the
  direction of the offset.

  **This will not port as-is.** Warping the pointer is not generally permitted
  on **Wayland** (no unconstrained pointer warp; `winit`'s `set_cursor_position`
  fails), which is the primary desktop here. So decide deliberately between:
  clamp the centre and accept an off-centre pointer (then a dead zone must be
  large enough that the initial offset does not read as a selection, and the
  opening highlight must be suppressed until the pointer actually moves); or
  take a pointer lock/constraint for the menu's lifetime and drive a virtual
  cursor from relative motion, which is what a locked pointer gives us anyway.
  Prototype before committing — this is the task's one real unknown.
- **Keyboard reachable.** The reference's pie is mouse-only; ours must not be —
  `viewer-ui-widget-scaffold` established that focus and tab navigation are the
  UI's spine, and a menu no keyboard can reach is a hole in it.

Per the scaffold's conventions this is **direction-neutral by construction** (a
circle has no leading side) but its *labels* are not: the slice text must lay
out through the same bidi text stack as everything else, and any left/right-ish
affordance is named logically.

Deliberately **not** in scope: which entries any given pie holds. Those are
per-domain and belong with the domain — the object / avatar / land / attachment
pies are [[viewer-object-context-menu]].

Reference (Firestorm, read-only): `newview/piemenu.{h,cpp}` (the widget, the
angle maths, `PIE_MAX_SLICES = 8`, `PIE_OUTER_SIZE = 96`), `newview/pieslice.*`,
`newview/pieseparator.*`, `newview/pieautohide.*`, and the `PieMenu*` settings
in `newview/app_settings/settings.xml`. Note the pie is a
**Firestorm re-addition** — Linden Lab's viewer 2 dropped it — so upstream LL
sources will not have it.

## Done

`sl-client-bevy-viewer/src/pie_menu.rs` + `pie_menu.wgsl`, wired into both the
viewer (`ViewerUiPlugin`'s sibling `PieMenuPlugin`) and the UI gallery. The
widget only — which entries any pie holds stays with
[[viewer-object-context-menu]].

- **Angular stability, as data and as tests.** An entry declares its compass
  point (`PieEntry::at`); `resolve_slots` writes each into its slot, so an
  absent entry leaves its slice empty and nothing rotates. `PieAddress`
  /`addresses` compute an action's full path statically.
  `every_action_keeps_its_declared_ address` pins the fixture's table, and
  `no_condition_can_move_an_entry` sweeps the powerset of live conditions
  proving no state moves an entry.
  **Standing rule recorded (module docs + memory):** every *real* domain pie
  must ship its own pinned address table, so an unaware reorder fails a test
  rather than silently re-teaching muscle memory.
- **Named sub-pies, autohide chains, disabled/empty slots** — no `More >` by
  construction (a sub-pie is a `PieMenuDef` whose `label` is not optional). The
  autohide chain resolves to at most one member at one declared point, which
  fixes the reference's counter bug (`piemenu.cpp:474`).
- **Eight slices drawn by one `UiMaterial` shader**; selection is by **angle**
  from the ring centre, with a dead zone. Labels are placed **polar** on their
  wedges *inside* the ring (`fit_pie_layout`), and the ring **grows** to keep
  labels non-overlapping — with a **minimum** at the reference base size so
  single-glyph (CJK) labels stay comfortable, and a `max_width` bound plus a
  documented paragraph ceiling above. Colours match the reference
  (`PieMenuBgColor`, `PieMenuLineColor`, and the `EmphasisColor_35` orange
  highlight drawn as a radial gradient); a sub-pie slice gets a shader-drawn rim
  chevron rather than a `>` in the label.
- **Placement clamps inward** by the measured box so the whole menu clears every
  edge; a corner clamps both axes. Two interaction modes (flick / pinned).
  Dismiss on: dead-zone click, empty/disabled-slice click, click off the menu
  (which opens a new one there), or window focus loss. The menu **blocks picking
  and swallows presses** so on-menu clicks are the menu's alone and never bubble
  to the world's open-observer.
- **The pointer problem — deliberately resolved against the task's suggestion.**
  The reference warps the cursor to the clamped centre; Wayland forbids that.
  The task's fallback (pointer-lock + virtual cursor) was built, then
  **measured and abandoned**: the compositor refused `CursorGrabMode::Locked`,
  leaving two cursors disagreeing. Final decision: the
  **real pointer is the only cursor, with no jump** — away from edges nothing is
  lost; near an edge the opening highlight follows the reference's own
  non-warping descent. Descending a sub-pie re-centres the menu *on the pointer*
  (the inverse of the warp).

### Scope deviations (accepted with the user during the build)

- **Keyboard reachability dropped** — the task requires it ("the reference is
  mouse-only; ours must not be"), but the user directed removing it: a pie opens
  on an in-world object the pointer is over, and there is no keyboard way to
  pick that object, so a keyboard way to pick *within* the menu would open onto
  nothing. The pie is now mouse-only like the reference (no keypad compass, no
  tab focus, no Enter/Escape). Labels are neither buttons nor focus targets.
- **Concentric rings not built** — the task's "worth prototyping alongside"
  option is filed as [[viewer-ui-radial-menu-concentric-rings]] (wont-do): an
  additive input axis with its own cost, and nothing needs it while named
  sub-pies suffice.

### Test-harness additions (reusable by future radial widgets)

The universal box checks cannot see a shader-drawn wheel, so two **declared**
checks were added to `crate::ui_test`'s universal list, both verified to bite:
`radial_violations` (a labelled node must lie within tolerance of its declared
angle from a `RadialCentre`, or a pointer aimed at it selects a different slice)
and `radial_overlap_violations` (no two labels overlap).
`crate::ui::PhysicalAxes` was considered and then removed once the polar layout
made compass placement direction-independent for free.
