//! The **Spawn crowd** debug button — a bottom-toolbar affordance shown only
//! while a synthetic crowd is armed (`SL_VIEWER_CROWD=N`), so the user captures
//! the crowd template *manually* once the local avatar is fully rezzed.
//!
//! # Why a manual trigger
//!
//! The synthetic crowd ([`crate::gpu_avatars::crowd`]) copies the local avatar's
//! *currently visible* skinned submeshes verbatim, so it must be captured only
//! after every worn mesh + bake has loaded. A timing heuristic cannot tell when
//! that is: asynchronous BOM bakes (client-side on OpenSim, server-side on the SL
//! grids) flip body/head/clothing parts visible over many seconds with no
//! reliable "done" event, and the old auto-settle repeatedly fired mid-load and
//! froze a half-dressed crowd. The user is the only reliable oracle, so this
//! button hands them the trigger: it shows the **live visible-part count**, which
//! climbs as parts rez in, and the user clicks once it has plateaued and the
//! avatar looks complete.
//!
//! Zero cost on a normal run: with `SL_VIEWER_CROWD` unset the button is never
//! spawned. It is removed once the crowd is captured (its job is done).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::bottom_toolbar::BottomArea;
use crate::gpu_avatars::crowd::GpuCrowd;
use crate::ui_font::UiFont;

/// The button label font size, in logical pixels — matched to the toolbar's.
const FONT_SIZE: f32 = 13.0;

/// The button border colour — a warm amber that reads as a debug affordance,
/// distinct from the toolbar's blue live buttons.
const BORDER: Color = Color::srgb(0.70, 0.52, 0.18);

/// The button background — the amber fill of a live debug call to act.
const BACKGROUND: Color = Color::srgb(0.55, 0.40, 0.12);

/// Marks the Spawn crowd button (the focusable [`Button`] node).
#[derive(Component)]
struct CrowdSpawnButton;

/// Marks the Spawn crowd button's text child, so its label can be refreshed with
/// the live visible-part count without touching the button node.
#[derive(Component)]
struct CrowdSpawnButtonLabel;

/// The Spawn crowd debug-button plugin: spawns the button into the toolbar while
/// a crowd is armed, keeps its label's live part count current, and removes it
/// once the crowd is captured.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CrowdDebugButtonPlugin;

impl Plugin for CrowdDebugButtonPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_crowd_button, update_crowd_button).chain());
    }
}

/// Spawn the Spawn crowd button into the bottom toolbar, once, when a crowd is
/// armed and the toolbar exists. A no-op (and permanently retired) when no crowd
/// was requested, so a normal run never gets the button.
///
/// Runs in `Update` guarded by a `Local` done-flag (like the state buttons) so it
/// never races the toolbar's own startup spawn: it waits for [`BottomArea`] to be
/// published, spawns into its button bar, and never runs its body again.
fn spawn_crowd_button(
    mut done: Local<bool>,
    crowd: Res<GpuCrowd>,
    area: Option<Res<BottomArea>>,
    mut commands: Commands,
) {
    if *done {
        return;
    }
    // No crowd requested: retire this system for the whole run.
    if !crowd.enabled() {
        *done = true;
        return;
    }
    // Wait for the toolbar to publish its bar.
    let Some(area) = area else {
        return;
    };
    commands
        .spawn((
            Button,
            TabIndex(0),
            CrowdSpawnButton,
            Node {
                padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                // Size to the label and never be compressed below it, so the
                // whole label stays on one line.
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(BORDER),
            BackgroundColor(BACKGROUND),
            Name::new("crowd-debug:spawn"),
            ChildOf(area.bar),
        ))
        .with_child((
            Text::default(),
            CrowdSpawnButtonLabel,
            UiFont::Sans.at(FONT_SIZE),
            TextColor(Color::WHITE),
            // Keep the label on one line — a flex text measure otherwise
            // under-allocates and wraps the multi-word label.
            TextLayout::no_wrap(),
        ))
        .observe(on_crowd_button);
    *done = true;
}

/// Observer: the user clicked Spawn crowd — arm the capture. The crowd system
/// takes the template on the next frame the avatar has visible submeshes.
///
/// Emits a distinctive `info!` so the click is a clear **before/after boundary**
/// on the Tracy timeline (the `trace_tracy` layer forwards tracing events as
/// Tracy messages): everything before it is the empty scene, everything after is
/// the crowd — the two regions the profiling run compares.
fn on_crowd_button(_activate: On<Activate>, mut crowd: ResMut<GpuCrowd>) {
    if !crowd.awaiting_trigger() {
        return;
    }
    crowd.request_spawn();
    info!(
        "===== SL_VIEWER_CROWD MARKER: spawn triggered by user — capturing the \
         {}-part template and spawning {} copies (everything after this line is the \
         crowd; everything before is the empty scene) =====",
        crowd.visible_parts(),
        crowd.target(),
    );
}

/// Keep the button current: refresh its label with the live visible-part count
/// while the crowd is armed (so the user can watch it plateau), and remove the
/// button once the crowd has been captured (its job is done). A no-op when the
/// button does not exist (no crowd requested, or already removed).
fn update_crowd_button(
    crowd: Res<GpuCrowd>,
    button: Query<Entity, With<CrowdSpawnButton>>,
    mut labels: Query<&mut Text, With<CrowdSpawnButtonLabel>>,
    mut last_parts: Local<Option<usize>>,
    mut commands: Commands,
) {
    let Ok(button) = button.single() else {
        return;
    };
    if !crowd.awaiting_trigger() {
        // Captured (or the crowd was never armed): retire the button.
        commands.entity(button).despawn();
        return;
    }
    let parts = crowd.visible_parts();
    // Rewrite the label only when the count changes, so a static crowd does not
    // churn the text layout every frame.
    if *last_parts == Some(parts) {
        return;
    }
    *last_parts = Some(parts);
    if let Ok(mut text) = labels.single_mut() {
        *text = Text::new(format!(
            "Spawn crowd \u{d7}{} \u{2014} {parts} parts",
            crowd.target()
        ));
    }
}
