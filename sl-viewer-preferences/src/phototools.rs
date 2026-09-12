//! The **Phototools** window (`viewer-phototools`): the one panel a
//! photographer keeps open beside the shot — the environment on one tab and the
//! render knobs that change the *look* on the others, so neither Preferences nor
//! the environment editor has to be opened between frames.
//!
//! Firestorm's `floater_phototools.xml` is the model, and it is telling that it
//! is the largest single XUI layout in that viewer: photographers live in it. It
//! is also, in the reference, **the same C++ class as Quick Preferences**
//! (`FloaterQuickPrefs`, keyed on the floater's name) with a different layout —
//! which is the architecture this module keeps. Phototools is a *second curated
//! view* over the settings store, not a second implementation of the controls.
//!
//! # A table, not a pile of controls
//!
//! Every graphics row here is one row of one static table: a Fluent
//! label key, the setting it binds, the scope an edit writes to, and which
//! control draws it. The build walks the table, the tests walk the table, and a
//! row that named a setting the store does not declare — or drew a checkbox over
//! a number — is a **test failure**, not a control that quietly does nothing.
//!
//! The rows deliberately reuse the Preferences graphics tab's own label keys and
//! its `(option, value)` lists ([`crate::preferences_graphics`]). That is the
//! point of a view: *Shadow detail* is one setting with one name and one set of
//! levels, whichever window the user reached it through. A second spelling of
//! the option list would be a second place for a level to be added to, and the
//! window that missed it would bind a value nothing matches.
//!
//! # The environment tab is not settings
//!
//! The environment controls do not touch the store at all — they drive the live
//! [`EnvironmentState`] the World ▸ Environment menu drives, so a pin made here
//! is the same pin the menu shows a check mark for. They are therefore built by
//! hand rather than from the table, and they
//! carry the same `@setenv` restriction the menu entries do: a collar holding
//! the sky takes the buttons here too.
//!
//! What this window deliberately does **not** do is grow a second sun-and-moon
//! column. The full local override — the colour swatches, the atmosphere
//! sliders, the trackballs — is the Personal Lighting window
//! (`viewer-environment-personal-lighting`), and the reference's own Phototools
//! answers the same way: a **Personal Lighting** button. Duplicating that column
//! is the "parallel pile of controls" this task exists to avoid.
//!
//! Reference (Firestorm, read-only): `floater_phototools.xml`, `quickprefs.cpp`
//! (`FloaterQuickPrefs::getIsPhototools`), `menu_viewer.xml`
//! (World ▸ Photo and Video ▸ Phototools, `alt|P`).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui::{Checked, InteractionDisabled};
use bevy::ui_widgets::{
    Activate, Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue, ValueChange,
};
use sl_client_bevy::{EnvironmentAsset, SkySettings};
use sl_settings::{Scope, SettingKind, SettingValue};
use sl_viewer_environment::knobs::{AimKnobs, SkyKnob};
use sl_viewer_environment::rows::{
    AimTrackball, RowsPlugin, spawn_slider, spawn_trackball_row, tag_aim_slider,
};
use sl_viewer_ui_widgets::ui_trackball::TrackballAim;

use crate::environment::{EnvironmentState, FixedEnvironment};
use crate::floater::{
    DeferredFloaterContent, Floater, FloaterCaps, FloaterHandle, FloaterSpec, floater_panel,
    spawn_floater, toggle_floater,
};
use crate::i18n::Translated;
use crate::personal_lighting::PERSONAL_LIGHTING_FLOATER_ID;
use crate::preferences_graphics::QualityTierControl;
use crate::settings::ViewerSettings;
use crate::settings_binding::{ComboBindingValues, SettingBinding, bound_checkbox, bound_slider};
use crate::sky::day_position;
use crate::sky_presets::FixedSky;
use crate::ui::{LogicalInset, LogicalRect, UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_combo::{ComboSelection, ComboSpec, spawn_combo};
use crate::ui_element::ElementCx;
use crate::ui_font::UiFont;
use crate::ui_tab::{
    DEFAULT_ELLIPSIS, TabPlacement, TabSpec, fill_tab_container, spawn_tab_container,
};
use crate::world_api::rlv::{RlvSession, can_change_environment};

/// The stable floater id: its geometry-persistence key, its menu-toggle handle,
/// and its id in the floater registry and the sweeps.
pub const PHOTOTOOLS_FLOATER_ID: &str = "phototools";

/// The body font size, in logical pixels.
const FONT: f32 = 13.0;
/// A section heading's font size.
const SECTION_FONT: f32 = 14.0;
/// The gap between rows.
const ROW_GAP: f32 = 8.0;

/// A slider track's width, in logical pixels.
const TRACK_WIDTH: f32 = 120.0;
/// A slider thumb's width, in logical pixels.
const THUMB_WIDTH: f32 = 12.0;
/// A slider track's / thumb's height, in logical pixels.
const TRACK_HEIGHT: f32 = 14.0;
/// A checkbox box's side, in logical pixels.
const CHECK_SIZE: f32 = 16.0;
/// The minimum width of a slider row's trailing value readout.
const VALUE_WIDTH: f32 = 44.0;

/// A section heading's colour.
const SECTION_COLOR: Color = Color::srgb(0.78, 0.83, 0.9);
/// A row label's colour.
const LABEL_COLOR: Color = Color::srgb(0.86, 0.88, 0.92);
/// A label's colour while the control beside it is refused.
const DIM_LABEL_COLOR: Color = Color::srgb(0.52, 0.56, 0.63);
/// A value readout's colour.
const VALUE_COLOR: Color = Color::srgb(0.7, 0.74, 0.82);
/// A control's border.
const CONTROL_BORDER: Color = Color::srgb(0.4, 0.5, 0.62);
/// A slider track's fill.
const TRACK_FILL: Color = Color::srgb(0.16, 0.19, 0.25);
/// A slider thumb's fill.
const THUMB_FILL: Color = Color::srgb(0.62, 0.72, 0.86);
/// A checkbox box's fill while unchecked.
const CHECK_OFF: Color = Color::srgb(0.12, 0.14, 0.18);
/// A checkbox box's fill while checked.
const CHECK_ON: Color = Color::srgb(0.3, 0.7, 0.45);
/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.3, 0.34, 0.42);
/// A button's fill.
const BUTTON_FILL: Color = Color::srgb(0.16, 0.17, 0.2);
/// The fill of the time button whose environment is the one in force.
const BUTTON_ACTIVE_FILL: Color = Color::srgb(0.24, 0.32, 0.45);
/// A refused button's fill.
const BUTTON_DISABLED_FILL: Color = Color::srgb(0.13, 0.14, 0.16);

// ---------------------------------------------------------------------------
// The table.
// ---------------------------------------------------------------------------

