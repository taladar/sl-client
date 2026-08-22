//! The two-way widget↔settings binding layer (`viewer-ui-settings-binding`): the
//! mechanism that connects a headless `bevy_ui_widgets` control to a named
//! setting in the persistent store (`settings`), so a panel *declares*
//! which setting a checkbox or slider edits instead of hand-wiring each one.
//!
//! This is our counterpart of the reference viewer's `control_name=` idiom —
//! the attribute it uses 1,293 times, and *why* ~20 of its preference panels
//! have almost no code behind them: a control names the setting it edits and the
//! framework keeps the two in sync. Here that is a [`SettingBinding`] component
//! attached to the widget entity, plus a handful of systems that keep widget and
//! store agreeing in both directions.
//!
//! # The three directions of sync
//!
//! - **Read on build.** When a bound widget is spawned (or the setting is first
//!   registered), the `sync_bound_checkboxes` / `sync_bound_sliders` passes
//!   push the setting's current effective value into the widget — the checkbox
//!   shows `Checked`, the slider's thumb sits at the stored value.
//! - **Write on change.** When the user toggles or drags, the widget emits a
//!   [`ValueChange`]; `on_bound_checkbox_change` / `on_bound_slider_change`
//!   write the new value to the binding's [`Scope`] and, so there is no one-frame
//!   lag, immediately reflect the widget's own new state.
//! - **React to external change.** Anything else that moves the store — a "reset
//!   to defaults" button, the per-account scope loading at login, a second widget
//!   bound to the *same* setting — is picked up by the same idempotent sync
//!   passes, which run every frame and only touch a widget whose displayed value
//!   actually disagrees with the store.
//!
//! Because the store is the single source of truth and every write converges to
//! it, the read and write paths never fight: a sync pass that runs right after a
//! user edit finds the widget already agrees and does nothing.
//!
//! # What binds today
//!
//! - [`Checkbox`] (a [`SettingValue::Bool`]) and [`Slider`] (a
//!   [`SettingValue::F32`], or an integer [`SettingValue::I32`] /
//!   [`SettingValue::U32`] widened to the slider's `f32` and rounded back on
//!   write) — the two headless `bevy_ui_widgets` primitives.
//! - A [combo](crate::ui_combo) whose anchor carries a [`ComboBindingValues`]
//!   beside its [`SettingBinding`]: each option index maps to one
//!   [`SettingValue`], a user pick writes that value, and the sync pass moves
//!   [`ComboSelection`] to the option matching the store (an external value no
//!   option covers leaves the combo untouched).
//! - A [text input](crate::ui_text_input) (an [`EditableText`] entity) bound to
//!   a [`SettingValue::String`]: edits made **while the field holds focus**
//!   write live (matching the checkbox/slider live-edit model the preferences
//!   snapshot relies on), and the sync pass seeds / follows external changes,
//!   skipping the focused or IME-composing field so it never clobbers typing.
//! - A [colour swatch](crate::ui_color_picker) carrying a [`SettingBinding`]
//!   beside its [`ColorSwatchValue`], bound to a [`SettingValue::Color3`]:
//!   every [`ColorPicked`] reply — the picker's live drag, OK, and the
//!   original-colour re-emit on its Cancel — writes through, so the preview is
//!   live and a picker cancel self-reverts. A pick equal to the declared
//!   default is written as a **reset** instead of an override, so a colour the
//!   user left at the skin's value keeps following the skin
//!   (the reference's save-only-if-different `colors.xml` behaviour).
//!
//! Reference (Firestorm, read-only): `llui` `control_name` handling, `lluictrl`
//! `setControlName`, `llviewercontrol` connections.

use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::Checked;
use bevy::ui_widgets::{
    Activate, Button, Checkbox, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
    ValueChange,
};
use sl_settings::{Scope, SettingKind, SettingValue};
use tracing::warn;

use crate::ui_color_picker::{ColorPicked, ColorSwatchValue};
use crate::ui_combo::{ComboChanged, ComboSelection};
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::ui::{
    LogicalInset, LogicalMargin, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row,
};
use sl_viewer_ui_core::ui_font::UiFont;

/// Names a setting a widget edits, and the override [`Scope`] a user edit is
/// written to.
///
/// Attach it to a [`Checkbox`] or [`Slider`] entity (usually via
/// [`bound_checkbox`] / [`bound_slider`]) and the binding systems keep the two in
/// sync. Reads always resolve the *effective* value (account → global →
/// default); the scope only chooses which layer a write lands in — [`Global`](Scope::Global)
/// for a machine-wide preference, [`Account`](Scope::Account) for a per-avatar one.
#[derive(Component, Debug, Clone)]
pub struct SettingBinding {
    /// The name of the setting in the `settings` store.
    name: String,
    /// The override layer a user edit is written to.
    scope: Scope,
}

impl SettingBinding {
    /// Bind to a machine-wide ([`Global`](Scope::Global)) setting.
    pub fn global(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            scope: Scope::Global,
        }
    }

    /// Bind to a per-avatar ([`Account`](Scope::Account)) setting.
    pub fn account(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            scope: Scope::Account,
        }
    }

    /// The name of the bound setting.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The override scope a user edit is written to.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }
}

/// The option-index → setting-value map of a bound [combo](crate::ui_combo):
/// entry *i* is the value written when the user picks option *i*, and the value
/// the sync pass matches to show option *i*.
///
/// Attach it beside a [`SettingBinding`] on the combo's anchor entity (the one
/// carrying [`ComboSelection`]); its length must equal the combo's option count.
#[derive(Component, Debug, Clone)]
pub struct ComboBindingValues(pub Vec<SettingValue>);

/// The bundle for a checkbox bound to a boolean setting: the headless
/// [`Checkbox`] widget plus its [`SettingBinding`]. The caller adds the node's
/// styling, a [`TabIndex`] and any label.
#[must_use]
pub fn bound_checkbox(binding: SettingBinding) -> impl Bundle {
    (Checkbox, binding)
}

/// The bundle for a slider bound to a numeric setting: the headless [`Slider`]
/// widget with its range and step, seeded at the range start, plus its
/// [`SettingBinding`]. The initial value is corrected to the stored one on the
/// first sync pass; the caller adds the track/thumb nodes and a [`TabIndex`].
#[must_use]
pub fn bound_slider(binding: SettingBinding, range: SliderRange, step: SliderStep) -> impl Bundle {
    (
        Slider::default(),
        SliderValue(range.start()),
        range,
        step,
        binding,
    )
}

