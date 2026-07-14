---
id: viewer-quick-preferences
title: Quick-preferences panel
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-framework, viewer-preferences-ui]
---

Context: [context/viewer.md](../context/viewer.md).

The small always-reachable panel of the settings you actually change several
times an hour, so you never open the full preferences floater for them: draw
distance, the environment / windlight preset and time of day, avatar hover
height ([[viewer-hover-height]]), rendering quality, avatar complexity limits
and maximum non-imposters, and whatever else turns out to be reached-for often.
Firestorm's Quick Preferences is the model, including that its **contents are
user-configurable** — the panel is a curated view over the settings store, not a
fixed list.

That is the design question worth settling here: rather than a hard-coded
floater, make it a *view* over the typed settings store that
[[viewer-preferences-ui]] defines, so a setting can be surfaced in the quick
panel without being reimplemented — and so a user can add or remove entries.
Whether the entries are user-editable in the first version, or just a good
default set with the plumbing ready, is a scope call for the fleshing-out agent.

Cross-refs: [[viewer-preferences-ui]] (the settings store and the full floater),
[[viewer-hover-height]] and [[viewer-volume-panel]] (two entries that are also
tasks in their own right).

Reference (Firestorm, read-only): `fsfloaterquickprefs` (`quick_preferences`
XUI and its user-editable control list), `llfloaterpreference`.

Deps: [[viewer-ui-framework]] (the floater),
[[viewer-preferences-ui]] (the settings store it is a view over).