/// How a [`PhotoRow`] draws its setting.
enum PhotoControl {
    /// A boolean setting, as a checkbox.
    Check,
    /// A numeric setting, as a slider with a trailing readout.
    Slider {
        /// The slider's inclusive minimum.
        min: f32,
        /// The slider's inclusive maximum.
        max: f32,
        /// The slider's step.
        step: f32,
        /// Whether the readout shows whole numbers.
        integer: bool,
    },
    /// An enumerated setting, as a combo over a **shared** option list — a
    /// function rather than a literal so the Preferences graphics tab and this
    /// window cannot drift apart on what the levels are.
    Combo(fn() -> Vec<(&'static str, SettingValue)>),
    /// The quality-tier combo: a [`Combo`](Self::Combo) whose anchor also
    /// carries [`QualityTierControl`], so a pick here runs the same applier the
    /// graphics tab's row does.
    QualityTier,
}

/// One row: a setting drawn with a control.
struct PhotoRow {
    /// The row's Fluent label key.
    label: &'static str,
    /// The element id the control reports, and the prefix of its node names.
    /// Spelled out rather than derived because a combo's element id must be
    /// `&'static` — and because it must *not* collide with the Preferences
    /// tab's row for the same setting, which the label key alone would.
    element: &'static str,
    /// The setting this row binds.
    setting: &'static str,
    /// The scope a user edit writes to.
    scope: Scope,
    /// The control that draws it.
    control: PhotoControl,
}

/// A heading over a run of rows.
struct PhotoSection {
    /// The heading's Fluent key.
    heading: &'static str,
    /// The rows under it.
    rows: &'static [PhotoRow],
}

/// One tab of the window.
struct PhotoTab {
    /// The tab label's Fluent key.
    label: &'static str,
    /// The tab's slug, used in its panel's [`Name`].
    slug: &'static str,
    /// Content built above the sections that is not a settings row — the
    /// environment tab's live controls. The `&str` is the seed label the preset
    /// combos open showing.
    prologue: Option<fn(&mut Commands, Entity, &str)>,
    /// The tab's sections, in order.
    sections: &'static [PhotoSection],
}

/// The window's tabs, in order.
///
/// The set is the reference's, minus the tabs whose contents this viewer does
/// not have: its *Aids* tab is the Advanced-menu render toggles plus a
/// statistics strip, and its *Cam* tab is the camera / joystick window
/// (`viewer-camera-controls-window`), both of which are their own tasks. What is
/// here is every render knob the store actually declares.
static PHOTO_TABS: &[PhotoTab] = &[
    PhotoTab {
        label: "phototools-tab-environment",
        slug: "environment",
        prologue: Some(build_environment_prologue),
        sections: &[PhotoSection {
            heading: "preferences-section-reflections",
            rows: &[
                PhotoRow {
                    label: "preferences-row-probe-dynamic",
                    element: "phototools:probe-dynamic",
                    setting: crate::probes::PROBE_DYNAMIC_SETTING,
                    scope: Scope::Global,
                    control: PhotoControl::Check,
                },
                PhotoRow {
                    label: "preferences-row-mirrors",
                    element: "phototools:mirrors",
                    setting: crate::probes::RENDER_MIRRORS_SETTING,
                    scope: Scope::Global,
                    control: PhotoControl::Check,
                },
                PhotoRow {
                    label: "preferences-row-mirror-resolution",
                    element: "phototools:mirror-resolution",
                    setting: crate::probes::HERO_RESOLUTION_SETTING,
                    scope: Scope::Global,
                    control: PhotoControl::Combo(
                        crate::preferences_graphics::mirror_resolution_options,
                    ),
                },
                PhotoRow {
                    label: "preferences-row-mirror-update-rate",
                    element: "phototools:mirror-update-rate",
                    setting: crate::probes::HERO_UPDATE_RATE_SETTING,
                    scope: Scope::Global,
                    control: PhotoControl::Combo(
                        crate::preferences_graphics::mirror_update_rate_options,
                    ),
                },
            ],
        }],
    },
    PhotoTab {
        label: "phototools-tab-shadows",
        slug: "shadows",
        prologue: None,
        sections: &[PhotoSection {
            heading: "preferences-section-shadows",
            rows: &[
                PhotoRow {
                    label: "preferences-row-shadow-detail",
                    element: "phototools:shadow-detail",
                    setting: crate::preferences_graphics::SETTING_SHADOW_DETAIL,
                    scope: Scope::Global,
                    control: PhotoControl::Combo(
                        crate::preferences_graphics::shadow_detail_options,
                    ),
                },
                PhotoRow {
                    label: "preferences-row-shadow-map-size",
                    element: "phototools:shadow-map-size",
                    setting: crate::preferences_graphics::SETTING_SHADOW_MAP_SIZE,
                    scope: Scope::Global,
                    control: PhotoControl::Combo(
                        crate::preferences_graphics::shadow_map_size_options,
                    ),
                },
                PhotoRow {
                    label: "preferences-row-shadow-cascades",
                    element: "phototools:shadow-cascades",
                    setting: crate::preferences_graphics::SETTING_SHADOW_CASCADES,
                    scope: Scope::Global,
                    control: PhotoControl::Slider {
                        min: 1.0,
                        max: 4.0,
                        step: 1.0,
                        integer: true,
                    },
                },
            ],
        }],
    },
    PhotoTab {
        label: "phototools-tab-look",
        slug: "look",
        prologue: None,
        sections: &[
            PhotoSection {
                heading: "preferences-section-tonemap",
                rows: &[
                    PhotoRow {
                        label: "preferences-row-tonemap-type",
                        element: "phototools:tonemap-type",
                        setting: crate::tonemap::SETTING_TONEMAP_TYPE,
                        scope: Scope::Global,
                        control: PhotoControl::Combo(
                            crate::preferences_graphics::tonemap_type_options,
                        ),
                    },
                    PhotoRow {
                        label: "preferences-row-tonemap-mix",
                        element: "phototools:tonemap-mix",
                        setting: crate::tonemap::SETTING_TONEMAP_MIX,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 0.0,
                            max: 1.0,
                            step: crate::preferences_graphics::TONEMAP_MIX_STEP,
                            integer: false,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-exposure",
                        element: "phototools:exposure",
                        setting: crate::tonemap::SETTING_EXPOSURE,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: crate::preferences_graphics::EXPOSURE_MIN,
                            max: crate::preferences_graphics::EXPOSURE_MAX,
                            step: crate::preferences_graphics::EXPOSURE_STEP,
                            integer: false,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-dynamic-exposure",
                        element: "phototools:dynamic-exposure",
                        setting: crate::exposure::SETTING_ENABLED,
                        scope: Scope::Global,
                        control: PhotoControl::Check,
                    },
                    PhotoRow {
                        label: "preferences-row-auto-adjust-legacy",
                        element: "phototools:auto-adjust-legacy",
                        setting: crate::exposure::SETTING_AUTO_ADJUST_LEGACY,
                        scope: Scope::Global,
                        control: PhotoControl::Check,
                    },
                ],
            },
            PhotoSection {
                heading: "preferences-section-glow",
                rows: &[
                    PhotoRow {
                        label: "preferences-row-glow",
                        element: "phototools:glow",
                        setting: crate::glow::SETTING_ENABLED,
                        scope: Scope::Global,
                        control: PhotoControl::Check,
                    },
                    PhotoRow {
                        label: "preferences-row-glow-strength",
                        element: "phototools:glow-strength",
                        setting: crate::glow::SETTING_STRENGTH,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 0.0,
                            max: crate::preferences_graphics::GLOW_STRENGTH_MAX,
                            step: crate::preferences_graphics::GLOW_STRENGTH_STEP,
                            integer: false,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-glow-width",
                        element: "phototools:glow-width",
                        setting: crate::glow::SETTING_WIDTH,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 0.0,
                            max: crate::preferences_graphics::GLOW_WIDTH_MAX,
                            step: crate::preferences_graphics::GLOW_WIDTH_STEP,
                            integer: false,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-glow-iterations",
                        element: "phototools:glow-iterations",
                        setting: crate::glow::SETTING_ITERATIONS,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: crate::preferences_graphics::GLOW_ITERATIONS_MIN,
                            max: crate::preferences_graphics::GLOW_ITERATIONS_MAX,
                            step: 1.0,
                            integer: true,
                        },
                    },
                ],
            },
        ],
    },
    PhotoTab {
        label: "phototools-tab-general",
        slug: "general",
        prologue: None,
        sections: &[
            PhotoSection {
                heading: "preferences-section-render-quality",
                rows: &[
                    PhotoRow {
                        label: "preferences-row-render-quality",
                        element: "phototools:render-quality",
                        setting: crate::preferences_graphics::SETTING_RENDER_QUALITY,
                        scope: Scope::Global,
                        control: PhotoControl::QualityTier,
                    },
                    PhotoRow {
                        label: "preferences-row-draw-distance",
                        element: "phototools:draw-distance",
                        setting: crate::session::SETTING_DRAW_DISTANCE,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 32.0,
                            max: 1024.0,
                            step: 8.0,
                            integer: true,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-lod-factor",
                        element: "phototools:lod-factor",
                        setting: crate::render_priority::SETTING_LOD_FACTOR,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: crate::render_priority::LOD_FACTOR_MIN,
                            max: crate::render_priority::LOD_FACTOR_MAX,
                            step: crate::preferences_graphics::LOD_FACTOR_STEP,
                            integer: false,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-max-particles",
                        element: "phototools:max-particles",
                        setting: crate::particles::SETTING_MAX_PARTICLES,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 0.0,
                            max: 8192.0,
                            step: 256.0,
                            integer: true,
                        },
                    },
                ],
            },
            PhotoSection {
                heading: "preferences-section-avatar-complexity",
                rows: &[
                    PhotoRow {
                        label: "preferences-row-avatar-max-complexity",
                        element: "phototools:avatar-max-complexity",
                        setting: crate::avatar_complexity::SETTING_MAX_COMPLEXITY,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: 0.0,
                            max: crate::avatar_complexity::MAX_COMPLEXITY_SLIDER_MAX,
                            step: crate::avatar_complexity::MAX_COMPLEXITY_SLIDER_STEP,
                            integer: true,
                        },
                    },
                    PhotoRow {
                        label: "preferences-row-avatar-complexity-mode",
                        element: "phototools:avatar-complexity-mode",
                        setting: crate::avatar_complexity::SETTING_COMPLEXITY_MODE,
                        scope: Scope::Global,
                        control: PhotoControl::Combo(
                            crate::preferences_graphics::complexity_mode_options,
                        ),
                    },
                    // Per avatar, like the setting itself: whose friends are
                    // drawn is not a property of the machine.
                    PhotoRow {
                        label: "quick-prefs-friends-only",
                        element: "phototools:friends-only",
                        setting: crate::derender::SETTING_FRIENDS_ONLY,
                        scope: Scope::Account,
                        control: PhotoControl::Check,
                    },
                ],
            },
            PhotoSection {
                heading: "preferences-section-display",
                rows: &[
                    PhotoRow {
                        label: "preferences-row-vsync",
                        element: "phototools:vsync",
                        setting: crate::preferences_graphics::SETTING_VSYNC,
                        scope: Scope::Global,
                        control: PhotoControl::Check,
                    },
                    PhotoRow {
                        label: "preferences-row-limit-framerate",
                        element: "phototools:limit-framerate",
                        setting: crate::preferences_graphics::SETTING_LIMIT_FRAMERATE,
                        scope: Scope::Global,
                        control: PhotoControl::Check,
                    },
                    PhotoRow {
                        label: "preferences-row-fps-limit",
                        element: "phototools:fps-limit",
                        setting: crate::preferences_graphics::SETTING_FPS_LIMIT,
                        scope: Scope::Global,
                        control: PhotoControl::Slider {
                            min: crate::preferences_graphics::FPS_LIMIT_MIN,
                            max: crate::preferences_graphics::FPS_LIMIT_MAX,
                            step: crate::preferences_graphics::FPS_LIMIT_STEP,
                            integer: true,
                        },
                    },
                ],
            },
        ],
    },
];