/// Wires the two-way binding: the write-side [`ValueChange`] observers and the
/// idempotent read/react-side sync passes.
///
/// Every system tolerates a missing [`ViewerSettings`] (an app without the store,
/// e.g. the gallery) by early-returning, so adding the plugin is always safe. It
/// also owns the `F7` demo panel, the live proof of the mechanism.
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsBindingPlugin;

impl Plugin for SettingsBindingPlugin {
    fn build(&self, app: &mut App) {
        // `ComboWidgetPlugin` / `ColorPickerPlugin` also register these
        // messages; doing it here too (idempotent) keeps the binding layer safe
        // to add standalone (tests).
        app.add_message::<ComboChanged>()
            .add_message::<ColorPicked>()
            .insert_resource(SettingsBindingDemoVisible::from_env())
            .add_observer(on_bound_checkbox_change)
            .add_observer(on_bound_slider_change)
            .add_systems(
                Startup,
                (
                    register_demo_settings,
                    setup_settings_binding_demo.after(UiScaffoldSystems::SpawnRoot),
                ),
            )
            .add_systems(
                Update,
                (
                    toggle_settings_binding_demo,
                    apply_settings_binding_demo_visibility.after(toggle_settings_binding_demo),
                    sync_bound_checkboxes,
                    sync_bound_sliders,
                    write_bound_combo_changes,
                    sync_bound_combos.after(write_bound_combo_changes),
                    write_bound_text_edits,
                    sync_bound_text_inputs.after(write_bound_text_edits),
                    write_bound_swatch_picks,
                    sync_bound_color_swatches.after(write_bound_swatch_picks),
                    drive_demo_checkbox_visual.after(sync_bound_checkboxes),
                    drive_demo_slider_visual.after(sync_bound_sliders),
                    update_settings_binding_demo_labels,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Write side: a user edit flows widget → store.
// ---------------------------------------------------------------------------

/// Observer: a bound checkbox was toggled — reflect its new `Checked` state at
/// once (so the tick tracks the click without a frame's lag) and write the bool
/// to the binding's scope.
fn on_bound_checkbox_change(
    change: On<ValueChange<bool>>,
    bindings: Query<&SettingBinding>,
    settings: Option<ResMut<ViewerSettings>>,
    mut commands: Commands,
) {
    let Ok(binding) = bindings.get(change.source) else {
        return;
    };
    reconcile_checked(&mut commands, change.source, change.value);
    if let Some(mut settings) = settings {
        settings.set(
            binding.scope(),
            binding.name(),
            SettingValue::Bool(change.value),
        );
    }
}

/// Observer: a bound slider moved — clamp the new value to the range, reflect it
/// on the widget at once, and write it to the binding's scope in the setting's
/// declared numeric type.
fn on_bound_slider_change(
    change: On<ValueChange<f32>>,
    bindings: Query<(&SettingBinding, &SliderRange)>,
    settings: Option<ResMut<ViewerSettings>>,
    mut commands: Commands,
) {
    let Ok((binding, range)) = bindings.get(change.source) else {
        return;
    };
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    let Some(mut settings) = settings else {
        return;
    };
    let Some(kind) = settings
        .store()
        .declaration(binding.name())
        .map(|d| d.kind())
    else {
        return;
    };
    match slider_value_as_setting(clamped, kind) {
        Some(value) => settings.set(binding.scope(), binding.name(), value),
        None => warn!(
            "settings_binding: slider bound to non-numeric setting {} ({kind:?})",
            binding.name()
        ),
    }
}

/// A **user** pick on a bound combo writes the picked option's value to the
/// binding's scope. Programmatic [`ComboSelection`] writes emit no
/// [`ComboChanged`], so the sync pass never loops through here.
fn write_bound_combo_changes(
    mut changes: MessageReader<ComboChanged>,
    combos: Query<(&SettingBinding, &ComboBindingValues)>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    for change in changes.read() {
        let Ok((binding, values)) = combos.get(change.combo) else {
            continue;
        };
        let Some(value) = values.0.get(change.active) else {
            warn!(
                "settings_binding: combo bound to {} picked option {} but only {} values are mapped",
                binding.name(),
                change.active,
                values.0.len()
            );
            continue;
        };
        if let Some(settings) = settings.as_mut() {
            settings.set(binding.scope(), binding.name(), value.clone());
        }
    }
}

/// The last text the binding layer itself processed for a bound field — set on
/// first sight and on every [`sync_bound_text_inputs`] write. A `Changed`
/// flag whose text still equals this shadow is an echo of the layer's own
/// write (or focus churn), not a user edit, so [`write_bound_text_edits`]
/// ignores it; only a genuine divergence counts.
#[derive(Component, Debug, Clone)]
struct BoundTextShadow(String);

/// An edit to a bound text field **while it holds focus** writes the field's
/// text to the binding's scope — the same live-edit model as the checkbox and
/// slider, so the preferences snapshot/revert covers it. The
/// [`BoundTextShadow`] comparison keeps the seeding pass
/// ([`sync_bound_text_inputs`]) from echoing its own writes back as user
/// edits — the focused/unfocused frames alone cannot tell the two apart,
/// because a `Changed` flag from an unfocused-frame seed is only *observed*
/// a frame later, by which time the field may have gained focus.
fn write_bound_text_edits(
    focus: Option<Res<InputFocus>>,
    mut fields: Query<
        (
            Entity,
            &EditableText,
            &SettingBinding,
            Option<&mut BoundTextShadow>,
        ),
        Changed<EditableText>,
    >,
    mut settings: Option<ResMut<ViewerSettings>>,
    mut commands: Commands,
) {
    let focused = focus.as_ref().and_then(|focus| focus.get());
    for (entity, editable, binding, shadow) in &mut fields {
        let value = editable.value().to_string();
        let Some(mut shadow) = shadow else {
            // First sight (the spawn frame): remember the text, write nothing —
            // the sync pass seeds the field from the store.
            commands.entity(entity).insert(BoundTextShadow(value));
            continue;
        };
        if shadow.0 == value {
            continue;
        }
        if focused != Some(entity) || editable.is_composing() {
            // Not a user edit (some other writer moved the text): note it so a
            // later comparison is against the current text, but do not write.
            shadow.0 = value;
            continue;
        }
        shadow.0.clone_from(&value);
        if let Some(settings) = settings.as_mut() {
            settings.set(binding.scope(), binding.name(), SettingValue::String(value));
        }
    }
}

/// Every [`ColorPicked`] reply to a bound colour swatch writes the colour to
/// the binding's scope — the live drag, OK, and the original-colour re-emit of
/// the picker's Cancel alike, the same live-edit model as the slider (the
/// preferences snapshot covers a preferences-level Cancel).
///
/// A pick that lands on the setting's **declared default** is written as a
/// [`ViewerSettings::reset`] instead of an override: the declared default is
/// the active skin's value (`skin_colors`), and pinning it as an
/// override would freeze the colour across a later skin switch.
fn write_bound_swatch_picks(
    mut picks: MessageReader<ColorPicked>,
    swatches: Query<&SettingBinding, With<ColorSwatchValue>>,
    mut settings: Option<ResMut<ViewerSettings>>,
) {
    for pick in picks.read() {
        let Ok(binding) = swatches.get(pick.requester) else {
            continue;
        };
        let Some(settings) = settings.as_mut() else {
            continue;
        };
        let srgba = pick.color.to_srgba();
        let rgb = [srgba.red, srgba.green, srgba.blue];
        let is_declared_default = settings
            .store()
            .declaration(binding.name())
            .and_then(|decl| decl.default().as_color3())
            .is_some_and(|default| color3_agrees(rgb, default));
        if is_declared_default {
            settings.reset(binding.scope(), binding.name());
        } else {
            settings.set(binding.scope(), binding.name(), SettingValue::Color3(rgb));
        }
    }
}

// ---------------------------------------------------------------------------
// Read / react side: the store flows store → widget, idempotently.
// ---------------------------------------------------------------------------

/// Keep every bound checkbox's `Checked` state equal to its setting's effective
/// value. Idempotent — a checkbox already agreeing is left untouched — so this is
/// safe to run every frame and picks up both a fresh spawn and any external
/// change (a reset, the account scope loading at login).
fn sync_bound_checkboxes(
    settings: Option<Res<ViewerSettings>>,
    checkboxes: Query<(Entity, &SettingBinding, Has<Checked>), With<Checkbox>>,
    mut commands: Commands,
) {
    let Some(settings) = settings else {
        return;
    };
    for (entity, binding, checked) in &checkboxes {
        let Ok(want) = settings.store().get_bool(binding.name()) else {
            continue;
        };
        if want != checked {
            reconcile_checked(&mut commands, entity, want);
        }
    }
}

/// Keep every bound slider's [`SliderValue`] equal to its setting's effective
/// value (an integer setting widened to `f32`, clamped to the range). Idempotent,
/// like `sync_bound_checkboxes`; [`SliderValue`] is an immutable component, so a
/// change is applied by re-inserting it.
fn sync_bound_sliders(
    settings: Option<Res<ViewerSettings>>,
    sliders: Query<(Entity, &SettingBinding, &SliderValue, &SliderRange), With<Slider>>,
    mut commands: Commands,
) {
    let Some(settings) = settings else {
        return;
    };
    for (entity, binding, current, range) in &sliders {
        let Some(value) = settings
            .store()
            .get(binding.name())
            .and_then(setting_as_slider_value)
        else {
            continue;
        };
        let want = range.clamp(value);
        if (want - current.0).abs() > SLIDER_SYNC_EPSILON {
            commands.entity(entity).insert(SliderValue(want));
        }
    }
}

/// Keep every bound combo showing the option whose mapped value equals the
/// setting's effective value. Idempotent: a combo already agreeing is left
/// untouched, and a stored value no option maps (e.g. a hand-edited config)
/// leaves the combo where it is rather than snapping to a wrong option.
fn sync_bound_combos(
    settings: Option<Res<ViewerSettings>>,
    mut combos: Query<(&SettingBinding, &ComboBindingValues, &mut ComboSelection)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (binding, values, mut selection) in &mut combos {
        let Some(current) = settings.store().get(binding.name()) else {
            continue;
        };
        let Some(want) = values.0.iter().position(|value| value == current) else {
            continue;
        };
        if selection.active != want {
            selection.active = want;
        }
    }
}

/// Keep every bound text field's content equal to its string setting's
/// effective value, skipping the focused or IME-composing field so an active
/// edit is never clobbered. Also the seeding pass for a freshly-spawned field.
#[expect(
    clippy::cmp_owned,
    reason = "the editor's SplitString has no borrow-free comparison against &str; the guard \
              keeps the per-frame pass write-free when nothing changed"
)]
fn sync_bound_text_inputs(
    settings: Option<Res<ViewerSettings>>,
    focus: Option<Res<InputFocus>>,
    mut fields: Query<(
        Entity,
        &mut EditableText,
        &SettingBinding,
        Option<&mut BoundTextShadow>,
    )>,
    mut commands: Commands,
) {
    let Some(settings) = settings else {
        return;
    };
    let focused = focus.as_ref().and_then(|focus| focus.get());
    for (entity, mut editable, binding, shadow) in &mut fields {
        if focused == Some(entity) || editable.is_composing() {
            continue;
        }
        let Ok(want) = settings.store().get_str(binding.name()) else {
            continue;
        };
        if editable.value().to_string() != want {
            editable.editor_mut().set_text(want);
            match shadow {
                Some(mut shadow) => want.clone_into(&mut shadow.0),
                None => {
                    commands
                        .entity(entity)
                        .insert(BoundTextShadow(want.to_owned()));
                }
            }
        }
    }
}

/// Keep every bound colour swatch's [`ColorSwatchValue`] equal to its
/// setting's effective value — the seed on spawn, and the follow on any
/// external change (a per-row reset, the account scope loading at login, a
/// skin switch moving the declared default). Idempotent within
/// [`COLOR_SYNC_EPSILON`], so the swatch fill repaint only runs on a real
/// change.
fn sync_bound_color_swatches(
    settings: Option<Res<ViewerSettings>>,
    mut swatches: Query<(&SettingBinding, &mut ColorSwatchValue)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (binding, mut value) in &mut swatches {
        let Ok(want) = settings.store().get_color3(binding.name()) else {
            continue;
        };
        let current = value.0.to_srgba();
        if !color3_agrees([current.red, current.green, current.blue], want) {
            let [red, green, blue] = want;
            value.0 = Color::srgb(red, green, blue);
        }
    }
}

/// Whether two stored sRGB triples agree within [`COLOR_SYNC_EPSILON`],
/// channel-wise.
fn color3_agrees(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| (x - y).abs() <= COLOR_SYNC_EPSILON)
}

/// The largest per-channel disagreement the colour sync treats as "already in
/// sync": half the picker's 8-bit quantisation step, so a value that
/// round-trips through the picker's byte channels still counts as equal to a
/// float default it visually matches.
const COLOR_SYNC_EPSILON: f32 = 0.5 / 255.0;

/// Add or remove the [`Checked`] marker on `entity` to match `checked`.
fn reconcile_checked(commands: &mut Commands, entity: Entity, checked: bool) {
    if checked {
        commands.entity(entity).insert(Checked);
    } else {
        commands.entity(entity).remove::<Checked>();
    }
}

/// The largest [`SliderValue`] disagreement the sync pass treats as "already in
/// sync", so float round-trip noise does not thrash the immutable component. An
/// integer-backed slider is exact and well clear of this.
const SLIDER_SYNC_EPSILON: f32 = 1.0e-4;

// ---------------------------------------------------------------------------
// Numeric conversions between the slider's `f32` and the setting's type.
// ---------------------------------------------------------------------------

/// A setting's value as the `f32` a slider shows: a float passes through, an
/// integer widens, and a non-numeric setting has no slider representation.
const fn setting_as_slider_value(value: &SettingValue) -> Option<f32> {
    match value {
        SettingValue::F32(v) => Some(*v),
        SettingValue::I32(v) => Some(i32_to_f32(*v)),
        SettingValue::U32(v) => Some(u32_to_f32(*v)),
        _ => None,
    }
}

/// A slider's `f32` as the setting's declared numeric type (rounding to the
/// nearest integer, clamped into range, for the integer kinds), or `None` for a
/// non-numeric kind.
fn slider_value_as_setting(value: f32, kind: SettingKind) -> Option<SettingValue> {
    match kind {
        SettingKind::F32 => Some(SettingValue::F32(value)),
        SettingKind::I32 => Some(SettingValue::I32(f32_to_i32(value))),
        SettingKind::U32 => Some(SettingValue::U32(f32_to_u32(value))),
        _ => None,
    }
}

/// Widen an `i32` setting to the `f32` a slider carries.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a slider-bound integer setting's magnitude is small; a wider one loses only sub-unit precision the slider cannot display anyway"
)]
const fn i32_to_f32(value: i32) -> f32 {
    value as f32
}

