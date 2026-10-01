//! A test harness for the **preview**: every control of an environment window
//! is driven the way a person drives it, and the environment the scene draws is
//! read back after each one.
//!
//! Every window in this crate previews what it edits through
//! [`EnvironmentState`] — the editors through its edit layer, Personal Lighting
//! through its local layer — so a control whose change never reaches that
//! layer is a knob that edits a value nobody sees until the save. The windows
//! wire their controls one by one, and the knob tables' own tests check only
//! that a value survives `write` then `read`: nothing else would notice a
//! slider whose change is written into the session but never marked for the
//! preview, or a swatch whose pick lands in a buffer the preview does not read.
//!
//! The controls are found by the names [`crate::rows`] gives them
//! (`{element}-{slug}:slider`, `:color-swatch`, `:texture-swatch`,
//! `:trackball`), so the sweep covers whatever a window draws without a list of
//! its own to fall out of step. A slider and a trackball are driven by the
//! keyboard through the real focus and input stack; a swatch by the reply its
//! picker sends, which is that control's seam.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::ui_widgets::{SliderRange, SliderValue};
use sl_client_bevy::{SkySettings, TextureKey, WaterSettings};
use sl_viewer_intents::TexturePicked;
use sl_viewer_pickers::ui_texture_picker::TextureSwatchValue;
use sl_viewer_testkit::interact::{self, InteractionTest};
use sl_viewer_testkit::{find_by_name, settle};
use sl_viewer_ui_core::i18n::install_untranslated;
use sl_viewer_ui_core::ui::UiPanelShown;
use sl_viewer_ui_widgets::floater::FloaterPlugin;
use sl_viewer_ui_widgets::ui_color_picker::{ColorPicked, ColorSwatchValue};
use sl_viewer_ui_widgets::ui_trackball::{TrackballAim, TrackballBody};
use sl_viewer_world_scene::environment::EnvironmentState;
use sl_viewer_world_scene::sky::day_position;

use crate::EnvironmentUiPlugins;
use crate::knobs::{AimKnobs, ColorKnob, SkyKnob, TextureKnob, WaterKnob};

/// How close a drawn value must be to the control's to count as the same.
const TOLERANCE: f32 = 1e-3;

/// An interaction app with every window of this crate, and an environment to
/// preview into: the built-in default, as a viewer has before a region
/// answers.
pub(crate) fn app() -> App {
    let mut app = InteractionTest::new().build();
    app.init_resource::<Time>()
        .add_message::<ColorPicked>()
        .add_message::<TexturePicked>()
        .add_message::<sl_client_bevy::SlCommand>()
        .add_message::<sl_client_bevy::SlEvent>()
        .add_message::<sl_viewer_notifications::NotificationResponse>()
        .init_resource::<EnvironmentState>()
        .add_plugins((FloaterPlugin, EnvironmentUiPlugins));
    install_untranslated(&mut app);
    settle(&mut app);
    app
}

/// The sky and water the scene draws now.
pub(crate) fn drawn(app: &App) -> (SkySettings, WaterSettings) {
    let state = app.world().resource::<EnvironmentState>();
    let position = day_position(state);
    let fallback = EnvironmentState::default();
    let sky = state
        .rendered_sky()
        .or_else(|| fallback.rendered_sky())
        .unwrap_or_else(|| SkySettings::legacy_windlight_default("none"));
    let water = state
        .water_at(position)
        .unwrap_or_else(|| WaterSettings::legacy_default("none"));
    (sky, water)
}

/// Show or hide the floater whose id is `floater`, and let it lay out.
pub(crate) fn show(app: &mut App, floater: &str, shown: bool) -> Result<(), String> {
    let panel = find_by_name(app, &format!("floater:{floater}"))
        .ok_or_else(|| format!("no floater {floater}"))?;
    app.world_mut()
        .entity_mut(panel)
        .insert(UiPanelShown(shown));
    settle(app);
    Ok(())
}

/// What one control drives.
#[derive(Debug, Clone, Copy)]
enum Control {
    /// A sky slider.
    Sky(SkyKnob),
    /// A water slider.
    Water(WaterKnob),
    /// A colour swatch.
    Color(ColorKnob),
    /// A texture swatch.
    Texture(TextureKnob),
    /// A sun or moon trackball.
    Aim(AimKnobs),
}