// ---------------------------------------------------------------------------
// The environment tab's live controls.
// ---------------------------------------------------------------------------

/// The preset-group combo's options, in order: the three groups the World ▸
/// Environment menu offers. There is no *shared* row — un-pinning is the
/// **Shared Environment** button below, which is what the reference's own
/// Phototools offers and what keeps the combo a pure "which library".
const ENV_GROUP_KEYS: [&str; 3] = [
    "quick-prefs-env-daycycle",
    "quick-prefs-env-legacy",
    "quick-prefs-env-modern",
];

/// The element id of the preset-group combo.
const ENV_GROUP_ELEMENT: &str = "phototools:env-group";

/// The four times of day, in button order, with their labels.
const ENV_TIMES: [(FixedSky, &str); 4] = [
    (FixedSky::Sunrise, "quick-prefs-time-sunrise"),
    (FixedSky::Midday, "quick-prefs-time-midday"),
    (FixedSky::Sunset, "quick-prefs-time-sunset"),
    (FixedSky::Midnight, "quick-prefs-time-midnight"),
];

/// The [`FixedEnvironment`] a (group index, time) pair pins.
const fn fixed_for(group_index: usize, sky: FixedSky) -> FixedEnvironment {
    match group_index {
        1 => FixedEnvironment::Legacy(sky),
        2 => FixedEnvironment::Modern(sky),
        // Index 0 and any out-of-range value: the region's own day cycle,
        // frozen at that time.
        _ => FixedEnvironment::DayCycle(sky),
    }
}

/// The group-combo index a pinned environment sits in.
const fn group_index_of(fixed: FixedEnvironment) -> usize {
    match fixed {
        FixedEnvironment::DayCycle(_) => 0,
        FixedEnvironment::Legacy(_) => 1,
        FixedEnvironment::Modern(_) => 2,
    }
}

/// The time of day a pinned environment is frozen at. `FixedEnvironment::time`
/// says the same thing and is private to its own crate; it is one match either
/// way, and this one is the half of the pair [`group_index_of`] is the other of.
const fn time_of(fixed: FixedEnvironment) -> FixedSky {
    match fixed {
        FixedEnvironment::DayCycle(sky)
        | FixedEnvironment::Legacy(sky)
        | FixedEnvironment::Modern(sky) => sky,
    }
}

/// The preset-group combo's anchor.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoEnvGroupCombo;

/// A time-of-day button, tagged with the time it pins.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoTimeButton(FixedSky);

/// The **Shared Environment** button: drop the local environment entirely.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoSharedButton;

/// The **Personal Lighting…** button.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoPersonalLightingButton;

/// Marks every control the `@setenv` restriction takes away, so one system
/// greys and refuses them together.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoEnvGated;

/// The sun-and-moon scrubber's element prefix — the `scope` the shared
/// trackball / slider pairing systems in `sl_viewer_environment::rows` match on,
/// so this window's pair never drives Personal Lighting's.
const AIM_ELEMENT: &str = "phototools-aim";

/// One of the four angle sliders, tagged with the knob it writes.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoAimSliderRow(SkyKnob);

/// The sky the scrubber is editing.
///
/// A buffer rather than writing straight through, for the same reason Personal
/// Lighting keeps one: a slider writes **one** field of a `SkySettings`, so
/// something has to hold the other forty between drags.
#[derive(Resource, Debug, Default)]
struct PhotoAimEdit {
    /// The captured sky, once the window has been open with an environment.
    sky: Option<Box<SkySettings>>,
    /// The buffer has an edit the environment has not been given yet.
    dirty: bool,
    /// The widgets must be re-seeded from the buffer.
    reseed: bool,
    /// The next environment change is this window's **own** push, so the capture
    /// pass must not read it back as news.
    ///
    /// Without this the recapture would run on the frame after every drag frame
    /// and reseed the trackball from the sky it had just written — and that
    /// round trip is lossy exactly where it hurts: a body at a pole has no
    /// azimuth stored in its rotation, so the compass would be handed back a
    /// zero nobody typed. The same trap Personal Lighting's reseed-on-request
    /// design avoids by never reseeding during a drag.
    ours: bool,
}

/// Build the environment tab's live controls: which preset library, the four
/// times of day, the way back to the region's environment, the door to the full
/// local override, the three settings-asset tracks, and the sun-and-moon
/// scrubber.
///
/// `seed` is the label the three preset combos open showing, before the
/// inventory walk has filled them.
fn build_environment_prologue(commands: &mut Commands, panel: Entity, seed: &str) {
    spawn_section(commands, panel, "phototools-section-fixed-sky");

    let group_row = commands
        .spawn((
            row_node(),
            Name::new("phototools:row:env-group"),
            ChildOf(panel),
        ))
        .id();
    spawn_label(commands, group_row, "phototools-env-group");
    let labels: Vec<String> = ENV_GROUP_KEYS.iter().map(|key| (*key).to_owned()).collect();
    let group = spawn_combo(
        commands,
        group_row,
        &ComboSpec {
            element: ENV_GROUP_ELEMENT,
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: true,
        },
    );
    commands
        .entity(group)
        .insert((PhotoEnvGroupCombo, PhotoEnvGated));

    // The four times, wrapping rather than clipping: four translated words at a
    // large UI font do not fit one 400 px line, and the window is meant to be
    // narrow.
    let times = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            Name::new("phototools:row:env-times"),
            ChildOf(panel),
        ))
        .id();
    for (sky, label) in ENV_TIMES {
        let button = spawn_photo_button(commands, times, label, "env-time");
        commands
            .entity(button)
            .insert((PhotoTimeButton(sky), PhotoEnvGated))
            .observe(on_time_button);
    }

    let actions = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            Name::new("phototools:row:env-actions"),
            ChildOf(panel),
        ))
        .id();
    let shared = spawn_photo_button(commands, actions, "phototools-env-shared", "env-shared");
    commands
        .entity(shared)
        .insert((PhotoSharedButton, PhotoEnvGated))
        .observe(on_shared_button);
    let personal = spawn_photo_button(
        commands,
        actions,
        "phototools-personal-lighting",
        "personal-lighting",
    );
    commands
        .entity(personal)
        .insert((PhotoPersonalLightingButton, PhotoEnvGated))
        .observe(on_personal_lighting_button);

    // The three settings-asset tracks, from the same shared rows the Quick
    // Preferences panel hosts — a different `PresetHost`, the same lists.
    spawn_section(commands, panel, "phototools-section-presets");
    crate::quick_prefs_environment::spawn_preset_rows(
        commands,
        panel,
        &crate::quick_prefs_environment::PHOTOTOOLS_HOST,
        0,
        seed,
    );

    // The sun and the moon, each a trackball over its two angle sliders.
    spawn_section(commands, panel, "phototools-section-sun-moon");
    let bodies = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(12.0))
            },
            Name::new("phototools:row:aim"),
            ChildOf(panel),
        ))
        .id();
    let mut tab = 0;
    for knobs in AimKnobs::ALL {
        let column = commands
            .spawn((
                Node {
                    ..column(Val::Px(4.0))
                },
                ChildOf(bodies),
            ))
            .id();
        let trackball = spawn_trackball_row(commands, column, AIM_ELEMENT, *knobs, &mut tab);
        commands
            .entity(trackball)
            .insert(PhotoEnvGated)
            .observe(on_photo_trackball_aim);
        for knob in [knobs.azimuth, knobs.elevation] {
            let slider = spawn_slider(
                commands,
                column,
                AIM_ELEMENT,
                knob.slug(),
                knob.range(),
                knob.decimals(),
                &mut tab,
            );
            commands
                .entity(slider)
                .insert((PhotoAimSliderRow(knob), PhotoEnvGated))
                .observe(on_photo_aim_slider);
            tag_aim_slider(commands, slider, AIM_ELEMENT, knob);
        }
    }
}