/// Widen a `u32` setting to the `f32` a slider carries.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a slider-bound integer setting's magnitude is small; a wider one loses only sub-unit precision the slider cannot display anyway"
)]
const fn u32_to_f32(value: u32) -> f32 {
    value as f32
}

/// Round a slider's `f32` to the nearest `i32`, clamping to the `i32` range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "the value is clamped into the i32 range and rounded before the cast, so it neither truncates meaningfully nor is out of range"
)]
fn f32_to_i32(value: f32) -> i32 {
    let rounded = value.round();
    if rounded <= i32_to_f32(i32::MIN) {
        i32::MIN
    } else if rounded >= i32_to_f32(i32::MAX) {
        i32::MAX
    } else {
        rounded as i32
    }
}

/// Round a slider's `f32` to the nearest `u32`, clamping to the `u32` range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::as_conversions,
    reason = "the value is clamped into the u32 range (negatives to zero) and rounded before the cast"
)]
fn f32_to_u32(value: f32) -> u32 {
    let rounded = value.round();
    if rounded <= 0.0 {
        0
    } else if rounded >= u32_to_f32(u32::MAX) {
        u32::MAX
    } else {
        rounded as u32
    }
}

// ---------------------------------------------------------------------------
// The `F7` demo panel: a live proof of the mechanism (in the pattern of the
// `F5` scaffold and `F6` i18n demos). A checkbox and a slider are each bound to
// a runtime-only setting; a "Reset" button drives them from outside to show the
// react-to-external-change path. Modelled on `ui`'s `setup_ui_demo`.
// ---------------------------------------------------------------------------

