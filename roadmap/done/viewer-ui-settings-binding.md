---
id: viewer-ui-settings-binding
title: Two-way widget↔settings binding layer
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-settings-store]
refs: [viewer-ui-combo-widget, viewer-ui-settings-binding-combo, viewer-ui-settings-binding-text]
---

Context: [context/viewer.md](../context/viewer.md).

The two-way binding layer that connects a widget to a named setting in the
[[viewer-ui-settings-store]] — the `control_name=` idiom the reference uses
1,293 times, which is *why* ~20 of its preference panels have almost no code
behind them: a checkbox/slider/combo declares which setting it edits, and the
binding keeps widget and store in sync (read on build, write on change, react to
external changes).

This is what makes [[viewer-preferences-floater]], [[viewer-quick-preferences]]
and the [[viewer-input-rebinding-ui]] mostly declarative rather than hand-wired.
It depends on both the widget scaffold (the widgets) and the settings store (the
values).

Reference (Firestorm, read-only): `llui` `control_name` handling, `lluictrl`
`setControlName`, `llviewercontrol` connections.

## Done

New viewer module **`src/settings_binding.rs`** — the `control_name=` idiom as a
`SettingBinding { name, scope }` component on a widget entity, plus a
`SettingsBindingPlugin` that keeps widget and store in sync **all three
directions**:

- **Read on build** — idempotent `sync_bound_checkboxes` / `sync_bound_sliders`
  passes push the setting's effective value into the widget (`Checked` /
  `SliderValue`) whenever they disagree, so a freshly-spawned widget seeds
  itself.
- **Write on change** — `on_bound_checkbox_change` / `on_bound_slider_change`
  observers on `ValueChange<bool>` / `ValueChange<f32>` write the edit to the
  binding's `Scope` and reflect the widget's own new state at once (no one-frame
  lag).
- **React to external change** — because the store is the single source of truth
  and the sync passes run every frame, a reset button, the account scope loading
  at login, or a second widget bound to the same setting all propagate for free;
  the read and write paths never fight (a sync right after an edit is a no-op).

Ergonomic constructors `bound_checkbox(binding)` / `bound_slider(binding, range,
step)`; the binding carries the write `Scope` (`global()` / `account()`), a
small generalization of the reference's implicit "control's home group".
`ViewerSettings` grew `set(scope, …)`, `reset(scope, …)` and
`register_transient(…)` to serve it.

A live **`F7` demo panel** (in the pattern of the `F5` scaffold / `F6` i18n
demos, `SL_VIEWER_SETTINGS_BINDING_DEMO` to auto-show): a bound checkbox
(global) and a bound slider (account, exercising both scopes) over runtime-only
settings,
each with a live label, plus a "Reset to defaults" button that drives both from
outside to prove the react-to-external path. This makes the mechanism a real
(non-dead-code) consumer from day one.

Verified by 8 headless unit tests (read-on-build, write-on-change for bool and
f32, integer round/clamp, external-change reflection, two-widgets-one-setting
cross-sync, non-numeric rejection); clippy/fmt clean.

**Scope notes** (extension points, split into follow-up tasks, not built here):
a **combo / dropdown** binding — the reference's third `control_name` widget —
needs a combo widget that does not exist yet ([[viewer-ui-combo-widget]]) and
then its own binding pass ([[viewer-ui-settings-binding-combo]]), because a
combo's value is a *named option*, not a scalar, so it needs an option-key ↔
`SettingValue` mapping of its own. A **text-field** (string) binding waits on
[[viewer-ui-text-input-widget]] and is [[viewer-ui-settings-binding-text]] —
deferred not just for the widget but because text needs commit-on-final (not
per-keystroke) writes and must not clobber a focused edit, which the scalar
bindings do not. Both slot in as one more `ValueChange` observer + sync pass on
the plumbing this task established. Integer settings on a slider *are* supported
(widened to the slider's `f32`, rounded and range-clamped on write).

**Why no change-generation counter.** An earlier design considered adding a
monotonic `generation: u64` to `sl_settings::SettingsStore`, bumped on every
`set` / `reset` / `clear_scope` / `load_scope`, so the sync passes could *skip*
entirely on frames where nothing moved — cheaper than re-reading every bound
setting each frame — and, with a per-widget "last-seen generation", update only
the widgets whose setting actually changed rather than reconciling all of them.
It was dropped: the reconcile is already idempotent (it only writes a widget
that disagrees) and O(bound widgets), which is a handful when a preferences
panel is open and zero when it is closed, so the counter would buy negligible
time while adding mutable API surface to the otherwise-pure crate and a second
source of truth (the counter) that every future mutator would have to remember
to bump.
The follow-up combo/text bindings inherit the same every-frame-idempotent
approach; if a future panel with hundreds of live bindings ever makes the
per-frame scan measurable, the counter is the documented first optimization.