/// Observer: pin the selected library's sky at this button's time of day.
fn on_time_button(
    activate: On<Activate>,
    buttons: Query<(&PhotoTimeButton, Has<InteractionDisabled>)>,
    groups: Query<&ComboSelection, With<PhotoEnvGroupCombo>>,
    environment: Option<ResMut<EnvironmentState>>,
    settings: Option<Res<ViewerSettings>>,
) {
    let Ok((button, disabled)) = buttons.get(activate.entity) else {
        return;
    };
    // Bevy's `InteractionDisabled` only tells the a11y tree; refusing the press
    // is this observer's job.
    if disabled {
        return;
    }
    let Some(mut environment) = environment else {
        return;
    };
    let group = groups.single().map_or(0, |selection| selection.active);
    let wanted = fixed_for(group, button.0);
    // Picking what is already pinned un-pins it, when the user asked for that
    // (`EnvironmentRepeatedTogglesShared`) — the same choke point the menu uses,
    // so the two surfaces cannot disagree about what a second press does.
    let fixed = if crate::environment::repeated_toggles_shared(settings.as_deref())
        && environment.fixed() == Some(wanted)
    {
        None
    } else {
        Some(wanted)
    };
    environment.set_fixed(fixed);
}

/// Observer: drop the local environment and go back to the region's.
fn on_shared_button(
    activate: On<Activate>,
    buttons: Query<Has<InteractionDisabled>, With<PhotoSharedButton>>,
    environment: Option<ResMut<EnvironmentState>>,
) {
    let Ok(disabled) = buttons.get(activate.entity) else {
        return;
    };
    if disabled {
        return;
    }
    if let Some(mut environment) = environment {
        environment.set_fixed(None);
    }
}

/// Observer: open (or close) the Personal Lighting window.
fn on_personal_lighting_button(
    activate: On<Activate>,
    buttons: Query<Has<InteractionDisabled>, With<PhotoPersonalLightingButton>>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let Ok(disabled) = buttons.get(activate.entity) else {
        return;
    };
    if disabled {
        return;
    }
    toggle_floater(&floaters, &mut panels, PERSONAL_LIGHTING_FLOATER_ID);
}

