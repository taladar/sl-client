---
id: viewer-audit-env-overrides-preferences
title: Environment variables silently override live graphics preferences
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Several `SL_VIEWER_*` variables override a **registered, GUI-editable** setting
with no indication in the UI, so the checkbox or slider moves and does nothing:

- `sl-viewer-world-scene/src/tonemap.rs:205-219` — `SL_VIEWER_TONEMAP`,
  `_MIX`, `SL_VIEWER_EXPOSURE` beat the stored `RenderTonemapType` /
  `RenderTonemapMix` / `RenderExposure` (bound at
  `sl-viewer-preferences/src/preferences_graphics.rs:536`, `:556`, `:564`), and
  `refresh_tonemap_settings` re-reads them **per camera per frame**;
- `sl-viewer-preferences/src/preferences_graphics.rs:606`, `:664` —
  `SL_VIEWER_SUN_SHADOWS`, `SL_VIEWER_SHADOW_CASCADES`;
- `sl-viewer-world-scene/src/{glow,exposure,particles}.rs` —
  `SL_VIEWER_DISABLE_GLOW`, `SL_VIEWER_GLOW_STRENGTH`,
  `SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE`, `SL_VIEWER_DISABLE_HUD_PARTICLES`.

Two authorities for one user-facing value, and the env one wins silently.
Related: `SL_VIEWER_SKIN` / `SL_VIEWER_THEME`
(`sl-viewer-ui-core/src/skin.rs:99`, `:102`) are a **third** authority alongside
the Colors & Skins tab and the `--skin` / `--theme` flags, resolved in
`SkinSelection::resolve`; and `SL_VIEWER_UI_LOCALE` / `_DIRECTION`
(`i18n.rs:99`, `:337`) shadow a registered locale setting.

The project rule is: GUI options live in preferences, CLI is for
non-GUI/startup concerns, env vars are for source-level debugging only. Scope:
either make these debug-only knobs that log loudly when they win, or drop them
in favour of the preference. Two more that drive user-facing behaviour rather
than debugging: `SL_VIEWER_GPU_AVATARS` / `_SPIKE` (`lib.rs:1674`, `:1683`)
select the rendering path, and `SL_VIEWER_CROWD` adds a visible toolbar button —
`CrowdDebugButtonPlugin` is added unconditionally to the live viewer at
`lib.rs:1186`.

For the whole picture: 93 distinct `SL_VIEWER_*` variables exist across 17
crates, of which about 60 are legitimate `SL_VIEWER_LOG_*` / `_DISABLE_*` /
budget knobs. Roughly 89 have no CLI or preference counterpart at all, so they
are undiscoverable and unlisted by `--help`.

## Resolution

The knobs stay — they are the A/B levers that isolate a rendering defect, and a
lever you have to rebuild to pull is not a lever — but they stop being *silent*,
which was the whole defect. Three pieces:

- **A registry.** `sl-viewer-settings/src/env_pins.rs` holds
  `EnvPinnedSettings`, a resource mapping a setting name to the `SL_VIEWER_*`
  variable holding it and the value it holds. A pin is either `Live`
  (re-applied every frame, so the preference is dead for the session) or `Seed`
  (only the start-up value, so the preference still works). Four modules record
  into it, each owning one half of a name pair:
  `render_overrides::record_env_pins` (glow, dynamic exposure, the
  tone mapper), `preferences_graphics::record_env_pins` (the two shadow knobs),
  `preferences_colors_skins::record_env_pins` (skin / theme, via
  `skin::record_env_pins`) and `i18n::record_env_pins` (the UI locale).
  `preferences::collect_env_pins` assembles them at plugin build — this crate is
  the one place that can see all four — and the environment is read exactly
  there and nowhere later.
- **A loud log.** `EnvPinnedSettings::announce` emits one `warn!` per pinned
  setting, naming the variable, its value and what it beat.
  `log_active_env_knobs` additionally logs *every* `SL_VIEWER_*` variable the
  process was started with, which is the cheap half of the discoverability
  problem the audit's last paragraph describes: a run's log now says which
  levers were pulled whether or not any of them pinned a setting.
- **A visible UI.** `guard_pref_bindings` (the old `guard_account_bindings`,
  extended — one system, because two systems inserting and removing
  `InteractionDisabled` on the same entity would each undo the other every
  frame) disables a control whose setting is `Live`-pinned, and
  `annotate_env_pinned_rows` appends a notice to its row naming the variable
  (`preferences-env-pin-live` / `-seed` in the `en` bundle). A `Seed` pin gets
  the notice without the disable.

Two of the audit's claims did not survive contact with the code:

- **`SL_VIEWER_GPU_AVATARS` no longer exists.** Phase 4 removed the CPU joint
  entities, so there is no `cpu`/`off`/`ghost` path to select; only
  `SL_VIEWER_GPU_AVATARS_READBACK` is left, and it only turns on a log. The
  comment in `viewer_plugins.rs` still described the removed knob and was
  corrected. `SL_VIEWER_GPU_AVATAR_SPIKE` already `warn!`s on every run it is
  on, which is exactly the treatment asked for.
- **`CrowdDebugButtonPlugin` being unconditional costs nothing visible.** The
  plugin registers two `Update` systems; the *button* is gated on `GpuCrowd`
  being armed and retires itself permanently when it is not, so an unset
  `SL_VIEWER_CROWD` produces no toolbar affordance.

Not done, deliberately: a `--help`-visible catalogue of all 93 variables. That
needs a registry of every knob, which nothing currently has and which would rot
the moment a module adds one without registering it. The active-knob log covers
the case that actually matters — "what was this run started with" — without a
list to keep honest.

`SL_VIEWER_DISABLE_HUD_PARTICLES`, `SL_VIEWER_EXPOSURE_COEFFICIENT`,
`SL_VIEWER_DISABLE_UNDERWATER_FOG`, `SL_VIEWER_UI_DIRECTION` and the rest of the
family are **not** pinned, because no registered setting claims those values —
there is no second authority to report. They appear in the active-knob log like
every other knob.