/// Every control of `element` the window drew, with the knob it drives.
fn controls(app: &mut App, element: &str) -> Vec<(String, Entity, Control)> {
    let mut found = Vec::new();
    let mut look = |name: String, control: Control| {
        if let Some(entity) = find_by_name(app, &name) {
            found.push((name, entity, control));
        }
    };
    for knob in SkyKnob::ALL {
        look(
            format!("{element}-{}:slider", knob.slug()),
            Control::Sky(*knob),
        );
    }
    for knob in WaterKnob::ALL {
        look(
            format!("{element}-{}:slider", knob.slug()),
            Control::Water(*knob),
        );
    }
    for knob in ColorKnob::ALL {
        look(
            format!("{element}-{}:color-swatch", knob.slug()),
            Control::Color(*knob),
        );
    }
    for knob in TextureKnob::ALL {
        look(
            format!("{element}-{}:texture-swatch", knob.slug()),
            Control::Texture(*knob),
        );
    }
    for knob in AimKnobs::ALL {
        look(
            // Named by the body (`spawn_trackball`), not the pair's label slug.
            format!(
                "{element}-{}:trackball",
                match knob.body {
                    TrackballBody::Sun => "sun",
                    TrackballBody::Moon => "moon",
                }
            ),
            Control::Aim(*knob),
        );
    }
    found
}

/// What the scene draws for the knob `control` drives, as a comparable list.
fn drawn_value(app: &App, control: Control) -> Vec<f32> {
    let (sky, water) = drawn(app);
    match control {
        Control::Sky(knob) => vec![knob.read(&sky)],
        Control::Water(knob) => vec![knob.read(&water)],
        Control::Color(knob) => {
            let color = knob.read(&sky, &water).to_srgba();
            vec![color.red, color.green, color.blue]
        }
        Control::Texture(knob) => {
            let key = knob.read(&sky, &water).uuid().as_u64_pair();
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "only compared for equality with another key read the same way"
            )]
            let halves = vec![key.0 as f32, key.1 as f32];
            halves
        }
        Control::Aim(knob) => {
            let aim = knob.read(&sky);
            vec![aim.azimuth, aim.elevation]
        }
    }
}

/// Whether two drawn values are the same, within `tolerance`.
fn same(left: &[f32], right: &[f32], tolerance: f32) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| (left - right).abs() <= tolerance.max(right.abs() * 1e-4))
}

/// How close a trackball's angles must come back, in degrees: a direction
/// stored as a rotation in `f32` and read back as two angles loses a few
/// hundredths of a degree, most near a pole, where the azimuth is barely
/// defined.
const AIM_TOLERANCE_DEGREES: f32 = 0.05;

/// How close two values of `control` must be to count as one: a thousandth of
/// a slider's range (some density terms span a ten-thousandth),
/// [`AIM_TOLERANCE_DEGREES`] for a trackball, [`TOLERANCE`] for a swatch.
fn tolerance(app: &App, entity: Entity, control: Control) -> f32 {
    if matches!(control, Control::Aim(_)) {
        return AIM_TOLERANCE_DEGREES;
    }
    app.world()
        .get::<SliderRange>(entity)
        .map_or(TOLERANCE, |range| (range.span() * 1e-3).min(TOLERANCE))
}

/// What the widget of `control` shows, in the terms [`drawn_value`] uses.
fn shown_value(app: &App, entity: Entity, control: Control) -> Option<Vec<f32>> {
    let world = app.world();
    Some(match control {
        Control::Sky(_) | Control::Water(_) => vec![world.get::<SliderValue>(entity)?.0],
        Control::Color(_) => {
            let color = world.get::<ColorSwatchValue>(entity)?.0.to_srgba();
            vec![color.red, color.green, color.blue]
        }
        Control::Texture(_) => {
            let key = world
                .get::<TextureSwatchValue>(entity)?
                .0
                .uuid()
                .as_u64_pair();
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "only compared for equality with another key read the same way"
            )]
            let halves = vec![key.0 as f32, key.1 as f32];
            halves
        }
        Control::Aim(_) => {
            let aim = world.get::<TrackballAim>(entity)?;
            vec![aim.azimuth, aim.elevation]
        }
    })
}