/// The key that toggles the demo panel on and off.
const DEMO_TOGGLE_KEY: KeyCode = KeyCode::F7;

/// The environment variable that starts the demo shown, for the offline
/// screenshot harness (as `SL_VIEWER_UI_DEMO` does for the scaffold demo).
const DEMO_ENV: &str = "SL_VIEWER_SETTINGS_BINDING_DEMO";

/// The boolean demo setting the checkbox is bound to (a global preference).
const DEMO_FLAG_SETTING: &str = "DemoBindingFlag";

/// The float demo setting the slider is bound to (a per-account preference, so
/// the two scopes are both exercised).
const DEMO_LEVEL_SETTING: &str = "DemoBindingLevel";

/// The default of [`DEMO_LEVEL_SETTING`], and the value "Reset" returns to.
const DEMO_LEVEL_DEFAULT: f32 = 40.0;

/// The inclusive slider range for [`DEMO_LEVEL_SETTING`].
const DEMO_LEVEL_MIN: f32 = 0.0;
/// The upper bound of the demo slider; see [`DEMO_LEVEL_MIN`].
const DEMO_LEVEL_MAX: f32 = 100.0;

/// The demo slider track's width, in logical pixels.
const DEMO_TRACK_WIDTH: f32 = 220.0;
/// The demo slider thumb's width, in logical pixels.
const DEMO_THUMB_WIDTH: f32 = 14.0;
/// The demo slider track and thumb height, in logical pixels.
const DEMO_TRACK_HEIGHT: f32 = 16.0;

/// The demo checkbox box's side length, in logical pixels.
const DEMO_CHECK_SIZE: f32 = 18.0;

/// The demo panel's inset from the top-leading corner, clear of the
/// `F3`/`F4`/`F5`/`F6` overlays.
const DEMO_PANEL_MARGIN: f32 = 260.0;

/// The demo panel's instruction-line font size, in logical pixels.
const DEMO_TITLE_FONT_SIZE: f32 = 13.0;
/// The demo panel's body font size, in logical pixels.
const DEMO_FONT_SIZE: f32 = 15.0;

/// The one-line instruction shown above the demo's controls.
const DEMO_TITLE: &str = "Settings binding demo (F7) - toggle / drag to write the store; Reset \
    drives both from outside";

/// The demo panel's translucent backdrop, matching the other demos'.
const DEMO_PANEL_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
/// The demo's heading / label colour.
const DEMO_TEXT_COLOR: Color = Color::srgb(0.82, 0.87, 0.94);
/// A demo control's border. The keyboard focus ring is the shared outline the
/// skin draws on any focusable widget (`viewer-ui-focus-ring-visible`), not a
/// recolour of this border.
const DEMO_CONTROL_BORDER: Color = Color::srgb(0.40, 0.50, 0.62);
/// The checkbox box's fill when unchecked.
const DEMO_CHECK_OFF: Color = Color::srgb(0.12, 0.14, 0.18);
/// The checkbox box's fill when checked.
const DEMO_CHECK_ON: Color = Color::srgb(0.30, 0.70, 0.45);
/// The slider track's fill.
const DEMO_TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// The slider thumb's fill.
const DEMO_THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);
/// The reset button's background.
const DEMO_BUTTON_BACKGROUND: Color = Color::srgb(0.16, 0.19, 0.25);