/// Keep the environment controls showing the environment in force, and refuse
/// them while `@setenv` holds the sky.
///
/// The group combo follows an external pin (the World ▸ Environment menu, a
/// script) because it is the same state; a **local settings asset** — a sky from
/// inventory, which this window cannot describe — leaves the combo where it is
/// and simply lights no time button, which is the honest answer.
fn sync_environment_controls(
    environment: Option<Res<EnvironmentState>>,
    rlv: Option<Res<RlvSession>>,
    mut groups: Query<&mut ComboSelection, With<PhotoEnvGroupCombo>>,
    mut times: Query<(&PhotoTimeButton, &mut BackgroundColor)>,
    gated: Query<(Entity, Has<InteractionDisabled>), With<PhotoEnvGated>>,
    mut commands: Commands,
) {
    let allowed = rlv
        .as_deref()
        .is_none_or(|session| can_change_environment(session.state()));
    for (entity, disabled) in &gated {
        if disabled == allowed {
            if allowed {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }

    let fixed = environment.as_deref().and_then(EnvironmentState::fixed);
    if let Some(fixed) = fixed
        && let Ok(mut group) = groups.single_mut()
    {
        let index = group_index_of(fixed);
        if group.active != index {
            group.active = index;
        }
    }
    let group_shown = groups.single().ok().map(|selection| selection.active);
    for (button, mut fill) in &mut times {
        let active = fixed.is_some_and(|fixed| time_of(fixed) == button.0)
            && fixed.map(group_index_of) == group_shown;
        let target = if !allowed {
            BUTTON_DISABLED_FILL
        } else if active {
            BUTTON_ACTIVE_FILL
        } else {
            BUTTON_FILL
        };
        if fill.0 != target {
            fill.0 = target;
        }
    }
}

/// Capture the sky the scrubber edits: on the window opening, and again
/// whenever the environment changed under it (a time button, the World ▸
/// Environment menu, a region change, a script).
///
/// Re-capturing is what keeps the four sliders honest — they show where the sun
/// *is*, not where it was when the window opened — and the [`PhotoAimEdit::ours`]
/// latch is what stops that honesty from eating the user's own drag.
fn capture_photo_aim(
    floaters: Query<(Entity, &Floater)>,
    shown: Query<Ref<UiPanelShown>>,
    environment: Option<Res<EnvironmentState>>,
    mut edit: ResMut<PhotoAimEdit>,
) {
    let Some(environment) = environment else {
        return;
    };
    let Some(panel) = floater_panel(&floaters, PHOTOTOOLS_FLOATER_ID) else {
        return;
    };
    let Ok(shown) = shown.get(panel) else {
        return;
    };
    if !shown.0 {
        return;
    }
    if edit.ours {
        edit.ours = false;
        return;
    }
    if !shown.is_changed() && !environment.is_changed() {
        return;
    }
    let Some(sky) = environment.sky_at(0.0, day_position(&environment)) else {
        // An environment with no sky frame at all: there is nothing to clone,
        // and inventing one would be a sky nobody asked for. The sliders stay
        // where they are rather than snapping to the bottom of their ranges.
        return;
    };
    edit.sky = Some(Box::new(sky));
    edit.reseed = true;
}

/// Observer: an angle slider moved — clamp it and write its knob.
fn on_photo_aim_slider(
    change: On<ValueChange<f32>>,
    sliders: Query<(&PhotoAimSliderRow, &SliderRange, Has<InteractionDisabled>)>,
    mut edit: ResMut<PhotoAimEdit>,
    mut commands: Commands,
) {
    let Ok((row_info, range, disabled)) = sliders.get(change.source) else {
        return;
    };
    if disabled {
        return;
    }
    let clamped = range.clamp(change.value);
    commands.entity(change.source).insert(SliderValue(clamped));
    if let Some(sky) = edit.sky.as_deref_mut() {
        row_info.0.write(sky, clamped);
        edit.dirty = true;
    }
}

/// Observer: a trackball was aimed — write the body's whole direction.
///
/// The two sliders under it are put back in step by the shared `rows` systems,
/// which read the widget's own aim rather than this buffer, so nothing here has
/// to think about what a rotation can and cannot store.
fn on_photo_trackball_aim(
    change: On<ValueChange<Vec2>>,
    trackballs: Query<(&AimTrackball, Has<InteractionDisabled>)>,
    mut edit: ResMut<PhotoAimEdit>,
) {
    let Ok((trackball, disabled)) = trackballs.get(change.source) else {
        return;
    };
    if disabled {
        return;
    }
    let aim = TrackballAim {
        azimuth: change.value.x,
        elevation: change.value.y,
    };
    if let Some(sky) = edit.sky.as_deref_mut() {
        trackball.knobs.write(sky, aim);
        edit.dirty = true;
    }
}

/// Seed the four sliders and the two trackballs from the captured sky.
fn reseed_photo_aim(
    mut commands: Commands,
    mut edit: ResMut<PhotoAimEdit>,
    sliders: Query<(Entity, &PhotoAimSliderRow, &SliderRange, &SliderValue)>,
    mut trackballs: Query<(&AimTrackball, &mut TrackballAim)>,
) {
    if !edit.reseed {
        return;
    }
    // The content is built on the window's first open by a deferred builder
    // whose commands land a frame later, so the capture that asked for this can
    // arrive before there is a widget to seed. Hold the request rather than
    // spend it on an empty query.
    if sliders.is_empty() {
        return;
    }
    edit.reseed = false;
    let Some(sky) = edit.sky.as_deref() else {
        return;
    };
    // `SliderValue` is immutable, so a new value is *inserted*, and only when it
    // differs — an insert marks the component changed whether or not it carries
    // a new number, and a spurious change would reach the pairing systems as a
    // drag that never happened.
    for (entity, row_info, range, value) in &sliders {
        let wanted = range.clamp(row_info.0.read(sky));
        if value.0.to_bits() != wanted.to_bits() {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
    // Seeded here rather than left to the pairing systems: those only reach a
    // trackball when a slider *changes*, and a sky whose sun happens to sit at
    // the sliders' current values would leave the marker where it was spawned.
    for (trackball, mut aim) in &mut trackballs {
        if trackball.scope != AIM_ELEMENT {
            continue;
        }
        let wanted = trackball.knobs.read(sky);
        if *aim != wanted {
            *aim = wanted;
        }
    }
}

/// Push the scrubbed sky into the local environment layer.
///
/// Only when something was actually dragged: merely *opening* this window must
/// not pin a local sky over the region's, which is the one behaviour that would
/// make a photographer's quick look change the shot.
fn push_photo_aim(mut edit: ResMut<PhotoAimEdit>, environment: Option<ResMut<EnvironmentState>>) {
    if !edit.dirty {
        return;
    }
    let Some(mut environment) = environment else {
        return;
    };
    let Some(sky) = edit.sky.clone() else {
        return;
    };
    edit.dirty = false;
    edit.ours = true;
    // No asset id: this sky is the user's own edit, and no inventory row names
    // it — a preset list must not claim one is in force.
    environment.set_local_instant(EnvironmentAsset::Sky(sky), None);
}

// ---------------------------------------------------------------------------
// The plugin.
// ---------------------------------------------------------------------------

/// A setting slider's trailing readout, tagged with what it shows.
#[derive(Component, Debug, Clone)]
struct PhotoValueLabel {
    /// The setting the readout shows.
    setting: String,
    /// Whether it shows a whole number.
    integer: bool,
}

/// Marks a setting slider's thumb, so it slides to the bound value.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoSliderThumb;

/// Marks a setting checkbox's box, so its fill tracks `Checked`.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoCheckboxBox;

/// Marks a button's label, so the `@setenv` greying reaches the text too.
#[derive(Component, Debug, Clone, Copy)]
struct PhotoButtonLabel(Entity);

/// Owns the Phototools window: the floater chrome and deferred content, the
/// environment controls, and the control visuals.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhototoolsPlugin;

impl Plugin for PhototoolsPlugin {
    fn build(&self, app: &mut App) {
        // The shared trackball / slider pairing systems, and the readout sync
        // every environment slider uses. Guarded because Personal Lighting and
        // the two settings editors add it too, and the first one wins.
        if !app.is_plugin_added::<RowsPlugin>() {
            app.add_plugins(RowsPlugin);
        }
        app.init_resource::<PhotoAimEdit>()
            .add_systems(
                Startup,
                spawn_phototools_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    sync_environment_controls,
                    update_photo_values,
                    drive_photo_thumbs,
                    drive_photo_checkboxes,
                    drive_photo_button_labels,
                ),
            )
            // Chained, and push first: the push is what sets the `ours` latch
            // the capture consumes in the same frame, so a drag never reads its
            // own write back through the lossy rotation round trip.
            .add_systems(
                Update,
                (push_photo_aim, capture_photo_aim, reseed_photo_aim).chain(),
            );
    }
}

/// The window's [`FloaterSpec`] — shared with the `FLOATERS` registry, so the
/// swept window is the one the viewer spawns.
#[must_use]
pub fn phototools_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PHOTOTOOLS_FLOATER_ID,
        title: "Phototools".to_owned(),
        position: Vec2::new(80.0, 80.0),
        // A definite rect, the scroll-list carve-out from the content-driven
        // convention: each tab's panel scrolls, and a panel can only scroll
        // inside a box with a height of its own. Narrow and tall on purpose —
        // this window lives down one side of the screen while the shot is
        // composed in the rest of it.
        default_size: Some(Vec2::new(400.0, 620.0)),
        min_size: Some(Vec2::new(300.0, 320.0)),
        dock_host: None,
        caps: FloaterCaps {
            resizable: true,
            minimizable: true,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the chrome hidden, with the content deferred to first open.
fn spawn_phototools_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, phototools_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("phototools-title"));
    let builder = commands.register_system(build_phototools_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content build: the tab container, one panel per [`PHOTO_TABS`]
/// entry, each filled from the table.
fn build_phototools_content(
    In(handle): In<FloaterHandle>,
    mut commands: Commands,
    settings: Option<Res<ViewerSettings>>,
    translator: crate::i18n::Translator,
) {
    let seed = translator.get(crate::quick_prefs_environment::KEY_REGION_DEFAULT);
    let content = commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                ..column(Val::Px(ROW_GAP))
            },
            Name::new("phototools:content"),
            ChildOf(handle.content),
        ))
        .id();

    let labels: Vec<String> = PHOTO_TABS.iter().map(|tab| tab.label.to_owned()).collect();
    let tabs = spawn_tab_container(
        &mut commands,
        content,
        &TabSpec {
            element: "phototools-tabs",
            placement: TabPlacement::BlockStart,
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );
    fill_tab_container(&mut commands, TabPlacement::BlockStart, &tabs);
    for (tab, panel) in PHOTO_TABS.iter().zip(tabs.panels.iter().copied()) {
        commands
            .entity(panel)
            .insert(Name::new(format!("phototools:tab:{}", tab.slug)));
        if let Some(prologue) = tab.prologue {
            prologue(&mut commands, panel, &seed);
        }
        for section in tab.sections {
            spawn_section(&mut commands, panel, section.heading);
            for row_def in section.rows {
                if bindable(settings.as_deref(), row_def) {
                    spawn_row(&mut commands, panel, row_def);
                }
            }
        }
    }
}

/// Whether a row's setting is declared with a type its control can drive.
///
/// A row that names a setting the store has never heard of, or draws a checkbox
/// over a number, is skipped with a warning rather than bound to nothing — the
/// quick-preferences guard, kept because the same class of mistake is possible
/// here (a setting renamed in its own crate, a row moved between control kinds).
/// A unit test makes it a build-time failure for the shipped table; this is the
/// runtime backstop. With no store at all — the gallery app — every row is
/// admitted so the specimen has something to draw.
fn bindable(settings: Option<&ViewerSettings>, row_def: &PhotoRow) -> bool {
    let Some(settings) = settings else {
        return true;
    };
    let Some(declaration) = settings.store().declaration(row_def.setting) else {
        warn!(
            "phototools: setting {} is not declared; skipping its row",
            row_def.setting
        );
        return false;
    };
    let kind = declaration.kind();
    let ok = match row_def.control {
        PhotoControl::Check => kind == SettingKind::Bool,
        PhotoControl::Slider { .. } => {
            matches!(kind, SettingKind::F32 | SettingKind::I32 | SettingKind::U32)
        }
        PhotoControl::Combo(_) | PhotoControl::QualityTier => {
            matches!(kind, SettingKind::I32 | SettingKind::U32)
        }
    };
    if !ok {
        warn!(
            "phototools: setting {} is {kind:?}, which its control cannot drive; skipping its row",
            row_def.setting
        );
    }
    ok
}

/// The binding a row writes through, at its declared scope.
fn row_binding(row_def: &PhotoRow) -> SettingBinding {
    match row_def.scope {
        Scope::Account => SettingBinding::account(row_def.setting),
        Scope::Global => SettingBinding::global(row_def.setting),
    }
}

/// Spawn one row of the table.
fn spawn_row(commands: &mut Commands, parent: Entity, row_def: &PhotoRow) {
    match row_def.control {
        PhotoControl::Check => spawn_check_row(commands, parent, row_def),
        PhotoControl::Slider {
            min,
            max,
            step,
            integer,
        } => spawn_slider_row(commands, parent, row_def, min, max, step, integer),
        PhotoControl::Combo(options) => {
            spawn_combo_row(commands, parent, row_def, &options(), false);
        }
        PhotoControl::QualityTier => spawn_combo_row(
            commands,
            parent,
            row_def,
            &crate::preferences_graphics::quality_options(),
            true,
        ),
    }
}

/// A label + control row node.
fn row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        width: Val::Percent(100.0),
        ..row(Val::Px(ROW_GAP))
    }
}