/// Every control of `element` whose widget does not show what the scene
/// draws — after a Revert, the widgets have to be put back with the preview.
pub(crate) fn widgets_out_of_step(app: &mut App, element: &str) -> Vec<String> {
    let found = controls(app, element);
    let mut failures = Vec::new();
    for (name, entity, control) in found {
        let mut drawn = drawn_value(app, control);
        // A slider shows a value outside its range at the nearer end, as the
        // reference's does: the legacy default frame has density terms past
        // the ranges the density panel's sliders cover.
        if let Some(range) = app.world().get::<SliderRange>(entity) {
            for value in &mut drawn {
                *value = range.clamp(*value);
            }
        }
        match shown_value(app, entity, control) {
            Some(shown) if same(&shown, &drawn, tolerance(app, entity, control)) => {}
            Some(shown) => failures.push(format!(
                "{name}: shows {shown:?}, the scene draws {drawn:?}"
            )),
            None => failures.push(format!("{name}: shows no value")),
        }
    }
    failures
}

/// Drive one control to a value it does not hold, and say what it should now
/// draw. `None` when it could not be driven.
fn drive(app: &mut App, entity: Entity, control: Control) -> Option<Vec<f32>> {
    match control {
        Control::Sky(_) | Control::Water(_) => {
            let range = *app.world().get::<SliderRange>(entity)?;
            let value = app.world().get::<SliderValue>(entity)?.0;
            // Towards whichever end is farther, so the value always moves.
            let (key_code, key) = if value < range.start() + range.span() / 2.0 {
                (KeyCode::End, Key::End)
            } else {
                (KeyCode::Home, Key::Home)
            };
            interact::focus(app, entity);
            interact::tap(app, key_code, key);
            settle(app);
            let moved = app.world().get::<SliderValue>(entity)?.0;
            Some(vec![moved])
        }
        Control::Color(_) => {
            let color = Color::srgb(0.25, 0.5, 0.75);
            app.world_mut().write_message(ColorPicked {
                requester: entity,
                color,
                final_pick: false,
            });
            settle(app);
            let picked = color.to_srgba();
            Some(vec![picked.red, picked.green, picked.blue])
        }
        Control::Texture(_) => {
            let texture = TextureKey::from(sl_client_bevy::Uuid::from_u128(0x7E57_7E57_7E57));
            app.world_mut().write_message(TexturePicked {
                requester: entity,
                texture,
                final_pick: false,
            });
            settle(app);
            let key = texture.uuid().as_u64_pair();
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "only compared for equality with another key read the same way"
            )]
            let halves = vec![key.0 as f32, key.1 as f32];
            Some(halves)
        }
        Control::Aim(_) => {
            interact::focus(app, entity);
            interact::tap(app, KeyCode::ArrowLeft, Key::ArrowLeft);
            settle(app);
            let aim = *app.world().get::<TrackballAim>(entity)?;
            Some(vec![aim.azimuth, aim.elevation])
        }
    }
}

/// Drive every control `element` draws, each to a value it did not hold, and
/// check the scene draws that value afterwards. Returns how many controls were
/// driven, and one line per control the preview did not follow.
pub(crate) fn sweep(app: &mut App, element: &str) -> (usize, Vec<String>) {
    let found = controls(app, element);
    let mut failures = Vec::new();
    // A control the knob tables do not name would be skipped silently: every
    // slider, swatch and trackball of the window has to be one the sweep
    // drives.
    let prefix = format!("{element}-");
    let mut unswept: Vec<String> = app
        .world_mut()
        .query::<&Name>()
        .iter(app.world())
        .map(|name| name.as_str().to_owned())
        .filter(|name| {
            name.starts_with(&prefix)
                && [":slider", ":color-swatch", ":texture-swatch", ":trackball"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
                && !found.iter().any(|(swept, _entity, _control)| swept == name)
        })
        .collect();
    unswept.sort();
    for name in unswept {
        failures.push(format!("{name}: a control the sweep does not drive"));
    }
    for (name, entity, control) in &found {
        let tolerance = tolerance(app, *entity, *control);
        let before = drawn_value(app, *control);
        let Some(wanted) = drive(app, *entity, *control) else {
            failures.push(format!("{name}: could not be driven"));
            continue;
        };
        let after = drawn_value(app, *control);
        if same(&after, &before, tolerance) {
            failures.push(format!(
                "{name}: the preview did not change (drew {before:?}, the control holds \
                 {wanted:?})"
            ));
        } else if !same(&after, &wanted, tolerance) {
            failures.push(format!(
                "{name}: the preview drew {after:?}, the control holds {wanted:?}"
            ));
        }
    }
    (found.len(), failures)
}