/// Whether the demo panel is currently shown.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct SettingsBindingDemoVisible(bool);

impl SettingsBindingDemoVisible {
    /// Seed from `DEMO_ENV`: any non-empty value starts the panel shown.
    fn from_env() -> Self {
        Self(std::env::var_os(DEMO_ENV).is_some_and(|value| !value.is_empty()))
    }
}

/// A marker on the demo panel's root node.
#[derive(Component, Debug, Clone, Copy)]
struct SettingsBindingDemoRoot;

/// A marker on the demo checkbox's box node, so its fill tracks `Checked`.
#[derive(Component, Debug, Clone, Copy)]
struct DemoCheckboxBox;

/// A marker on the demo slider's thumb node, so it slides to the bound value.
#[derive(Component, Debug, Clone, Copy)]
struct DemoSliderThumb;

/// Which of the demo's live labels a `Text` node is.
#[derive(Component, Debug, Clone, Copy)]
enum DemoLabel {
    /// Reports [`DEMO_FLAG_SETTING`]'s current value.
    Flag,
    /// Reports [`DEMO_LEVEL_SETTING`]'s current value.
    Level,
}

/// Startup: register the two runtime-only demo settings (never persisted, so the
/// demo writes no junk to the user's config).
fn register_demo_settings(settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.register_transient(
        DEMO_FLAG_SETTING,
        SettingValue::Bool(true),
        "Demo bound flag",
    );
    settings.register_transient(
        DEMO_LEVEL_SETTING,
        SettingValue::F32(DEMO_LEVEL_DEFAULT),
        "Demo bound level",
    );
}

/// Startup: spawn the demo panel under [`UiRoot`] (after the root exists).
fn setup_settings_binding_demo(
    mut commands: Commands,
    visible: Res<SettingsBindingDemoVisible>,
    root: Res<UiRoot>,
) {
    let display = if visible.0 {
        Display::Flex
    } else {
        Display::None
    };
    commands
        .spawn((
            Node {
                display,
                padding: UiRect::all(Val::Px(12.0)),
                max_width: Val::Px(420.0),
                ..column(Val::Px(10.0))
            },
            LogicalMargin(LogicalRect {
                inline_start: Val::Px(DEMO_PANEL_MARGIN),
                block_start: Val::Px(DEMO_PANEL_MARGIN),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(DEMO_PANEL_BACKGROUND),
            UiPanelShown(visible.0),
            SettingsBindingDemoRoot,
            ChildOf(root.0),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(DEMO_TITLE),
                UiFont::Sans.at(DEMO_TITLE_FONT_SIZE),
                TextColor(DEMO_TEXT_COLOR),
            ));
            spawn_demo_checkbox_row(panel);
            spawn_demo_slider_row(panel);
            spawn_demo_reset_button(panel);
        });
}

/// The checkbox row: the bound [`Checkbox`] (whose box fill shows `Checked`) and
/// a live label of the bound setting.
fn spawn_demo_checkbox_row(panel: &mut RelatedSpawnerCommands<'_, ChildOf>) {
    panel
        .spawn(Node {
            align_items: AlignItems::Center,
            ..row(Val::Px(8.0))
        })
        .with_children(|check_row| {
            check_row.spawn((
                bound_checkbox(SettingBinding::global(DEMO_FLAG_SETTING)),
                Node {
                    width: Val::Px(DEMO_CHECK_SIZE),
                    height: Val::Px(DEMO_CHECK_SIZE),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(DEMO_CONTROL_BORDER),
                BackgroundColor(DEMO_CHECK_OFF),
                TabIndex(0),
                DemoCheckboxBox,
            ));
            check_row.spawn((
                Text::default(),
                UiFont::Sans.at(DEMO_FONT_SIZE),
                TextColor(DEMO_TEXT_COLOR),
                DemoLabel::Flag,
            ));
        });
}

/// The slider row: the bound [`Slider`] track with a thumb that slides to the
/// bound value, and a live numeric label.
fn spawn_demo_slider_row(panel: &mut RelatedSpawnerCommands<'_, ChildOf>) {
    panel
        .spawn(Node {
            align_items: AlignItems::Center,
            ..row(Val::Px(8.0))
        })
        .with_children(|slider_row| {
            slider_row
                .spawn((
                    bound_slider(
                        SettingBinding::account(DEMO_LEVEL_SETTING),
                        SliderRange::new(DEMO_LEVEL_MIN, DEMO_LEVEL_MAX),
                        SliderStep(1.0),
                    ),
                    Node {
                        width: Val::Px(DEMO_TRACK_WIDTH),
                        height: Val::Px(DEMO_TRACK_HEIGHT),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(DEMO_CONTROL_BORDER),
                    BackgroundColor(DEMO_TRACK_FILL),
                    TabIndex(0),
                ))
                .with_children(|track| {
                    track.spawn((
                        SliderThumb,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(DEMO_THUMB_WIDTH),
                            height: Val::Px(DEMO_TRACK_HEIGHT),
                            ..default()
                        },
                        LogicalInset(LogicalRect {
                            inline_start: Val::Px(0.0),
                            ..LogicalRect::ZERO
                        }),
                        BackgroundColor(DEMO_THUMB_FILL),
                        DemoSliderThumb,
                    ));
                });
            slider_row.spawn((
                Text::default(),
                UiFont::Sans.at(DEMO_FONT_SIZE),
                TextColor(DEMO_TEXT_COLOR),
                DemoLabel::Level,
            ));
        });
}

/// The "Reset" button: an external writer that returns both demo settings to
/// their defaults, so the widgets visibly follow a change they did not make.
fn spawn_demo_reset_button(panel: &mut RelatedSpawnerCommands<'_, ChildOf>) {
    panel
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(DEMO_CONTROL_BORDER),
            BackgroundColor(DEMO_BUTTON_BACKGROUND),
        ))
        .observe(reset_demo_settings)
        .with_child((
            Text::new("Reset to defaults"),
            UiFont::Sans.at(DEMO_FONT_SIZE),
            TextColor(DEMO_TEXT_COLOR),
        ));
}

/// Observer: reset both demo settings, in their own scopes, so the sync passes
/// drive the checkbox and slider back to the declared defaults.
fn reset_demo_settings(_activate: On<Activate>, settings: Option<ResMut<ViewerSettings>>) {
    let Some(mut settings) = settings else {
        return;
    };
    settings.reset(Scope::Global, DEMO_FLAG_SETTING);
    settings.reset(Scope::Account, DEMO_LEVEL_SETTING);
}