/// Spawn a section heading.
fn spawn_section(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(SECTION_FONT),
        TextColor(SECTION_COLOR),
        Name::new(format!("phototools:section:{key}")),
        ChildOf(parent),
    ));
}

/// Spawn a row's translated label.
fn spawn_label(commands: &mut Commands, parent: Entity, key: &'static str) {
    commands.spawn((
        Text::default(),
        Translated::new(key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// Spawn a checkbox row.
fn spawn_check_row(commands: &mut Commands, parent: Entity, row_def: &PhotoRow) {
    let row_entity = spawn_row_shell(commands, parent, row_def);
    commands.spawn((
        bound_checkbox(row_binding(row_def)),
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(CHECK_OFF),
        TabIndex(0),
        PhotoCheckboxBox,
        Name::new(format!("{}:checkbox", row_def.element)),
        ChildOf(row_entity),
    ));
}

/// Spawn a slider row with its trailing readout.
fn spawn_slider_row(
    commands: &mut Commands,
    parent: Entity,
    row_def: &PhotoRow,
    min: f32,
    max: f32,
    step: f32,
    integer: bool,
) {
    let row_entity = spawn_row_shell(commands, parent, row_def);
    // A trailing group keeps the slider and its readout together, so the label
    // sits at the leading edge and the control at the trailing one.
    let group = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..row(Val::Px(6.0))
            },
            ChildOf(row_entity),
        ))
        .id();
    commands
        .spawn((
            bound_slider(
                row_binding(row_def),
                SliderRange::new(min, max),
                SliderStep(step),
            ),
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            TabIndex(0),
            Name::new(format!("{}:slider", row_def.element)),
            ChildOf(group),
        ))
        .with_children(|track| {
            track.spawn((
                SliderThumb,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(THUMB_WIDTH),
                    height: Val::Px(TRACK_HEIGHT),
                    ..default()
                },
                LogicalInset(LogicalRect {
                    inline_start: Val::Px(0.0),
                    ..LogicalRect::ZERO
                }),
                BackgroundColor(THUMB_FILL),
                PhotoSliderThumb,
            ));
        });
    // A right-aligning slot with a *minimum* width, so the readout column lines
    // up but a long value or a large UI font grows it rather than clipping. The
    // `Text` itself stays content-sized: a width on the leaf makes bevy_text
    // wrap instead of growing the box.
    let slot = commands
        .spawn((
            Node {
                min_width: Val::Px(VALUE_WIDTH),
                justify_content: JustifyContent::FlexEnd,
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(group),
        ))
        .id();
    commands.spawn((
        Text::default(),
        UiFont::Sans.at(FONT),
        TextColor(VALUE_COLOR),
        PhotoValueLabel {
            setting: row_def.setting.to_owned(),
            integer,
        },
        Pickable::IGNORE,
        ChildOf(slot),
    ));
}

/// Spawn a combo row over a shared option list.
fn spawn_combo_row(
    commands: &mut Commands,
    parent: Entity,
    row_def: &PhotoRow,
    options: &[(&'static str, SettingValue)],
    quality_tier: bool,
) {
    let row_entity = spawn_row_shell(commands, parent, row_def);
    let labels: Vec<String> = options.iter().map(|(key, _)| (*key).to_owned()).collect();
    let anchor = spawn_combo(
        commands,
        row_entity,
        &ComboSpec {
            element: row_def.element,
            labels: &labels,
            active: 0,
            tab_index: 0,
            font_size: FONT,
            translate_labels: true,
        },
    );
    commands.entity(anchor).insert((
        row_binding(row_def),
        ComboBindingValues(options.iter().map(|(_, value)| value.clone()).collect()),
    ));
    if quality_tier {
        // The same marker the graphics tab puts on its row's anchor, so one
        // applier serves both surfaces — a tier picked here writes exactly the
        // settings a tier picked there does.
        commands.entity(anchor).insert(QualityTierControl);
    }
}

/// The row node and its label, shared by all three control kinds.
fn spawn_row_shell(commands: &mut Commands, parent: Entity, row_def: &PhotoRow) -> Entity {
    let row_entity = commands
        .spawn((
            row_node(),
            Name::new(format!("phototools:row:{}", row_def.setting)),
            ChildOf(parent),
        ))
        .id();
    spawn_label(commands, row_entity, row_def.label);
    row_entity
}

/// Spawn a bordered button with a translated label, returning the button.
fn spawn_photo_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    slug: &str,
) -> Entity {
    let button = commands
        .spawn((
            Button,
            TabIndex(0),
            Node {
                padding: UiRect::axes(Val::Px(9.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_FILL),
            Name::new(format!("phototools:button:{slug}:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(FONT),
        TextColor(LABEL_COLOR),
        PhotoButtonLabel(button),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    button
}

// ---------------------------------------------------------------------------
// Control visuals.
// ---------------------------------------------------------------------------

/// Keep each slider's readout current from the store.
fn update_photo_values(
    settings: Option<Res<ViewerSettings>>,
    mut labels: Query<(&mut Text, &PhotoValueLabel)>,
) {
    let Some(settings) = settings else {
        return;
    };
    for (mut text, label) in &mut labels {
        let Some(value) = settings
            .store()
            .get(&label.setting)
            .and_then(setting_as_f32)
        else {
            continue;
        };
        let wanted = if label.integer {
            format!("{}", value.round())
        } else {
            format!("{value:.2}")
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// A setting's value as the `f32` a readout shows, or `None` for a non-numeric
/// setting.
const fn setting_as_f32(value: &SettingValue) -> Option<f32> {
    match *value {
        SettingValue::F32(v) => Some(v),
        SettingValue::I32(v) => Some(i32_to_f32(v)),
        SettingValue::U32(v) => Some(u32_to_f32(v)),
        _ => None,
    }
}

/// Widen an `i32` setting to the `f32` a readout shows.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a Phototools integer setting's magnitude is small; the readout only shows whole \
              numbers"
)]
const fn i32_to_f32(value: i32) -> f32 {
    value as f32
}

/// Widen a `u32` setting to the `f32` a readout shows.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a Phototools integer setting's magnitude is small; the readout only shows whole \
              numbers"
)]
const fn u32_to_f32(value: u32) -> f32 {
    value as f32
}

/// Slide each slider's thumb to its value within the range.
fn drive_photo_thumbs(
    sliders: Query<(&SliderValue, &SliderRange, &Children), With<Slider>>,
    mut thumbs: Query<&mut LogicalInset, With<PhotoSliderThumb>>,
) {
    for (value, range, children) in &sliders {
        let span = range.span();
        let fraction = if span > f32::EPSILON {
            ((value.0 - range.start()) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let offset = fraction * (TRACK_WIDTH - THUMB_WIDTH);
        for child in children {
            if let Ok(mut inset) = thumbs.get_mut(*child) {
                inset.0.inline_start = Val::Px(offset);
            }
        }
    }
}

/// Colour each checkbox's box from its `Checked` state.
fn drive_photo_checkboxes(
    mut boxes: Query<(&mut BackgroundColor, Has<Checked>), With<PhotoCheckboxBox>>,
) {
    for (mut fill, checked) in &mut boxes {
        let target = if checked { CHECK_ON } else { CHECK_OFF };
        if fill.0 != target {
            fill.0 = target;
        }
    }
}

/// Grey a refused button's label. The fill is the time buttons' own business
/// (they also carry the "this is the environment in force" highlight); the text
/// is every button's, which is why it is a system over the labels rather than
/// another arm of [`sync_environment_controls`].
fn drive_photo_button_labels(
    disabled: Query<Has<InteractionDisabled>>,
    mut labels: Query<(&mut TextColor, &PhotoButtonLabel)>,
) {
    for (mut color, label) in &mut labels {
        let refused = disabled.get(label.0).unwrap_or(false);
        let target = if refused {
            DIM_LABEL_COLOR
        } else {
            LABEL_COLOR
        };
        if color.0 != target {
            color.0 = target;
        }
    }
}

// ---------------------------------------------------------------------------
// The gallery specimen.
// ---------------------------------------------------------------------------

/// The static Phototools specimen for the gallery / headless harness: the tab
/// strip over the environment tab's controls and two setting rows — the layout,
/// with none of the live behaviour (per the element registry's rule: no plugin,
/// no store, no observers).
pub fn spawn_phototools_specimen(commands: &mut Commands, parent: Entity, cx: ElementCx) -> Entity {
    let card = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(10.0)),
                min_width: Val::Px(300.0),
                ..column(Val::Px(ROW_GAP))
            },
            Name::new("phototools-specimen"),
            ChildOf(parent),
        ))
        .id();
    let strip = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(4.0))
            },
            ChildOf(card),
        ))
        .id();
    for (index, label) in ["Environment", "Shadows", "Look", "General"]
        .into_iter()
        .enumerate()
    {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(CONTROL_BORDER),
                BackgroundColor(if index == 0 {
                    BUTTON_ACTIVE_FILL
                } else {
                    BUTTON_FILL
                }),
                ChildOf(strip),
            ))
            .with_child((
                Text::new(cx.text(label)),
                cx.font(UiFont::Sans),
                TextColor(LABEL_COLOR),
            ));
    }
    commands.spawn((
        Text::new(cx.text("Fixed sky")),
        cx.font(UiFont::Sans),
        TextColor(SECTION_COLOR),
        ChildOf(card),
    ));
    spawn_specimen_combo_row(commands, card, &cx, "Preset library", "Legacy WindLight");
    let times = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                ..row(Val::Px(6.0))
            },
            ChildOf(card),
        ))
        .id();
    for label in [
        "Sunrise",
        "Midday",
        "Sunset",
        "Midnight",
        "Personal Lighting…",
    ] {
        commands
            .spawn((
                Node {
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(BUTTON_BORDER),
                BackgroundColor(BUTTON_FILL),
                ChildOf(times),
            ))
            .with_child((
                Text::new(cx.text(label)),
                cx.font(UiFont::Sans),
                TextColor(LABEL_COLOR),
            ));
    }
    commands.spawn((
        Text::new(cx.text("Reflections")),
        cx.font(UiFont::Sans),
        TextColor(SECTION_COLOR),
        ChildOf(card),
    ));
    spawn_specimen_check_row(commands, card, &cx, "Avatars in reflections", true);
    spawn_specimen_slider_row(commands, card, &cx, "Exposure", "1.00", 0.25);
    card
}

