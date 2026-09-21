---
id: test-crosscheck-ui-scenes
title: UI scenes — putting two viewers into the same interaction state
topic: test
status: ideas
origin: Vintage skin fidelity work (2026-09-20)
points: 13
refs: [viewer-vintage-ui-chrome-crosscheck, test-firestorm-crosscheck-runner,
       test-firestorm-harness-skin-selection, viewer-ui-floater-persist-geometry,
       viewer-ui-interaction-contracts, viewer-ui-interaction-harness,
       viewer-ui-widget-interaction-suite, viewer-vintage-skin]
---

Context: [context/testing.md](../context/testing.md),
[context/vintage-skin.md](../context/vintage-skin.md).

A `--capture-ui` cross-check compares **chrome** — menu bar, toolbars, chat
bar, status row — and nothing else, because the harness closes every floater
and blocks them for the whole run. That block is right and should stay: a
notification appearing between two frames of one sequence would make them
incomparable. But it leaves the entire floater surface unphotographable, and
floaters are most of the interface and most of what a skin dresses. Every
claim in [[viewer-vintage-skin]] about fields, lists, tabs, scrollbars and
checkboxes concerns pixels no cross-check can currently take.

Filed as an idea rather than a ready task because the scope is still the
question — but the *shape* of the answer is now clear enough to write down.

## Opening the right floater is the tip of it

The rest, roughly in the order it stops being a property and starts being the
residue of input:

- **Which floater**, and for an **instanced** one *which subject*. Our
  registry is keyed by id and key, mirroring
  `LLFloaterReg::showInstance("profile", LLSD().with("id", agent))`. The
  subject has to be an id the **world scenario** pins, or the two viewers open
  windows about different things.
- **Geometry** — size and position, and the UI scale beside them.
- **Tabs** — which tab of a container is front.
- **Scroll positions.**
- **Selections** — list rows, gallery items, the in-world selection (which is
  upstream of what several floaters *show*).
- **Combos and select boxes**, open or closed, on which entry.
- **Menus**, submenus, and which entry is highlighted.
- **Context menus** — a menu that exists only because of a right-click on a
  particular thing.
- **Pickers** — texture, colour, name: floaters that also carry a value and a
  selection within it.
- **Hover**, which half the states a skin defines depend on.

## The shape: locate and act, as a web testing framework does

Not "declare every end state" (a set-your-state hook per widget kind on both
sides, and on the reference side a long list of `llui` classes in a fork we
rebase) and not "replay input" (raw coordinates are worthless across two
viewers that deliberately do **not** share XUI layouts —
[[viewer-ui-skin-tokens]]). It ends up a **mix**, and the borrowed model is
Selenium/Playwright:

- a **selector** that names a widget — by role, id, label text, or index
  within a named list — which each viewer resolves against its own tree;
- **semantic operations** on the located widget: `scroll_into_view`, `click`,
  `hover`, `select_option`, `expand`, `type`;
- **actionability waiting** before each operation — visible, stable, enabled.

The scroll case is why this beats both alternatives. A pixel offset is not
portable between two viewers whose rows are not the same height, and replaying
wheel events lands somewhere different in each. *"Scroll element N into view"*
is a statement that means the same thing on both sides. Selection is the same:
"the row named X", not "row 3 at y=240".

So the mix falls out of the vocabulary rather than being designed: geometry
and tab choice stay declarative because they *are* properties with portable
values; scroll, selection, hover and context menus become operations, because
they have no portable value to declare.

Actionability waiting also answers a problem raised separately — a floater's
contents are data, and the world-quiescence heuristic counts quiet frames of
the *world* while an inventory list is still filling. "Wait until this element
is stable" is the same primitive, applied to the thing actually being
photographed.

## What each side already has

**Ours**: three interaction tasks are done — a contract per registered element
([[viewer-ui-interaction-contracts]]), headless synthetic pointer input
([[viewer-ui-interaction-harness]]) and deep tests for the stateful widgets
([[viewer-ui-widget-interaction-suite]]) — plus the `FLOATERS` and `UiElement`
registries. Most of a locator-and-action layer exists; what it lacks is a
vocabulary meant to be spoken by something outside the process.

**Theirs**: XUI `name=` attributes and `LLView::getChildView(name, recurse)`
are a named tree, which is a selector path already. No event recorder in this
tree to build on.

**Both**: selectors are a per-viewer namespace, exactly like skin ids and
floater ids. Logical names mapped per side, with a per-viewer type so one
viewer's selector cannot be handed to the other —
[[test-firestorm-harness-skin-selection]] settled that pattern.

## The scoping question to answer first

**How much of this carries skin information?** Vintage fidelity needs colours,
widget shapes and widget states — not every floater in every scroll position.
The cheap target is the UI equivalent of the fake grid's `catalogue` scenario
(one prim per rendering feature): a **widget catalogue**, one window holding
one of each widget in each state, laid out to be photographed. We have that
surface already (the gallery), it needs no login, and it is where a skin is
authored anyway. The reference has none, so it would mean a debug floater in
the fork — contained, and far smaller than a hook per widget class.

If a widget catalogue answers the skin question, the full locate-and-act layer
is only needed for a *different* question — regression-testing our own UI over
time — and that one lives entirely on our side, with no cross-viewer namespace
problem at all.

## Regardless

- The floater block becomes **selective**: close all, block all, then open
  exactly what the scene declares.
- A UI scene is only reproducible **paired with a world scenario** and a fixed
  account; that pairing belongs in the scene.
- **Not a pixel-for-pixel layout diff.** Where a control sits inside its window
  is ours to decide; colour, shape and presence are what a pair is asked.

## Done when

The scoping question has an answer, and whichever surface it picks can be
photographed twice in a row, on both sides, with the same result.