/// Toggle the demo panel when `DEMO_TOGGLE_KEY` is pressed.
fn toggle_settings_binding_demo(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<SettingsBindingDemoVisible>,
) {
    if keyboard.just_pressed(DEMO_TOGGLE_KEY) {
        visible.0 = !visible.0;
    }
}

/// Drive the demo panel's [`UiPanelShown`] from [`SettingsBindingDemoVisible`],
/// leaving `ui::apply_panel_visibility` to do the hiding.
fn apply_settings_binding_demo_visibility(
    visible: Res<SettingsBindingDemoVisible>,
    mut panels: Query<&mut UiPanelShown, With<SettingsBindingDemoRoot>>,
) {
    if !visible.is_changed() {
        return;
    }
    for mut shown in &mut panels {
        if shown.0 != visible.0 {
            shown.0 = visible.0;
        }
    }
}

/// Colour the demo checkbox's box from its `Checked` state.
///
/// The keyboard focus ring these controls once painted by hand is now the shared
/// outline the skin draws on every focusable widget
/// (`viewer-ui-focus-ring-visible`), so nothing here touches the border.
fn drive_demo_checkbox_visual(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<DemoCheckboxBox>>,
) {
    for (mut fill, checked) in &mut boxes {
        let target = BackgroundColor(if checked {
            DEMO_CHECK_ON
        } else {
            DEMO_CHECK_OFF
        });
        if fill.0 != target.0 {
            *fill = target;
        }
    }
}