/// A content-sized specimen row (unlike the live row node, which fills the
/// window): the card grows to the widest row, so no fixed-width child overflows
/// its box across scripts and scales.
fn specimen_row() -> Node {
    Node {
        align_items: AlignItems::Center,
        ..row(Val::Px(ROW_GAP))
    }
}

/// A static combo-looking specimen row.
fn spawn_specimen_combo_row(
    commands: &mut Commands,
    parent: Entity,
    cx: &ElementCx,
    label: &str,
    value: &str,
) {
    let row_entity = commands.spawn((specimen_row(), ChildOf(parent))).id();
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    commands
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            ChildOf(row_entity),
        ))
        .with_child((
            Text::new(cx.text(value)),
            cx.font(UiFont::Sans),
            TextColor(VALUE_COLOR),
        ));
}

/// A static checkbox-looking specimen row.
fn spawn_specimen_check_row(
    commands: &mut Commands,
    parent: Entity,
    cx: &ElementCx,
    label: &str,
    checked: bool,
) {
    let row_entity = commands.spawn((specimen_row(), ChildOf(parent))).id();
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    commands.spawn((
        Node {
            width: Val::Px(CHECK_SIZE),
            height: Val::Px(CHECK_SIZE),
            border: UiRect::all(Val::Px(2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(CONTROL_BORDER),
        BackgroundColor(if checked { CHECK_ON } else { CHECK_OFF }),
        ChildOf(row_entity),
    ));
}

/// A static slider-looking specimen row.
fn spawn_specimen_slider_row(
    commands: &mut Commands,
    parent: Entity,
    cx: &ElementCx,
    label: &str,
    value: &str,
    fraction: f32,
) {
    let row_entity = commands.spawn((specimen_row(), ChildOf(parent))).id();
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    commands
        .spawn((
            Node {
                width: Val::Px(TRACK_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                border: UiRect::all(Val::Px(2.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(CONTROL_BORDER),
            BackgroundColor(TRACK_FILL),
            ChildOf(row_entity),
        ))
        .with_child((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(THUMB_WIDTH),
                height: Val::Px(TRACK_HEIGHT),
                ..default()
            },
            LogicalInset(LogicalRect {
                inline_start: Val::Px(fraction.clamp(0.0, 1.0) * (TRACK_WIDTH - THUMB_WIDTH)),
                ..LogicalRect::ZERO
            }),
            BackgroundColor(THUMB_FILL),
        ));
    commands.spawn((
        Text::new(cx.text(value)),
        cx.font(UiFont::Sans),
        TextColor(VALUE_COLOR),
        ChildOf(row_entity),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce as _;
    use pretty_assertions::assert_eq;

    /// The error type the tests bubble through `?` (`unwrap` / `expect` are
    /// lint-denied in this workspace).
    type TestError = Box<dyn core::error::Error>;

    /// Every row of every tab, in order — what the table-wide assertions below
    /// walk. The build itself walks the tabs and sections, because it has a
    /// panel and a heading to place per group; only the tests want the flat view.
    fn all_rows() -> impl Iterator<Item = &'static PhotoRow> {
        PHOTO_TABS
            .iter()
            .flat_map(|tab| tab.sections.iter())
            .flat_map(|section| section.rows.iter())
    }

    /// A settings store with every registrar's declarations, as the viewer has.
    fn declared() -> ViewerSettings {
        ViewerSettings::declared_for_test(REGISTRARS)
    }

    /// The registrars whose settings this window's rows name. Not the viewer's
    /// whole list — only what the table reaches — so a new row naming a setting
    /// from a crate nobody registered here fails loudly rather than being
    /// skipped.
    const REGISTRARS: &[fn(&mut ViewerSettings)] = &[
        crate::preferences_graphics::register_settings,
        crate::session::register_settings,
        crate::render_priority::register_settings,
        crate::particles::register_settings,
        crate::glow::register_settings,
        crate::tonemap::register_settings,
        crate::exposure::register_settings,
        // The four that the viewer declares from a `Startup` system rather than
        // the registrar list, because their plugins may run without a store at
        // all. Each exposes its declarations as a plain function so a reader
        // like this one can ask the same store the viewer will have.
        crate::probes::declare_probe_settings,
        crate::probes::declare_mirror_settings,
        crate::avatar_complexity::declare_complexity_settings,
        crate::derender::declare_derender_settings,
    ];

    /// Every row binds a setting the viewer actually declares — a row over a
    /// name nobody registered would render a control that silently does nothing.
    #[test]
    fn every_row_names_a_declared_setting() -> Result<(), TestError> {
        let settings = declared();
        let missing: Vec<&str> = all_rows()
            .filter(|row_def| settings.store().declaration(row_def.setting).is_none())
            .map(|row_def| row_def.setting)
            .collect();
        assert_eq!(missing, Vec::<&str>::new());
        Ok(())
    }

    /// Every row's control matches its setting's type: a checkbox over a `Bool`,
    /// a slider over a number, a combo over an integer.
    #[test]
    fn every_row_draws_a_control_its_setting_can_drive() -> Result<(), TestError> {
        let settings = declared();
        let wrong: Vec<&str> = all_rows()
            .filter(|row_def| !bindable(Some(&settings), row_def))
            .map(|row_def| row_def.setting)
            .collect();
        assert_eq!(wrong, Vec::<&str>::new());
        Ok(())
    }

    /// The runtime guard behind those two tests: a mistyped row is skipped, and
    /// the storeless gallery app still admits every row so the specimen draws.
    #[test]
    fn a_row_over_a_mistyped_setting_is_refused() -> Result<(), TestError> {
        let settings = declared();
        // Draw distance is an `F32`: a checkbox over it is the hand-edit
        // mistake the runtime guard exists for.
        let wrong = PhotoRow {
            label: "preferences-row-draw-distance",
            element: "phototools:test",
            setting: crate::session::SETTING_DRAW_DISTANCE,
            scope: Scope::Global,
            control: PhotoControl::Check,
        };
        assert!(!bindable(Some(&settings), &wrong));
        // And with no store at all (the gallery app) every row is admitted, or
        // the specimen would have nothing to draw.
        assert!(bindable(None, &wrong));
        Ok(())
    }

    /// No two rows report the same element id — the harness addresses controls
    /// by it, and a duplicate makes one of them unreachable.
    #[test]
    fn element_ids_are_unique() -> Result<(), TestError> {
        let mut seen: Vec<&str> = all_rows().map(|row_def| row_def.element).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total);
        Ok(())
    }

    /// No setting appears on two rows: two bound controls over one key would
    /// fight each other through the store.
    #[test]
    fn no_setting_is_on_two_rows() -> Result<(), TestError> {
        let mut seen: Vec<&str> = all_rows().map(|row_def| row_def.setting).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total);
        Ok(())
    }

    /// Every element id is namespaced to this window, so none collides with the
    /// Preferences tab's row for the same setting.
    #[test]
    fn every_element_id_is_namespaced() -> Result<(), TestError> {
        let stray: Vec<&str> = all_rows()
            .map(|row_def| row_def.element)
            .filter(|element| !element.starts_with("phototools:"))
            .collect();
        assert_eq!(stray, Vec::<&str>::new());
        Ok(())
    }

    /// A combo with one row is a label; every combo here offers a real choice.
    #[test]
    fn a_combo_row_offers_at_least_two_options() -> Result<(), TestError> {
        for row_def in all_rows() {
            let count = match row_def.control {
                PhotoControl::Combo(options) => options().len(),
                PhotoControl::QualityTier => crate::preferences_graphics::quality_options().len(),
                PhotoControl::Check | PhotoControl::Slider { .. } => continue,
            };
            assert!(count >= 2, "{} offers {count} options", row_def.setting);
        }
        Ok(())
    }

    /// A slider's range is non-empty and its step fits inside it — a zero step
    /// or an inverted range is a thumb that cannot be moved.
    #[test]
    fn a_slider_row_has_a_usable_range() -> Result<(), TestError> {
        for row_def in all_rows() {
            let PhotoControl::Slider { min, max, step, .. } = row_def.control else {
                continue;
            };
            assert!(max > min, "{} has an empty range", row_def.setting);
            assert!(step > 0.0, "{} has a zero step", row_def.setting);
            assert!(
                step <= max - min,
                "{} steps past its own range",
                row_def.setting
            );
        }
        Ok(())
    }

    /// No tab is empty: every one has either a prologue or at least one section.
    #[test]
    fn every_tab_has_a_slug_and_content() -> Result<(), TestError> {
        for tab in PHOTO_TABS {
            assert!(!tab.slug.is_empty());
            assert!(
                tab.prologue.is_some() || !tab.sections.is_empty(),
                "tab {} is empty",
                tab.slug
            );
        }
        Ok(())
    }

    /// The preset-library combo's index and the pinned environment agree in both
    /// directions, for all three libraries, without disturbing the time of day.
    #[test]
    fn the_group_combo_round_trips_every_library() -> Result<(), TestError> {
        for index in 0..ENV_GROUP_KEYS.len() {
            let fixed = fixed_for(index, FixedSky::Sunset);
            assert_eq!(group_index_of(fixed), index);
            assert_eq!(time_of(fixed), FixedSky::Sunset);
        }
        Ok(())
    }

    /// An index past the combo's own list pins the region's day cycle rather
    /// than nothing — a fallback, never an accidental un-pin.
    #[test]
    fn an_out_of_range_group_falls_back_to_the_region_day_cycle() -> Result<(), TestError> {
        assert_eq!(
            fixed_for(99, FixedSky::Midday),
            FixedEnvironment::DayCycle(FixedSky::Midday)
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // The sun-and-moon scrubber.
    // -----------------------------------------------------------------------

    /// An app with the scrubber's systems, an environment, and a shown panel
    /// carrying the window's floater id — the shape `capture_photo_aim` looks
    /// for.
    fn aim_app() -> Result<App, TestError> {
        let mut app = App::new();
        app.init_resource::<PhotoAimEdit>()
            .insert_resource(EnvironmentState::default());
        let root = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                let handle = spawn_floater(&mut commands, root, phototools_floater_spec());
                commands.entity(handle.root).insert(UiPanelShown(true));
            })
            .map_err(|error| format!("the floater spawn should run: {error:?}"))?;
        Ok(app)
    }

    /// **Opening the window does not pin a local sky.**
    ///
    /// A photographer opening this to check a slider must not find the region's
    /// environment replaced by a copy of itself — the capture fills the buffer,
    /// and only a drag makes it dirty.
    #[test]
    fn opening_captures_without_installing() -> Result<(), TestError> {
        let mut app = aim_app()?;
        app.add_systems(Update, capture_photo_aim);
        app.update();
        let edit = app.world().resource::<PhotoAimEdit>();
        assert!(!edit.dirty, "a capture is not an edit");
        assert!(edit.reseed, "the widgets are asked to follow it");
        assert!(
            app.world()
                .resource::<EnvironmentState>()
                .local()
                .is_empty(),
            "nothing was installed in the local layer"
        );
        Ok(())
    }

    /// **A drag reaches the local layer, once.**
    ///
    /// And the push sets the latch that stops the next capture from reading its
    /// own write back — the round trip through a rotation loses the azimuth of a
    /// body at a pole, so a recapture mid-drag would move a compass the user is
    /// holding still.
    #[test]
    fn a_dragged_angle_is_pushed_once() -> Result<(), TestError> {
        let mut app = aim_app()?;
        app.add_systems(Update, (push_photo_aim, capture_photo_aim).chain());
        app.update();
        {
            let mut edit = app.world_mut().resource_mut::<PhotoAimEdit>();
            let sky = edit
                .sky
                .as_deref_mut()
                .ok_or("the capture should have run")?;
            SkyKnob::SunAzimuth.write(sky, 123.0);
            edit.dirty = true;
        }
        app.update();
        let edit = app.world().resource::<PhotoAimEdit>();
        assert!(!edit.dirty, "the push consumed the edit");
        assert!(
            !edit.ours,
            "and the capture in the same frame consumed the latch"
        );
        assert!(
            app.world()
                .resource::<EnvironmentState>()
                .local()
                .sky()
                .is_some(),
            "the scrubbed sky is in the local layer"
        );
        Ok(())
    }

    /// **The scrubber's knobs are exactly the two bodies' four angles.**
    ///
    /// It is a scrubber, not a second sky editor: anything beyond the four is
    /// Personal Lighting's, and the button next to it is how you get there.
    #[test]
    fn the_scrubber_carries_only_the_aim_knobs() -> Result<(), TestError> {
        let mut knobs: Vec<SkyKnob> = AimKnobs::ALL
            .iter()
            .flat_map(|pair| [pair.azimuth, pair.elevation])
            .collect();
        assert_eq!(knobs.len(), 4);
        knobs.sort_unstable_by_key(|knob| knob.slug());
        knobs.dedup_by_key(|knob| knob.slug());
        assert_eq!(knobs.len(), 4, "the four angles are four distinct knobs");
        Ok(())
    }

    /// The four time buttons are four distinct skies, so none is unreachable.
    #[test]
    fn the_time_buttons_cover_every_fixed_sky() -> Result<(), TestError> {
        let mut times: Vec<FixedSky> = ENV_TIMES.iter().map(|(sky, _)| *sky).collect();
        times.sort_unstable_by_key(|sky| format!("{sky:?}"));
        times.dedup();
        assert_eq!(times.len(), ENV_TIMES.len());
        Ok(())
    }
}