/// Slide each demo thumb to its slider's [`SliderValue`] within the range.
fn drive_demo_slider_visual(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<Slider>>,
    mut thumbs: Query<&mut LogicalInset, With<DemoSliderThumb>>,
) {
    for (value, range, children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (DEMO_TRACK_WIDTH - DEMO_THUMB_WIDTH);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child) {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

/// Keep the demo's two labels showing the settings' current values, so the
/// store's state is visible independently of the widgets.
fn update_settings_binding_demo_labels(
    settings: Option<Res<ViewerSettings>>,
    mut labels: Query<(&mut Text, &DemoLabel)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (mut text, which) in &mut labels {
        let wanted = match which {
            DemoLabel::Flag => {
                let on = settings
                    .store()
                    .get_bool(DEMO_FLAG_SETTING)
                    .unwrap_or(false);
                format!("Flag (global): {}", if on { "on" } else { "off" })
            }
            DemoLabel::Level => {
                let level = settings
                    .store()
                    .get(DEMO_LEVEL_SETTING)
                    .and_then(setting_as_slider_value)
                    .unwrap_or(DEMO_LEVEL_DEFAULT);
                format!("Level (account): {level:.0}")
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::input_focus::InputFocus;
    use bevy::prelude::*;
    use bevy::text::EditableText;
    use bevy::ui::Checked;
    use bevy::ui_widgets::{SliderRange, SliderStep, SliderValue, ValueChange};
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};

    use super::{
        ComboBindingValues, SettingBinding, bound_checkbox, bound_slider, f32_to_i32, f32_to_u32,
        on_bound_checkbox_change, on_bound_slider_change, setting_as_slider_value,
        slider_value_as_setting, sync_bound_checkboxes, sync_bound_color_swatches,
        sync_bound_combos, sync_bound_sliders, sync_bound_text_inputs, write_bound_combo_changes,
        write_bound_swatch_picks, write_bound_text_edits,
    };
    use crate::ui_color_picker::{ColorPicked, ColorSwatchValue};
    use crate::ui_combo::{ComboChanged, ComboSelection};
    use sl_viewer_settings::ViewerSettings;

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A headless app with the widget observers, the binding write observers and
    /// the sync passes, over a store pre-populated by `register`.
    fn app(register: impl FnOnce(&mut SettingsStore)) -> App {
        let mut store = SettingsStore::new();
        register(&mut store);
        let mut app = App::new();
        // The mechanism is driven by triggering `ValueChange` / writing
        // `ComboChanged` / editing the field directly (the widget plugins'
        // pointer/focus path is theirs to test), so the harness needs only a
        // schedule runner and the store.
        app.add_plugins(MinimalPlugins)
            .add_message::<ComboChanged>()
            .add_message::<ColorPicked>()
            .init_resource::<InputFocus>()
            .insert_resource(ViewerSettings::from_store_for_test(store))
            .add_observer(on_bound_checkbox_change)
            .add_observer(on_bound_slider_change)
            .add_systems(
                Update,
                (
                    sync_bound_checkboxes,
                    sync_bound_sliders,
                    write_bound_combo_changes,
                    sync_bound_combos.after(write_bound_combo_changes),
                    write_bound_text_edits,
                    sync_bound_text_inputs.after(write_bound_text_edits),
                    write_bound_swatch_picks,
                    sync_bound_color_swatches.after(write_bound_swatch_picks),
                ),
            );
        app
    }

    /// A read-only view of the store the app is driving.
    fn store(app: &App) -> &SettingsStore {
        app.world().resource::<ViewerSettings>().store()
    }

    /// Assert two `f32`s are equal within a small tolerance.
    fn approx(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1.0e-4, "{actual} != {expected}");
    }

    /// A bound checkbox reads its setting's value on the first sync pass.
    #[test]
    fn checkbox_reads_setting_on_build() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(true), "a toggle")
                .ok();
        });
        let checkbox = app
            .world_mut()
            .spawn(bound_checkbox(SettingBinding::global("Flag")))
            .id();
        app.update();
        assert!(app.world().entity(checkbox).contains::<Checked>());
        Ok(())
    }

    /// Toggling a bound checkbox writes the new value to its scope.
    #[test]
    fn checkbox_writes_setting_on_change() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
        });
        let checkbox = app
            .world_mut()
            .spawn(bound_checkbox(SettingBinding::global("Flag")))
            .id();
        app.update();
        app.world_mut().trigger(ValueChange {
            source: checkbox,
            value: true,
            is_final: true,
        });
        app.update();
        assert!(store(&app).get_bool("Flag")?);
        assert!(app.world().entity(checkbox).contains::<Checked>());
        Ok(())
    }

    /// An external change to the store (e.g. a reset) is reflected on the widget.
    #[test]
    fn checkbox_follows_external_change() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
        });
        let checkbox = app
            .world_mut()
            .spawn(bound_checkbox(SettingBinding::global("Flag")))
            .id();
        app.update();
        assert!(!app.world().entity(checkbox).contains::<Checked>());
        // Move the store from outside the widget entirely.
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            "Flag",
            SettingValue::Bool(true),
        );
        app.update();
        assert!(app.world().entity(checkbox).contains::<Checked>());
        Ok(())
    }

    /// A bound slider reads its (float) setting on build and writes it on change.
    #[test]
    fn slider_reads_and_writes_f32() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Level", SettingValue::F32(25.0), "a level")
                .ok();
        });
        let slider = app
            .world_mut()
            .spawn(bound_slider(
                SettingBinding::global("Level"),
                SliderRange::new(0.0, 100.0),
                SliderStep(1.0),
            ))
            .id();
        app.update();
        approx(
            app.world()
                .entity(slider)
                .get::<SliderValue>()
                .map_or(0.0, |v| v.0),
            25.0,
        );

        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 60.0_f32,
            is_final: true,
        });
        app.update();
        approx(store(&app).get_f32("Level")?, 60.0);
        Ok(())
    }

    /// An integer setting round-trips through the slider's `f32`: writes round to
    /// the nearest integer, reads widen back.
    #[test]
    fn slider_rounds_for_integer_setting() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Count", SettingValue::I32(3), "a count")
                .ok();
        });
        let slider = app
            .world_mut()
            .spawn(bound_slider(
                SettingBinding::global("Count"),
                SliderRange::new(0.0, 10.0),
                SliderStep(1.0),
            ))
            .id();
        app.update();
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 6.7_f32,
            is_final: true,
        });
        app.update();
        assert_eq!(store(&app).get_i32("Count")?, 7);
        Ok(())
    }

    /// Two widgets bound to one setting stay in step: editing one moves the other
    /// through the store.
    #[test]
    fn two_checkboxes_share_one_setting() -> Result<(), TestError> {
        let mut app = app(|store| {
            store
                .register("Flag", SettingValue::Bool(false), "a toggle")
                .ok();
        });
        let a = app
            .world_mut()
            .spawn(bound_checkbox(SettingBinding::global("Flag")))
            .id();
        let b = app
            .world_mut()
            .spawn(bound_checkbox(SettingBinding::global("Flag")))
            .id();
        app.update();
        app.world_mut().trigger(ValueChange {
            source: a,
            value: true,
            is_final: true,
        });
        app.update();
        assert!(app.world().entity(a).contains::<Checked>());
        assert!(app.world().entity(b).contains::<Checked>());
        Ok(())
    }

    /// The three-option string combo the combo tests bind: PG / M / A.
    fn spawn_maturity_combo(app: &mut App, active: usize) -> Entity {
        app.world_mut()
            .spawn((
                ComboSelection {
                    element: "test-combo",
                    active,
                },
                SettingBinding::global("Maturity"),
                ComboBindingValues(vec![
                    SettingValue::String("PG".to_owned()),
                    SettingValue::String("M".to_owned()),
                    SettingValue::String("A".to_owned()),
                ]),
            ))
            .id()
    }

    /// Register the string setting the combo tests use.
    fn register_maturity(store: &mut SettingsStore, default: &str) {
        store
            .register(
                "Maturity",
                SettingValue::String(default.to_owned()),
                "a rating",
            )
            .ok();
    }

    /// A bound combo moves to the option matching the stored value on the first
    /// sync pass.
    #[test]
    fn combo_reads_setting_on_build() -> Result<(), TestError> {
        let mut app = app(|store| register_maturity(store, "M"));
        let combo = spawn_maturity_combo(&mut app, 0);
        app.update();
        assert_eq!(
            app.world()
                .entity(combo)
                .get::<ComboSelection>()
                .map(|s| s.active),
            Some(1)
        );
        Ok(())
    }

    /// A user pick writes the picked option's mapped value to the store.
    #[test]
    fn combo_user_pick_writes_setting() -> Result<(), TestError> {
        let mut app = app(|store| register_maturity(store, "PG"));
        let combo = spawn_maturity_combo(&mut app, 0);
        app.update();
        app.world_mut()
            .write_message(ComboChanged { combo, active: 2 });
        app.update();
        assert_eq!(store(&app).get_str("Maturity")?, "A");
        Ok(())
    }

    /// An external store change moves the combo's selection.
    #[test]
    fn combo_follows_external_change() -> Result<(), TestError> {
        let mut app = app(|store| register_maturity(store, "PG"));
        let combo = spawn_maturity_combo(&mut app, 0);
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            "Maturity",
            SettingValue::String("A".to_owned()),
        );
        app.update();
        assert_eq!(
            app.world()
                .entity(combo)
                .get::<ComboSelection>()
                .map(|s| s.active),
            Some(2)
        );
        Ok(())
    }

    /// A stored value no option maps (a hand-edited config) leaves the combo
    /// showing what it showed.
    #[test]
    fn combo_ignores_unknown_stored_value() -> Result<(), TestError> {
        let mut app = app(|store| register_maturity(store, "made-up"));
        let combo = spawn_maturity_combo(&mut app, 1);
        app.update();
        assert_eq!(
            app.world()
                .entity(combo)
                .get::<ComboSelection>()
                .map(|s| s.active),
            Some(1)
        );
        Ok(())
    }

    /// Register the string setting the text tests use.
    fn register_reply(store: &mut SettingsStore, default: &str) {
        store
            .register(
                "Reply",
                SettingValue::String(default.to_owned()),
                "a reply text",
            )
            .ok();
    }

    /// A bound text field is seeded from the store while unfocused.
    #[test]
    fn text_seeds_from_store_when_unfocused() -> Result<(), TestError> {
        let mut app = app(|store| register_reply(store, "stored text"));
        let field = app
            .world_mut()
            .spawn((EditableText::new(""), SettingBinding::account("Reply")))
            .id();
        app.update();
        let text = app
            .world()
            .entity(field)
            .get::<EditableText>()
            .map(|editable| editable.value().to_string());
        assert_eq!(text.as_deref(), Some("stored text"));
        Ok(())
    }

    /// An edit made while the field holds focus writes to the store.
    #[test]
    fn text_focused_edit_writes_setting() -> Result<(), TestError> {
        let mut app = app(|store| register_reply(store, "before"));
        let field = app
            .world_mut()
            .spawn((EditableText::new(""), SettingBinding::account("Reply")))
            .id();
        app.update();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(field));
        if let Some(mut editable) = app.world_mut().get_mut::<EditableText>(field) {
            editable.editor_mut().set_text("after");
        }
        app.update();
        assert_eq!(store(&app).get_str("Reply")?, "after");
        Ok(())
    }

    /// An external store change does not clobber a focused field's edit in
    /// progress, and lands once focus leaves.
    #[test]
    fn text_focused_field_not_clobbered() -> Result<(), TestError> {
        let mut app = app(|store| register_reply(store, "original"));
        let field = app
            .world_mut()
            .spawn((EditableText::new(""), SettingBinding::account("Reply")))
            .id();
        app.update();
        app.world_mut()
            .insert_resource(InputFocus::from_entity(field));
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Account,
            "Reply",
            SettingValue::String("external".to_owned()),
        );
        app.update();
        let while_focused = app
            .world()
            .entity(field)
            .get::<EditableText>()
            .map(|editable| editable.value().to_string());
        assert_eq!(while_focused.as_deref(), Some("original"));

        app.world_mut().insert_resource(InputFocus::default());
        app.update();
        let after_blur = app
            .world()
            .entity(field)
            .get::<EditableText>()
            .map(|editable| editable.value().to_string());
        assert_eq!(after_blur.as_deref(), Some("external"));
        Ok(())
    }

    /// Assert two `f32` slices are element-wise equal within the colour sync
    /// epsilon.
    fn approx_rgb(actual: [f32; 3], expected: [f32; 3]) {
        for (got, want) in actual.iter().zip(expected.iter()) {
            assert!(
                (got - want).abs() <= super::COLOR_SYNC_EPSILON,
                "{actual:?} != {expected:?}"
            );
        }
    }

    /// The stored sRGB triple of a swatch's current colour.
    fn swatch_rgb(app: &App, swatch: Entity) -> [f32; 3] {
        let srgba = app
            .world()
            .entity(swatch)
            .get::<ColorSwatchValue>()
            .map_or(Color::NONE, |value| value.0)
            .to_srgba();
        [srgba.red, srgba.green, srgba.blue]
    }

    /// Register the colour setting the swatch tests bind.
    fn register_tint(store: &mut SettingsStore) {
        store
            .register("Tint", SettingValue::Color3([0.2, 0.4, 0.6]), "a colour")
            .ok();
    }

    /// A bound swatch is seeded from the store's effective colour on the first
    /// sync pass.
    #[test]
    fn swatch_reads_setting_on_build() -> Result<(), TestError> {
        let mut app = app(register_tint);
        let swatch = app
            .world_mut()
            .spawn((
                ColorSwatchValue(Color::WHITE),
                SettingBinding::account("Tint"),
            ))
            .id();
        app.update();
        approx_rgb(swatch_rgb(&app, swatch), [0.2, 0.4, 0.6]);
        Ok(())
    }

    /// Every pick — the live drag and the final OK alike — writes the colour to
    /// the binding's scope.
    #[test]
    fn swatch_pick_writes_setting_live_and_final() -> Result<(), TestError> {
        let mut app = app(register_tint);
        let swatch = app
            .world_mut()
            .spawn((
                ColorSwatchValue(Color::WHITE),
                SettingBinding::account("Tint"),
            ))
            .id();
        app.update();

        // A live-preview (non-final) pick already lands in the store.
        app.world_mut().write_message(ColorPicked {
            requester: swatch,
            color: Color::srgb(1.0, 0.0, 0.0),
            final_pick: false,
        });
        app.update();
        approx_rgb(store(&app).get_color3("Tint")?, [1.0, 0.0, 0.0]);
        assert!(store(&app).is_overridden("Tint"));

        app.world_mut().write_message(ColorPicked {
            requester: swatch,
            color: Color::srgb(0.0, 1.0, 0.0),
            final_pick: true,
        });
        app.update();
        approx_rgb(store(&app).get_color3("Tint")?, [0.0, 1.0, 0.0]);
        // The swatch fill followed through the store.
        approx_rgb(swatch_rgb(&app, swatch), [0.0, 1.0, 0.0]);
        Ok(())
    }

    /// A pick equal to the declared default clears the override instead of
    /// pinning it, so the colour keeps following the (skin-supplied) default.
    #[test]
    fn swatch_pick_of_default_resets_override() -> Result<(), TestError> {
        let mut app = app(register_tint);
        let swatch = app
            .world_mut()
            .spawn((
                ColorSwatchValue(Color::WHITE),
                SettingBinding::account("Tint"),
            ))
            .id();
        app.update();
        app.world_mut().write_message(ColorPicked {
            requester: swatch,
            color: Color::srgb(1.0, 0.0, 0.0),
            final_pick: true,
        });
        app.update();
        assert!(store(&app).is_overridden("Tint"));

        app.world_mut().write_message(ColorPicked {
            requester: swatch,
            color: Color::srgb(0.2, 0.4, 0.6),
            final_pick: true,
        });
        app.update();
        assert!(
            !store(&app).is_overridden("Tint"),
            "a pick of the default must reset, not override"
        );
        Ok(())
    }

    /// An external reset moves the swatch back to the declared default.
    #[test]
    fn swatch_follows_external_reset() -> Result<(), TestError> {
        let mut app = app(register_tint);
        let swatch = app
            .world_mut()
            .spawn((
                ColorSwatchValue(Color::WHITE),
                SettingBinding::account("Tint"),
            ))
            .id();
        app.update();
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Account,
            "Tint",
            SettingValue::Color3([0.9, 0.1, 0.1]),
        );
        app.update();
        approx_rgb(swatch_rgb(&app, swatch), [0.9, 0.1, 0.1]);

        app.world_mut()
            .resource_mut::<ViewerSettings>()
            .reset(Scope::Account, "Tint");
        app.update();
        approx_rgb(swatch_rgb(&app, swatch), [0.2, 0.4, 0.6]);
        Ok(())
    }

    /// The numeric conversions clamp and round at the type boundaries.
    #[test]
    fn numeric_conversions_clamp_and_round() {
        assert_eq!(f32_to_i32(6.7), 7);
        assert_eq!(f32_to_i32(-6.7), -7);
        assert_eq!(f32_to_i32(1.0e12), i32::MAX);
        assert_eq!(f32_to_i32(-1.0e12), i32::MIN);
        assert_eq!(f32_to_u32(-5.0), 0);
        assert_eq!(f32_to_u32(6.4), 6);
        assert_eq!(f32_to_u32(1.0e12), u32::MAX);
    }

    /// A non-numeric setting has no slider representation, in either direction.
    #[test]
    fn non_numeric_settings_have_no_slider_value() {
        assert_eq!(
            setting_as_slider_value(&SettingValue::String("x".to_owned())),
            None
        );
        assert_eq!(
            slider_value_as_setting(1.0, SettingValue::String(String::new()).kind()),
            None
        );
    }
}
