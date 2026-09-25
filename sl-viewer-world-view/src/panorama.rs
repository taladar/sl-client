//! The **360° snapshot** (`viewer-360-snapshot`): an equirectangular panorama
//! of everything around the camera, in the format the platforms that render
//! immersive photos read.
//!
//! This is deliberately **not** an option on the snapshot floater
//! (`snapshot_floater`). That window photographs the *window* — one rectangle,
//! at the window's own resolution, with the interface toggles that only mean
//! anything for a window capture. A panorama is a different renderer: six
//! square views from one eye point at a fixed 90° lens, reprojected onto a
//! sphere. The reference viewer separates them for the same reason
//! (`llfloater360capture` is its own floater), and so does this.
//!
//! # The capture borrows the viewer's own camera
//!
//! The six faces are shot by pointing the **viewer camera** ([`ViewerCamera`])
//! at each of them in turn, into an off-screen square image, rather than by
//! standing up a second camera for the job. That is the same conclusion the
//! snapshot floater reached from the other side: the world camera carries the
//! probe-generated image-based lighting, the exposure, glow, underwater-fog and
//! tone-map selectors, and the render-layer membership that half the scene's
//! systems key off. A second camera has to reproduce every one of those or it
//! renders a darker, flatter world — and the list is not stable enough to copy.
//!
//! Borrowing it means three things have to be handed back afterwards, and one
//! writer has to be asked to stand aside while they are out on loan:
//!
//! - **The pose.** Each face is a rotation about the eye point, so the camera
//!   driver (`crate::camera`'s `position_camera`) must not write the pose while
//!   a capture owns it. It is gated on [`CameraBorrowed`] not existing.
//! - **The lens.** A cube face is 90° vertical on a square target, and the
//!   preferences tab rewrites the projection from the stored `CameraAngle`
//!   every frame; `sl_viewer_preferences`' `apply_camera_fov` yields on the
//!   same resource.
//! - **The target.** The camera is pointed at a square image of the chosen face
//!   size and put back on whatever it was on before — which may be the
//!   resolution divisor's reduced image rather than the window, so the old
//!   target is *saved*, not assumed. (The divisor leaves a camera on a foreign
//!   target alone, so it does not fight the borrow.) Swapping a camera's target
//!   also has to mark its projection changed, or `camera_system` never
//!   refreshes the target size — see `sl_viewer_world_scene::resolution_divisor`,
//!   which documents what that costs.
//!
//! While the faces are being shot the **window shows a stale frame** with the
//! interface still drawn over it: the overlay cameras are still on the window,
//! but the world camera is not. That is the panorama's version of the snapshot
//! floater's blink, and it lasts the dozen-or-so frames the six faces take.
//!
//! # Freezing the world, and waiting for it to arrive
//!
//! Six faces are six frames apart at best, and anything that moves between them
//! lands in the panorama twice, or half-way across a seam. So a capture:
//!
//! - **waits for quiet** first ([`crate::quiescence::SceneQuiescence`]) so the
//!   meshes, textures and bakes the scene asked for have arrived — the same
//!   wait the screenshot harness uses, with a timeout so a busy region still
//!   gets its photo; and
//! - **pauses virtual time** for the duration, which stops the water, the
//!   clouds, the stars, the particles and every played animation, because they
//!   are all driven from the clock `Time<Virtual>` feeds (`globals.time` in the
//!   shaders). That is the reference viewer's `freezeWorld`, reached through
//!   the one switch this engine already has for it.
//!
//! # What is in the frame, and what is not
//!
//! The interface and the worn HUD are drawn by the *overlay* cameras, which
//! keep pointing at the window, so a panorama never contains them and needs no
//! toggle for either. What it does contain today is the world-space text —
//! name tags and `llSetText` hover text are drawn by the world camera in the
//! transparent phase, so they appear in all six faces. Moving them to the
//! overlay pass is its own roadmap item
//! (`viewer-world-text-in-the-overlay-pass`), and doing so takes them out of a
//! panorama for free.
//!
//! # Where the maths and the metadata live
//!
//! [`equirect`] turns the six faces into the 2:1 image (and documents the
//! layout, the seams and the sampling); [`xmp`] writes the GPano packet that
//! makes a reader treat it as a sphere. Both are pure and tested on synthetic
//! cubes; this module is the part that drives the camera, the files and the
//! window.
//!
//! Deliberately out of scope: **stereo / VR 360**, which needs two eye points
//! and a different output layout.
//!
//! Reference (Firestorm, read-only): `llfloater360capture.cpp`, and this
//! workspace's own `sl_viewer_world_scene::probes` for the in-tree precedent
//! of cube-map capture.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy::ui_widgets::Activate;
use image::{ImageEncoder as _, RgbImage};

use sl_client_bevy::{SlCurrentRegion, SlRegionIdentity};
use sl_settings::SettingValue;
use sl_viewer_intents::LocalChatNotice;
use sl_viewer_settings::ViewerSettings;
use sl_viewer_ui_core::i18n::{TransArgs, Translated, Translator};
use sl_viewer_ui_core::ui::{UiRoot, UiScaffoldSystems, column, row};
use sl_viewer_ui_core::ui_font::UiFont;
use sl_viewer_ui_core::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use sl_viewer_ui_widgets::floater::{
    DeferredFloaterContent, FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater,
};
use sl_viewer_ui_widgets::ui_combo::{ComboChanged, ComboSelection, ComboSpec, spawn_combo};
use sl_viewer_world_api::ViewerCamera;

use crate::quiescence::SceneQuiescence;
use equirect::{CUBE_FACES, CubeFaces};
use sl_viewer_ui_core::skin::text_role;
use sl_viewer_ui_core::skin_palette::SkinPalette;
use xmp::PanoramaMetadata;

pub mod equirect;
pub mod xmp;

/// The floater's stable id (see `Floater::id`).
pub const PANORAMA_FLOATER_ID: &str = "panorama";

/// The body font size, in logical pixels.
const FONT_SIZE: f32 = 13.0;

/// A control label — the primary role.
const LABEL_COLOR: Color = SkinPalette::FALLBACK.text_primary;

/// The hint and status lines — the muted role.
const HINT_COLOR: Color = SkinPalette::FALLBACK.text_muted;

/// A button's background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The preview frame's border.
const PREVIEW_BORDER: Color = Color::srgb(0.30, 0.34, 0.42);

/// The preview frame's fill behind the image.
const PREVIEW_BACKGROUND: Color = Color::srgb(0.02, 0.02, 0.03);

/// The preview image's width in logical pixels; a panorama is 2:1, so its
/// height is half this and the frame never changes shape.
const PREVIEW_WIDTH: f32 = 480.0;

/// The preview image's height in logical pixels.
const PREVIEW_HEIGHT: f32 = 240.0;

/// How many frames the camera sits on a face before its shutter fires, so the
/// frame that is read back is one rendered with the new rotation and target.
///
/// The reference viewer renders each face more than once for the same reason
/// (its `360CaptureNumRenderPasses`), because a view that has just turned 90°
/// is a view whose interest list has not caught up.
const SETTLE_FRAMES: u8 = 2;

/// How many frames a face's read-back may take before the capture gives up and
/// hands the camera back. A lost screenshot callback must not strand the camera
/// off the window.
const EXPOSURE_TIMEOUT_FRAMES: u8 = 240;

/// How long a capture waits for the scene to go quiet before shooting anyway,
/// in seconds. A busy region never reaches zero outstanding fetches, and a
/// photographer who pressed the button still wants a photo.
const QUIET_TIMEOUT_SECONDS: f32 = 20.0;

/// The cube-face sizes the quality combo offers, in pixels — the reference
/// viewer's own preview / medium / high / maximum ladder.
const FACE_SIZES: &[u32] = &[128, 512, 1024, 2048];

/// The default quality: 1024, the reference's "high".
const DEFAULT_FACE_SIZE: usize = 2;

/// The panorama widths the output combo offers. The height is always half.
const OUTPUT_WIDTHS: &[u32] = &[2048, 4096, 8192];

/// The default output width: 4096, the reference's `360CaptureOutputImageWidth`.
const DEFAULT_OUTPUT_WIDTH: usize = 1;

/// One selectable output format: the extension that picks the encoder, its
/// label, and how the XMP packet gets into it.
#[derive(Debug, Clone, Copy)]
struct FormatPreset {
    /// The file extension.
    extension: &'static str,
    /// The combo label.
    label: &'static str,
}

/// The formats a panorama may be written as.
///
/// Shorter than the snapshot floater's list on purpose: BMP and TGA have no
/// metadata container at all, so a panorama written as one is a picture nothing
/// will open as a sphere. JPEG first, because that is what the photo-sphere
/// platforms accept.
const FORMATS: &[FormatPreset] = &[
    FormatPreset {
        extension: "jpg",
        label: "JPEG",
    },
    FormatPreset {
        extension: "png",
        label: "PNG",
    },
];

/// The default format index (JPEG).
const DEFAULT_FORMAT: usize = 0;

/// The JPEG quality a panorama is encoded at — the reference viewer's
/// `360CaptureJPEGEncodeQuality`.
const JPEG_QUALITY: u8 = 95;

/// The settings section the panorama preferences are grouped under.
const SETTINGS_SECTION: &[&str] = &["snapshot"];

/// The setting name for the last-used cube-face size index.
const SETTING_FACE_SIZE: &str = "panorama_face_size";

/// The setting name for the last-used output width index.
const SETTING_OUTPUT_WIDTH: &str = "panorama_output_width";

/// The setting name for the last-used output format index.
const SETTING_FORMAT: &str = "panorama_format";

// ---------------------------------------------------------------------------
// Plugin.
// ---------------------------------------------------------------------------

/// The plugin wiring the 360° snapshot floater and its capture into a host.
#[derive(Debug, Clone, Copy, Default)]
pub struct PanoramaPlugin;

impl Plugin for PanoramaPlugin {
    /// Spawn the floater chrome at startup, then drive the preferences, the
    /// capture state machine and the stitch / save tasks.
    ///
    /// `drive_capture` runs **in** the `CameraPositioned` set rather than
    /// after it: everything that follows the viewpoint (the sky dome, the
    /// ocean, the particle billboards, the underwater-fog matrix) orders itself
    /// against that set, so a face's rotation has to be written inside it or
    /// those systems spend the capture following the pose of the previous face.
    fn build(&self, app: &mut App) {
        app.init_resource::<PanoramaState>()
            .init_resource::<CapturedFace>()
            .add_message::<RequestPanoramaCapture>()
            .add_systems(
                Startup,
                spawn_panorama_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            .add_systems(
                Update,
                (
                    load_persisted_preferences,
                    apply_combos,
                    update_status_text,
                    start_capture,
                    poll_stitch,
                    poll_saves,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                drive_capture.in_set(sl_viewer_world_api::WorldPhase::CameraPositioned),
            );
    }
}

/// Register the panorama settings defaults so the store round-trips them.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_FACE_SIZE,
        SettingValue::I32(i32::try_from(DEFAULT_FACE_SIZE).unwrap_or(0)),
        "last-used 360 capture cube-face size (index into the floater's list)",
    );
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_OUTPUT_WIDTH,
        SettingValue::I32(i32::try_from(DEFAULT_OUTPUT_WIDTH).unwrap_or(0)),
        "last-used 360 panorama output width (index into the floater's list)",
    );
    settings.register_in(
        SETTINGS_SECTION,
        SETTING_FORMAT,
        SettingValue::I32(i32::try_from(DEFAULT_FORMAT).unwrap_or(0)),
        "last-used 360 panorama output format (index into the floater's list)",
    );
}

// ---------------------------------------------------------------------------
// State.
// ---------------------------------------------------------------------------

/// Present exactly while a 360 capture owns the viewer camera's pose, lens and
/// render target.
///
/// The two writers that would otherwise fight it read this by name and stand
/// aside: the camera driver here ([`crate::camera`]) and the field-of-view
/// preference in `sl_viewer_preferences`. A resource rather than a component so
/// a system that does not query the camera at all can still see the borrow with
/// one `Option<Res<_>>`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraBorrowed;

/// Where a capture is in its cycle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum CapturePhase {
    /// Nothing in flight.
    #[default]
    Idle,
    /// Waiting for the scene to go quiet (or for the wait to time out).
    Settling {
        /// Seconds of real time left before the capture proceeds regardless.
        remaining: f32,
    },
    /// Pointed at a face, counting down to its shutter.
    Framing {
        /// Which face, as an index into [`CUBE_FACES`].
        face: usize,
        /// Frames still to render before the read-back is asked for.
        settle: u8,
    },
    /// The read-back for a face has been asked for; waiting for it to land.
    Exposing {
        /// Which face.
        face: usize,
        /// Frames left before the capture gives up on this face.
        patience: u8,
    },
    /// Every face is in; the reprojection is running off-thread.
    Stitching,
}

/// The status line's current state.
#[derive(Debug, Clone, Default)]
enum StatusKind {
    /// The idle "Ready" line.
    #[default]
    Ready,
    /// Waiting for the scene to settle.
    Settling,
    /// Shooting face `n` of six.
    Shooting {
        /// The one-based face number.
        face: usize,
    },
    /// Reprojecting.
    Stitching,
    /// An already-formatted message (a saved path, or an error).
    Message(String),
}

/// The floater's preferences, the capture in flight, and the last panorama.
#[derive(Resource, Debug)]
struct PanoramaState {
    /// Selected cube-face size, as an index into [`FACE_SIZES`].
    face_size: usize,
    /// Selected output width, as an index into [`OUTPUT_WIDTHS`].
    output_width: usize,
    /// Selected format, as an index into [`FORMATS`].
    format: usize,
    /// Whether the persisted preferences have reached the controls yet.
    loaded: bool,
    /// Where the capture is.
    phase: CapturePhase,
    /// The faces shot so far, in [`CUBE_FACES`] order.
    faces: Vec<RgbImage>,
    /// The finished panorama, kept so Save writes the shot already on screen
    /// rather than taking a fresh one.
    panorama: Option<RgbImage>,
    /// The metadata that finished panorama was captured with.
    metadata: Option<PanoramaMetadata>,
    /// A per-session counter, so two saves in one second do not collide.
    counter: u32,
    /// The status line.
    status: StatusKind,
}

impl Default for PanoramaState {
    /// Start on the built-in defaults, nothing in flight — `derive` would give
    /// index zero for each picker, which is the 128-pixel preview cube rather
    /// than the quality a first capture should have.
    fn default() -> Self {
        Self {
            face_size: DEFAULT_FACE_SIZE,
            output_width: DEFAULT_OUTPUT_WIDTH,
            format: DEFAULT_FORMAT,
            loaded: false,
            phase: CapturePhase::Idle,
            faces: Vec::new(),
            panorama: None,
            metadata: None,
            counter: 0,
            status: StatusKind::Ready,
        }
    }
}

impl PanoramaState {
    /// The selected cube-face edge length in pixels.
    fn face_pixels(&self) -> u32 {
        FACE_SIZES
            .get(self.face_size)
            .or_else(|| FACE_SIZES.get(DEFAULT_FACE_SIZE))
            .copied()
            .unwrap_or(1024)
    }

    /// The selected panorama width in pixels.
    fn output_pixels(&self) -> u32 {
        OUTPUT_WIDTHS
            .get(self.output_width)
            .or_else(|| OUTPUT_WIDTHS.get(DEFAULT_OUTPUT_WIDTH))
            .copied()
            .unwrap_or(4096)
    }

    /// The selected format's file extension.
    fn extension(&self) -> &'static str {
        FORMATS
            .get(self.format)
            .or_else(|| FORMATS.get(DEFAULT_FORMAT))
            .map_or("jpg", |preset| preset.extension)
    }
}

/// What the camera was doing before the capture took it, and the image the
/// faces are rendered into.
#[derive(Resource, Debug)]
struct BorrowedCamera {
    /// Its render target before the borrow — the window, or the resolution
    /// divisor's reduced image.
    target: RenderTarget,
    /// Its projection before the borrow.
    projection: Projection,
    /// Its rotation before the borrow; the translation is never touched.
    rotation: Quat,
    /// The square image the faces are rendered into.
    face_image: Handle<Image>,
    /// Whether virtual time was already paused when the borrow started, so an
    /// unpause never resumes a clock somebody else stopped.
    time_was_paused: bool,
}

/// The most recently read-back face, handed from the screenshot observer to
/// [`drive_capture`].
#[derive(Resource, Debug, Default)]
struct CapturedFace(Option<Image>);

/// A press on Capture or Save.
#[derive(Message, Debug, Clone, Copy)]
enum RequestPanoramaCapture {
    /// Shoot a new panorama.
    Shoot,
    /// Write the panorama already captured to disk.
    Save,
}

/// The floater's live entity handles.
#[derive(Resource, Debug, Clone, Copy)]
struct PanoramaUi {
    /// The preview [`ImageNode`].
    preview: Entity,
    /// The "capture one" hint shown until the first panorama.
    preview_hint: Entity,
    /// The cube-face-size combo anchor.
    quality_combo: Entity,
    /// The output-width combo anchor.
    width_combo: Entity,
    /// The format combo anchor.
    format_combo: Entity,
    /// The status text node.
    status: Entity,
}

// ---------------------------------------------------------------------------
// Spawn.
// ---------------------------------------------------------------------------

/// The floater's [`FloaterSpec`] — shared with the `FLOATERS` registry, so the
/// swept window is the one the viewer spawns.
#[must_use]
pub fn panorama_floater_spec() -> FloaterSpec {
    FloaterSpec {
        id: PANORAMA_FLOATER_ID,
        title: "360° Snapshot".to_owned(),
        position: Vec2::new(360.0, 110.0),
        default_size: None,
        min_size: None,
        dock_host: None,
        caps: FloaterCaps {
            resizable: false,
            minimizable: false,
            closable: true,
            dockable: false,
        },
    }
}

/// Startup: spawn the (hidden) floater chrome, its content deferred to first
/// open.
fn spawn_panorama_floater(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(&mut commands, root.0, panorama_floater_spec());
    commands
        .entity(handle.title_text)
        .insert(Translated::new("panorama-title"));
    let builder = commands.register_system(build_panorama_content);
    commands
        .entity(handle.root)
        .insert(DeferredFloaterContent { builder, handle });
}

/// First-open content: the 2:1 preview, the three pickers, the two buttons and
/// the status line.
fn build_panorama_content(
    In(handle): In<FloaterHandle>,
    mut commands: Commands,
    state: Res<PanoramaState>,
) {
    let ui = spawn_panorama_content(&mut commands, handle.content, FONT_SIZE, &state);
    commands.insert_resource(ui);
}

// ---------------------------------------------------------------------------
// Gallery specimen.
// ---------------------------------------------------------------------------

/// The panorama floater's gallery / `ui_test` specimen: the live content,
/// built by the same `spawn_panorama_content` at the cell's font size on the
/// default picks, with a finished sample panorama put in the preview by the
/// same `show_preview` a capture ends in and the status line drawn by the
/// same `status_line` — the window as it looks after a capture.
///
/// The sample panorama is a synthetic sky-over-ground gradient: a specimen has
/// no camera to shoot six faces with.
pub fn spawn_panorama_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: sl_viewer_ui_core::ui_element::ElementCx,
) -> Entity {
    let ui = spawn_panorama_content(commands, parent, cx.font_size, &PanoramaState::default());
    commands.queue(move |world: &mut World| {
        if let Err(error) = world.run_system_cached_with(show_sample_capture, ui) {
            warn!("panorama specimen: the sample capture was not drawn: {error}");
        }
    });
    parent
}

/// The specimen's one-shot: show [`sample_panorama`] in the preview and the
/// "captured" status line, through the live helpers.
fn show_sample_capture(
    In(ui): In<PanoramaUi>,
    mut widgets: PreviewWidgets,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    show_preview(&ui, &sample_panorama(), &mut widgets);
    let status = StatusKind::Message(translator.get("panorama-captured"));
    set_status_text(&ui, &status_line(&status, &translator), &mut texts);
}

/// A synthetic 2:1 panorama for the specimen: a sky that pales towards the
/// horizon over a ground that darkens away from it, at the preview's size.
fn sample_panorama() -> RgbImage {
    let width = preview_dimension(PREVIEW_WIDTH);
    let height = preview_dimension(PREVIEW_HEIGHT);
    let horizon = height / 2;
    RgbImage::from_fn(width, height, |_x, y| {
        let (distance, span) = if y < horizon {
            (horizon.saturating_sub(y), horizon)
        } else {
            (y.saturating_sub(horizon), height.saturating_sub(horizon))
        };
        // 0 at the horizon, 255 at the zenith / nadir.
        let depth = distance
            .saturating_mul(255)
            .checked_div(span.max(1))
            .and_then(|depth| u8::try_from(depth).ok())
            .unwrap_or(u8::MAX);
        if y < horizon {
            image::Rgb([
                200_u8.saturating_sub(depth / 2),
                220_u8.saturating_sub(depth / 3),
                250,
            ])
        } else {
            image::Rgb([
                110_u8.saturating_sub(depth / 3),
                130_u8.saturating_sub(depth / 3),
                80_u8.saturating_sub(depth / 4),
            ])
        }
    })
}

/// Build the floater's content into `parent` at `font_size`, the pickers on
/// `state`'s choices: the 2:1 preview, the three pickers, the two buttons,
/// the status line and the hint. Returns the entities the systems update.
/// Shared by the live floater and its specimen.
fn spawn_panorama_content(
    commands: &mut Commands,
    parent: Entity,
    font_size: f32,
    state: &PanoramaState,
) -> PanoramaUi {
    let content = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_WIDTH),
                ..column(Val::Px(8.0))
            },
            ChildOf(parent),
        ))
        .id();

    let (preview, preview_hint) = spawn_preview(commands, content, font_size);

    let quality_combo = spawn_picker_row(
        commands,
        content,
        "panorama-quality-label",
        &ComboSpec {
            element: "panorama-quality",
            labels: &pixel_labels(FACE_SIZES),
            active: state.face_size,
            tab_index: 1,
            font_size,
            translate_labels: false,
        },
    );
    let width_combo = spawn_picker_row(
        commands,
        content,
        "panorama-width-label",
        &ComboSpec {
            element: "panorama-width",
            labels: &panorama_labels(),
            active: state.output_width,
            tab_index: 2,
            font_size,
            translate_labels: false,
        },
    );
    let format_combo = spawn_picker_row(
        commands,
        content,
        "panorama-format-label",
        &ComboSpec {
            element: "panorama-format",
            labels: &format_labels(),
            active: state.format,
            tab_index: 3,
            font_size,
            translate_labels: false,
        },
    );

    let buttons = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..row(Val::Px(8.0))
            },
            ChildOf(content),
        ))
        .id();
    let shoot = spawn_text_button(commands, buttons, "panorama-capture", 4, font_size);
    commands.entity(shoot).observe(
        |_activate: On<Activate>, requests: Option<MessageWriter<RequestPanoramaCapture>>| {
            // Absent only in the gallery, whose specimen has nothing to shoot.
            if let Some(mut requests) = requests {
                requests.write(RequestPanoramaCapture::Shoot);
            }
        },
    );
    let save = spawn_text_button(commands, buttons, "panorama-save-disk", 5, font_size);
    commands.entity(save).observe(
        |_activate: On<Activate>, requests: Option<MessageWriter<RequestPanoramaCapture>>| {
            // Absent only in the gallery, whose specimen has nothing to shoot.
            if let Some(mut requests) = requests {
                requests.write(RequestPanoramaCapture::Save);
            }
        },
    );

    let status = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            text_role(HINT_COLOR),
            Name::new("panorama-status"),
            ChildOf(content),
        ))
        .id();

    commands.spawn((
        Text::default(),
        Translated::new("panorama-hint"),
        UiFont::Sans.at(font_size),
        text_role(HINT_COLOR),
        Node {
            max_width: Val::Px(PREVIEW_WIDTH),
            ..default()
        },
        ChildOf(content),
    ));

    PanoramaUi {
        preview,
        preview_hint,
        quality_combo,
        width_combo,
        format_combo,
        status,
    }
}

/// Spawn the fixed 2:1 preview frame, returning the image node and the hint
/// shown until something has been captured into it (at `font_size`).
fn spawn_preview(commands: &mut Commands, parent: Entity, font_size: f32) -> (Entity, Entity) {
    let frame = commands
        .spawn((
            Node {
                width: Val::Px(PREVIEW_WIDTH),
                height: Val::Px(PREVIEW_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PREVIEW_BACKGROUND),
            BorderColor::all(PREVIEW_BORDER),
            Name::new("panorama-preview"),
            ChildOf(parent),
        ))
        .id();
    let preview = commands
        .spawn((
            ImageNode::default(),
            Node {
                display: Display::None,
                width: Val::Px(PREVIEW_WIDTH),
                height: Val::Px(PREVIEW_HEIGHT),
                ..default()
            },
            ChildOf(frame),
        ))
        .id();
    let preview_hint = commands
        .spawn((
            Text::default(),
            Translated::new("panorama-preview-empty"),
            UiFont::Sans.at(font_size),
            text_role(HINT_COLOR),
            ChildOf(frame),
        ))
        .id();
    (preview, preview_hint)
}

/// Spawn one `label: [combo]` row, the label at the combo's font size,
/// returning the combo anchor.
fn spawn_picker_row(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    combo: &ComboSpec,
) -> Entity {
    let row_entity = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                ..row(Val::Px(6.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(combo.font_size),
        text_role(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    spawn_combo(commands, row_entity, combo)
}

/// Spawn a translated-label push button at `font_size`, returning its
/// clickable box.
fn spawn_text_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab: i32,
    font_size: f32,
) -> Entity {
    ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::key(label_key), "panorama-button")
            .kind(ButtonKind::Headless)
            .tab_index(tab)
            .padding(10.0, 4.0)
            .border(2.0)
            .colors(BUTTON_BACKGROUND, BUTTON_BORDER)
            .label_color(LABEL_COLOR)
            .font_size(font_size)
            .layout(|node| node.align_self = AlignSelf::Start),
    )
    .button
}

/// `512 x 512` style labels for a list of square face sizes.
fn pixel_labels(sizes: &[u32]) -> Vec<String> {
    sizes
        .iter()
        .map(|size| format!("{size} × {size}"))
        .collect()
}

/// `4096 x 2048` style labels for the panorama widths.
fn panorama_labels() -> Vec<String> {
    OUTPUT_WIDTHS
        .iter()
        .map(|width| format!("{width} × {}", width / 2))
        .collect()
}

/// The format combo's literal labels.
fn format_labels() -> Vec<String> {
    FORMATS
        .iter()
        .map(|preset| preset.label.to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Preferences.
// ---------------------------------------------------------------------------

/// Load the persisted quality / width / format choices once the account scope
/// has resolved, and seed the combos with them.
fn load_persisted_preferences(
    settings: Res<ViewerSettings>,
    ui: Option<Res<PanoramaUi>>,
    mut state: ResMut<PanoramaState>,
    mut selections: Query<&mut ComboSelection>,
) {
    if state.loaded {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    if !settings.account_loaded() {
        return;
    }
    let store = settings.store();
    if let Ok(value) = store.get_i32(SETTING_FACE_SIZE) {
        state.face_size = clamp_index(value, FACE_SIZES.len());
    }
    if let Ok(value) = store.get_i32(SETTING_OUTPUT_WIDTH) {
        state.output_width = clamp_index(value, OUTPUT_WIDTHS.len());
    }
    if let Ok(value) = store.get_i32(SETTING_FORMAT) {
        state.format = clamp_index(value, FORMATS.len());
    }
    for (combo, active) in [
        (ui.quality_combo, state.face_size),
        (ui.width_combo, state.output_width),
        (ui.format_combo, state.format),
    ] {
        if let Ok(mut selection) = selections.get_mut(combo) {
            selection.active = active;
        }
    }
    state.loaded = true;
}

/// Clamp a stored index into `[0, len)`.
fn clamp_index(value: i32, len: usize) -> usize {
    let last = len.saturating_sub(1);
    usize::try_from(value).unwrap_or(0).min(last)
}

/// Apply the three combos' picks and persist them.
fn apply_combos(
    mut changes: MessageReader<ComboChanged>,
    ui: Option<Res<PanoramaUi>>,
    mut state: ResMut<PanoramaState>,
    mut settings: ResMut<ViewerSettings>,
) {
    let Some(ui) = ui else {
        changes.clear();
        return;
    };
    for change in changes.read() {
        let picked = i32::try_from(change.active).unwrap_or(0);
        if change.combo == ui.quality_combo {
            state.face_size = clamp_index(picked, FACE_SIZES.len());
            let value = i32::try_from(state.face_size).unwrap_or(0);
            settings.set_account(SETTING_FACE_SIZE, SettingValue::I32(value));
        } else if change.combo == ui.width_combo {
            state.output_width = clamp_index(picked, OUTPUT_WIDTHS.len());
            let value = i32::try_from(state.output_width).unwrap_or(0);
            settings.set_account(SETTING_OUTPUT_WIDTH, SettingValue::I32(value));
        } else if change.combo == ui.format_combo {
            state.format = clamp_index(picked, FORMATS.len());
            let value = i32::try_from(state.format).unwrap_or(0);
            settings.set_account(SETTING_FORMAT, SettingValue::I32(value));
        }
    }
}

/// Render the status line from the state each frame, so it re-localises when
/// the locale bundle loads or changes.
fn update_status_text(
    state: Res<PanoramaState>,
    ui: Option<Res<PanoramaUi>>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let Some(ui) = ui else {
        return;
    };
    set_status_text(&ui, &status_line(&state.status, &translator), &mut texts);
}

/// The status line's text for `status`.
fn status_line(status: &StatusKind, translator: &Translator) -> String {
    match status {
        StatusKind::Ready => translator.get("panorama-status-ready"),
        StatusKind::Settling => translator.get("panorama-status-settling"),
        StatusKind::Shooting { face } => translator.format(
            "panorama-status-shooting",
            &TransArgs::new()
                .text("face", &face.to_string())
                .text("total", &CUBE_FACES.len().to_string()),
        ),
        StatusKind::Stitching => translator.get("panorama-status-stitching"),
        StatusKind::Message(message) => message.clone(),
    }
}

/// Write `wanted` into the status line, touching it only on a change.
fn set_status_text(ui: &PanoramaUi, wanted: &str, texts: &mut Query<&mut Text>) {
    if let Ok(mut text) = texts.get_mut(ui.status)
        && text.0 != wanted
    {
        wanted.clone_into(&mut text.0);
    }
}

// ---------------------------------------------------------------------------
// Capture.
// ---------------------------------------------------------------------------

/// Begin a capture (or hand a finished one to the disk writer) on a request.
///
/// A Shoot while one is already in flight is dropped rather than queued: the
/// camera is already out on loan, and a second borrow would restore it to the
/// first borrow's state.
fn start_capture(
    mut requests: MessageReader<RequestPanoramaCapture>,
    mut state: ResMut<PanoramaState>,
    mut commands: Commands,
    translator: Translator,
) {
    let Some(request) = requests.read().last().copied() else {
        return;
    };
    match request {
        RequestPanoramaCapture::Shoot => {
            if state.phase != CapturePhase::Idle {
                return;
            }
            state.faces.clear();
            state.phase = CapturePhase::Settling {
                remaining: QUIET_TIMEOUT_SECONDS,
            };
            state.status = StatusKind::Settling;
        }
        RequestPanoramaCapture::Save => {
            let (Some(panorama), Some(metadata)) = (state.panorama.clone(), state.metadata.clone())
            else {
                state.status = StatusKind::Message(translator.get("panorama-nothing-captured"));
                return;
            };
            let extension = state.extension();
            match resolve_save_path(&mut state, extension) {
                Ok(path) => {
                    state.status = StatusKind::Stitching;
                    commands.spawn(PanoramaSaveTask(spawn_save_task(panorama, metadata, path)));
                }
                Err(message) => {
                    state.status = StatusKind::Message(translator.get(message));
                }
            }
        }
    }
}

/// The viewer camera's pose, lens and target: the three things a capture
/// borrows and hands back, named once so the driver and its helpers cannot
/// disagree about the shape of the query.
type BorrowedCameraQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static mut Transform,
        &'static mut Projection,
        &'static mut RenderTarget,
    ),
    With<ViewerCamera>,
>;

/// Everything [`drive_capture`] writes to, bundled so its signature stays
/// readable.
#[derive(bevy::ecs::system::SystemParam)]
struct CaptureRig<'w, 's> {
    /// What spawns the read-back entities and the stitch task.
    commands: Commands<'w, 's>,
    /// The camera on loan: its pose, lens and target.
    camera: BorrowedCameraQuery<'w, 's>,
    /// The image store the face target is created in.
    images: ResMut<'w, Assets<Image>>,
    /// The clock the freeze pauses.
    time: ResMut<'w, Time<Virtual>>,
    /// Real time, which the settle timeout counts in (virtual time is about to
    /// be paused, so it cannot time anything).
    real: Res<'w, Time<Real>>,
}

/// Advance the capture: wait for quiet, then point the camera at each face in
/// turn, read it back, and hand the camera back when the last one lands.
///
/// Runs inside the `CameraPositioned` set — see [`PanoramaPlugin::build`].
#[expect(
    clippy::too_many_lines,
    reason = "the six phases of one capture read as one sequence; splitting them across functions \
              would scatter the camera borrow's acquire and release, which is the one thing that \
              must stay visible in a single place"
)]
fn drive_capture(
    mut state: ResMut<PanoramaState>,
    mut captured: ResMut<CapturedFace>,
    mut rig: CaptureRig,
    borrowed: Option<ResMut<BorrowedCamera>>,
    quiescence: SceneQuiescence,
    region: Query<&SlRegionIdentity, With<SlCurrentRegion>>,
    translator: Translator,
) {
    match state.phase {
        CapturePhase::Idle | CapturePhase::Stitching => {}
        CapturePhase::Settling { remaining } => {
            let left = remaining - rig.real.delta_secs();
            if !quiescence.is_quiet() && left > 0.0 {
                state.phase = CapturePhase::Settling { remaining: left };
                return;
            }
            let Ok((_entity, transform, mut projection, mut target)) = rig.camera.single_mut()
            else {
                state.phase = CapturePhase::Idle;
                state.status = StatusKind::Message(translator.get("panorama-no-camera"));
                return;
            };
            let face_pixels = state.face_pixels();
            // The window's own surface format: an 8-bit sRGB target, so a face
            // is the transfer the window would have shown. The camera carries
            // `Hdr`, so the scene is still composed in float and tone-mapped
            // into this, exactly as the screenshot harness does it.
            let face_image = rig.images.add(Image::new_target_texture(
                face_pixels,
                face_pixels,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
            let previous_target = target.clone();
            let previous_projection = projection.clone();
            *target = RenderTarget::Image(face_image.clone().into());
            *projection = Projection::Perspective(cube_face_projection(&previous_projection));
            let time_was_paused = rig.time.is_paused();
            rig.time.pause();
            rig.commands.insert_resource(BorrowedCamera {
                target: previous_target,
                projection: previous_projection,
                rotation: transform.rotation,
                face_image,
                time_was_paused,
            });
            rig.commands.insert_resource(CameraBorrowed);
            state.phase = CapturePhase::Framing {
                face: 0,
                settle: SETTLE_FRAMES,
            };
            state.status = StatusKind::Shooting { face: 1 };
        }
        CapturePhase::Framing { face, settle } => {
            let Some(borrowed) = borrowed else {
                state.phase = CapturePhase::Idle;
                return;
            };
            point_at_face(&mut rig.camera, borrowed.rotation, face);
            if settle > 0 {
                state.phase = CapturePhase::Framing {
                    face,
                    settle: settle.saturating_sub(1),
                };
                return;
            }
            captured.0 = None;
            rig.commands
                .spawn(Screenshot::image(borrowed.face_image.clone()))
                .observe(
                    |shot: On<ScreenshotCaptured>,
                     mut captured: ResMut<CapturedFace>,
                     mut commands: Commands| {
                        captured.0 = Some(shot.image.clone());
                        commands.entity(shot.entity).despawn();
                    },
                );
            state.phase = CapturePhase::Exposing {
                face,
                patience: EXPOSURE_TIMEOUT_FRAMES,
            };
        }
        CapturePhase::Exposing { face, patience } => {
            let Some(borrowed) = borrowed else {
                state.phase = CapturePhase::Idle;
                return;
            };
            // Hold the face while the read-back is in flight, so the frame it
            // copies is the one that was composed for this face.
            point_at_face(&mut rig.camera, borrowed.rotation, face);
            let Some(image) = captured.0.take() else {
                if patience == 0 {
                    release_camera(&mut rig, &borrowed);
                    state.phase = CapturePhase::Idle;
                    state.status = StatusKind::Message(translator.get("panorama-capture-lost"));
                    return;
                }
                state.phase = CapturePhase::Exposing {
                    face,
                    patience: patience.saturating_sub(1),
                };
                return;
            };
            let Ok(dynamic) = image.try_into_dynamic() else {
                release_camera(&mut rig, &borrowed);
                state.phase = CapturePhase::Idle;
                state.status = StatusKind::Message(translator.get("panorama-capture-lost"));
                return;
            };
            // The alpha carries the glow mask, not coverage, so the face is
            // taken as opaque RGB — the same drop the snapshot floater makes.
            state.faces.push(dynamic.to_rgb8());
            let next = face.saturating_add(1);
            if next < CUBE_FACES.len() {
                state.phase = CapturePhase::Framing {
                    face: next,
                    settle: SETTLE_FRAMES,
                };
                state.status = StatusKind::Shooting {
                    face: next.saturating_add(1),
                };
                return;
            }
            let heading = heading_degrees(borrowed.rotation);
            let eye = rig
                .camera
                .single()
                .map_or(Vec3::ZERO, |(_entity, transform, _p, _t)| {
                    transform.translation
                });
            release_camera(&mut rig, &borrowed);
            let metadata = PanoramaMetadata {
                width: state.output_pixels(),
                height: state.output_pixels() / 2,
                heading_degrees: heading,
                face_size: state.face_pixels(),
                software: software_name(),
                region_name: region
                    .single()
                    .ok()
                    .and_then(|identity| identity.0.sim_name.clone())
                    .map(|name| name.to_string()),
                region_url: region
                    .single()
                    .ok()
                    .and_then(|identity| identity.0.sim_name.clone())
                    .map(|name| location_url(&name, eye)),
                captured_at: captured_at(),
            };
            let faces = core::mem::take(&mut state.faces);
            let width = state.output_pixels();
            state.metadata = Some(metadata);
            state.phase = CapturePhase::Stitching;
            state.status = StatusKind::Stitching;
            rig.commands
                .spawn(PanoramaStitchTask(spawn_stitch_task(faces, width)));
        }
    }
}

/// The cube-face lens: 90° vertical on a square target, keeping every other
/// number the viewer's own projection carried — the near and far planes above
/// all, so the panorama clips the world exactly where the live view does.
fn cube_face_projection(previous: &Projection) -> PerspectiveProjection {
    let mut perspective = match previous {
        Projection::Perspective(perspective) => perspective.clone(),
        Projection::Orthographic(..) | Projection::Custom(..) => {
            sl_viewer_world_scene::viewer_camera::viewer_projection()
        }
    };
    perspective.fov = core::f32::consts::FRAC_PI_2;
    perspective.aspect_ratio = 1.0;
    perspective
}

/// Point the borrowed camera at face `index`, in the frame the capture started
/// in.
fn point_at_face(camera: &mut BorrowedCameraQuery, base: Quat, index: usize) {
    let Some(face) = CUBE_FACES.get(index).copied() else {
        return;
    };
    let Ok((_entity, mut transform, _projection, _target)) = camera.single_mut() else {
        return;
    };
    // `mul_quat` / `mul_vec3` rather than the `*` operator: the workspace's
    // `arithmetic_side_effects` lint fires on glam's overloaded operators.
    let wanted = capture_basis(base).mul_quat(face.rotation());
    if transform.rotation != wanted {
        transform.rotation = wanted;
    }
}

/// The capture frame: the camera's heading with its pitch and roll removed, so
/// the panorama's horizon is level however the camera was tilted when the
/// shutter was pressed.
///
/// A tilted panorama is not a stylistic choice — a photo sphere is *displayed*
/// with its horizon horizontal, so a cube shot around a pitched camera comes
/// out with the world rolling through the frame.
fn capture_basis(rotation: Quat) -> Quat {
    let forward = rotation.mul_vec3(Vec3::NEG_Z);
    // Level the look direction; a camera pointing straight up or down leaves
    // nothing to take a heading from, so it keeps the frame's own X axis.
    let levelled = Vec3::new(forward.x, 0.0, forward.z);
    let heading = if levelled.length_squared() > f32::EPSILON {
        levelled.normalize()
    } else {
        let right = rotation.mul_vec3(Vec3::X);
        Vec3::new(-right.z, 0.0, right.x).normalize_or(Vec3::NEG_Z)
    };
    Quat::from_rotation_arc(Vec3::NEG_Z, heading)
}

/// The compass heading, in degrees clockwise from north, the capture frame
/// looks along.
///
/// Second Life's `+Y` is north and its `+X` east; the Bevy world those map into
/// puts north at `-Z` and east at `+X` (`sl_viewer_kit::coords`), so the
/// heading of a Bevy direction is `atan2(east, north)`.
fn heading_degrees(rotation: Quat) -> f32 {
    let forward = capture_basis(rotation).mul_vec3(Vec3::NEG_Z);
    let degrees = forward.x.atan2(-forward.z).to_degrees();
    if degrees < 0.0 {
        degrees + 360.0
    } else {
        degrees
    }
}

/// Give the camera back: its target, its lens, its pose and the clock.
fn release_camera(rig: &mut CaptureRig, borrowed: &BorrowedCamera) {
    if let Ok((_entity, mut transform, mut projection, mut target)) = rig.camera.single_mut() {
        transform.rotation = borrowed.rotation;
        // Assigning through the `Mut` marks the projection changed, which is
        // what makes `camera_system` refresh the target size for the target
        // going back underneath it.
        *projection = borrowed.projection.clone();
        *target = borrowed.target.clone();
    }
    if !borrowed.time_was_paused {
        rig.time.unpause();
    }
    // The face target is a full-resolution square texture; dropping it here is
    // what keeps a session's captures from accumulating on the GPU.
    if rig.images.remove(&borrowed.face_image).is_none() {
        warn!("panorama: the face render target was already gone when the camera was handed back");
    }
    rig.commands.remove_resource::<BorrowedCamera>();
    rig.commands.remove_resource::<CameraBorrowed>();
}

/// The viewer's name and version, as the XMP's capture / stitching software.
fn software_name() -> String {
    format!("sl-client-bevy-viewer {}", env!("CARGO_PKG_VERSION"))
}

/// A link back to where the panorama was shot.
fn location_url(region: &sl_types::map::RegionName, eye: Vec3) -> String {
    let local = sl_viewer_kit::coords::bevy_to_sl_vec(eye);
    sl_types::map::Location::new(
        region.clone(),
        clamp_to_u8(local.x),
        clamp_to_u8(local.y),
        clamp_to_u16(local.z),
    )
    .as_maps_url()
}

/// A region-local metre coordinate as the `0..=255` a map URL carries.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=255 before the conversion"
)]
fn clamp_to_u8(value: f32) -> u8 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= 255.0 {
        u8::MAX
    } else {
        value as u8
    }
}

/// A height in metres as the `u16` a map URL carries.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=4095 before the conversion"
)]
fn clamp_to_u16(value: f32) -> u16 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= 4095.0 {
        4095
    } else {
        value as u16
    }
}

/// The capture time as an RFC-3339 timestamp, which is what the GPano date
/// fields want.
///
/// **UTC**, like the scene dump's stamp and unlike the snapshot floater's
/// filename: `time`'s local-offset lookup refuses to run in a process with
/// other threads (it is unsound in one), and a panorama's metadata is read by
/// strangers' software, where an unambiguous `Z` beats a friendly local hour.
fn captured_at() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Stitching and saving, off the frame thread.
// ---------------------------------------------------------------------------

/// The off-thread reprojection of a finished cube.
#[derive(Component)]
struct PanoramaStitchTask(Task<Result<RgbImage, String>>);

/// The off-thread encode and write of a finished panorama.
#[derive(Component)]
struct PanoramaSaveTask(Task<Result<PathBuf, String>>);

/// Reproject the six faces into a panorama, off the frame thread.
///
/// Tens of millions of bilinear taps is not a thing to do between two frames;
/// [`equirect::reproject`] spreads them over the compute pool from inside this
/// task, so the viewer keeps drawing (and the world keeps moving, the freeze
/// having been lifted with the camera) while it runs.
fn spawn_stitch_task(faces: Vec<RgbImage>, width: u32) -> Task<Result<RgbImage, String>> {
    IoTaskPool::get().spawn(async move {
        let cube = CubeFaces::from_faces(faces).map_err(|error| error.to_string())?;
        equirect::reproject(&cube, width).map_err(|error| error.to_string())
    })
}

/// Encode a finished panorama with its XMP packet and write it, off the frame
/// thread.
fn spawn_save_task(
    panorama: RgbImage,
    metadata: PanoramaMetadata,
    path: PathBuf,
) -> Task<Result<PathBuf, String>> {
    IoTaskPool::get().spawn(async move {
        let packet = xmp::packet(&metadata);
        let encoded = encode_with_metadata(&panorama, &path, &packet)?;
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs_err::write(&path, encoded).map_err(|error| error.to_string())?;
        Ok(path)
    })
}

/// Encode `panorama` for `path`'s extension and splice `packet` into the
/// result.
///
/// Split out of the task so the encode-then-splice order — which is the whole
/// reason a panorama is not written with `DynamicImage::save` — is one readable
/// function.
fn encode_with_metadata(
    panorama: &RgbImage,
    path: &std::path::Path,
    packet: &str,
) -> Result<Vec<u8>, String> {
    let format = image::ImageFormat::from_path(path).map_err(|error| error.to_string())?;
    let mut encoded = Vec::new();
    match format {
        image::ImageFormat::Jpeg => {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
                .encode_image(panorama)
                .map_err(|error| error.to_string())?;
            xmp::embed_in_jpeg(&encoded, packet).map_err(|error| error.to_string())
        }
        image::ImageFormat::Png => {
            image::codecs::png::PngEncoder::new(&mut encoded)
                .write_image(
                    panorama.as_raw(),
                    panorama.width(),
                    panorama.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|error| error.to_string())?;
            xmp::embed_in_png(&encoded, packet).map_err(|error| error.to_string())
        }
        _other => Err(format!(
            "{format:?} carries no XMP, so a panorama written as one would not open as a sphere"
        )),
    }
}

/// Where the next panorama is written: the snapshots directory, a local
/// timestamp, the per-session counter and the panorama's shape.
fn resolve_save_path(
    state: &mut PanoramaState,
    extension: &'static str,
) -> Result<PathBuf, &'static str> {
    let directory = sl_viewer_platform::paths::snapshots_dir().ok_or("panorama-no-dir")?;
    state.counter = state.counter.wrapping_add(1);
    let width = state.output_pixels();
    let name = format!(
        "sl360-{}-{}-{width}x{}.{extension}",
        file_stamp(),
        state.counter,
        width / 2
    );
    Ok(directory.join(name))
}

/// The capture time as a filename-safe stamp (UTC, see [`captured_at`]), with
/// the time's colons written as dashes so the name is valid on every
/// filesystem.
fn file_stamp() -> String {
    let format =
        time::macros::format_description!("[year]-[month]-[day]T[hour]-[minute]-[second]Z");
    time::OffsetDateTime::now_utc()
        .format(format)
        .unwrap_or_default()
}

/// What a finished panorama is shown in, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the image store the
/// thumbnail is uploaded into, the preview image node, and the boxes whose
/// display the first capture flips.
#[derive(bevy::ecs::system::SystemParam)]
struct PreviewWidgets<'w, 's> {
    /// The image store the thumbnail is uploaded into.
    images: ResMut<'w, Assets<Image>>,
    /// The preview's image node, repointed at the fresh thumbnail.
    image_nodes: Query<'w, 's, &'static mut ImageNode>,
    /// The preview box and the hint it replaces.
    nodes: Query<'w, 's, &'static mut Node>,
}

/// Drain the finished reprojections: keep the panorama, show it in the preview.
fn poll_stitch(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PanoramaStitchTask)>,
    mut state: ResMut<PanoramaState>,
    ui: Option<Res<PanoramaUi>>,
    mut widgets: PreviewWidgets,
    translator: Translator,
) {
    for (entity, mut task) in &mut tasks {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            continue;
        };
        commands.entity(entity).despawn();
        state.phase = CapturePhase::Idle;
        match result {
            Ok(panorama) => {
                if let Some(ui) = ui.as_deref() {
                    show_preview(ui, &panorama, &mut widgets);
                }
                state.panorama = Some(panorama);
                state.status = StatusKind::Message(translator.get("panorama-captured"));
            }
            Err(error) => {
                state.status = StatusKind::Message(translator.format(
                    "panorama-save-failed",
                    &TransArgs::new().text("error", &error),
                ));
            }
        }
    }
}

/// Drain the finished writes: echo the saved path to nearby chat and the
/// status line, or surface the write error.
fn poll_saves(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PanoramaSaveTask)>,
    mut state: ResMut<PanoramaState>,
    mut notices: MessageWriter<LocalChatNotice>,
    translator: Translator,
) {
    for (entity, mut task) in &mut tasks {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            continue;
        };
        commands.entity(entity).despawn();
        match result {
            Ok(path) => {
                let saved = path.display().to_string();
                let message =
                    translator.format("panorama-saved", &TransArgs::new().text("path", &saved));
                notices.write(LocalChatNotice::new(message.clone()));
                state.status = StatusKind::Message(message);
            }
            Err(error) => {
                state.status = StatusKind::Message(translator.format(
                    "panorama-save-failed",
                    &TransArgs::new().text("error", &error),
                ));
            }
        }
    }
}

/// Put a finished panorama in the floater's preview, downscaled to the frame.
///
/// Downscaled on the CPU rather than by letting the UI stretch a 4096-wide
/// texture into a 480-pixel box: the node would sample it once per screen
/// pixel with no mip chain, which on a panorama's fine detail is a shimmering
/// mess.
fn show_preview(ui: &PanoramaUi, panorama: &RgbImage, widgets: &mut PreviewWidgets) {
    let thumbnail = image::imageops::resize(
        panorama,
        preview_dimension(PREVIEW_WIDTH),
        preview_dimension(PREVIEW_HEIGHT),
        image::imageops::FilterType::Triangle,
    );
    let handle = widgets.images.add(Image::from_dynamic(
        image::DynamicImage::ImageRgb8(thumbnail),
        true,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
    if let Ok(mut image_node) = widgets.image_nodes.get_mut(ui.preview) {
        image_node.image = handle;
    }
    if let Ok(mut node) = widgets.nodes.get_mut(ui.preview) {
        node.display = Display::Flex;
    }
    if let Ok(mut hint) = widgets.nodes.get_mut(ui.preview_hint) {
        hint.display = Display::None;
    }
}

/// A preview box dimension in logical pixels as a texture size.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the preview box is a small positive constant; the rounded value fits a u32 exactly"
)]
fn preview_dimension(value: f32) -> u32 {
    if value.is_finite() && value >= 1.0 {
        value.round() as u32
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CUBE_FACES, FACE_SIZES, FORMATS, OUTPUT_WIDTHS, PREVIEW_HEIGHT, PREVIEW_WIDTH,
        PanoramaState, capture_basis, clamp_index, clamp_to_u8, clamp_to_u16, cube_face_projection,
        heading_degrees, panorama_labels, pixel_labels, preview_dimension, sample_panorama,
    };
    use bevy::camera::{PerspectiveProjection, Projection};
    use bevy::math::{Quat, Vec3};
    use pretty_assertions::assert_eq;

    /// How far two headings may differ and still be the same one, in degrees.
    const HEADING_EPSILON: f32 = 1.0e-3;

    /// How far a lens number may differ and still be the same one.
    const LENS_EPSILON: f32 = 1.0e-4;

    /// The specimen's sample panorama is exactly the preview's size, so
    /// `show_preview` draws it unscaled, and reads as sky over ground: the
    /// zenith row is bluer than the nadir row.
    #[test]
    fn sample_panorama_fills_the_preview_sky_over_ground() {
        let panorama = sample_panorama();
        assert_eq!(
            panorama.dimensions(),
            (
                preview_dimension(PREVIEW_WIDTH),
                preview_dimension(PREVIEW_HEIGHT)
            )
        );
        let zenith = panorama.get_pixel(0, 0).0;
        let nadir = panorama.get_pixel(0, panorama.height().saturating_sub(1)).0;
        assert!(
            zenith.get(2) > nadir.get(2),
            "the sky must be bluer than the ground: {zenith:?} over {nadir:?}"
        );
    }

    /// Every offered output width is even, so its 2:1 height is a whole number
    /// of pixels — an odd one would make an equirectangular image that is not
    /// quite 2:1.
    #[test]
    fn every_output_width_is_an_even_two_to_one() {
        for width in OUTPUT_WIDTHS {
            assert_eq!(width % 2, 0, "{width} has no whole-pixel 2:1 height");
        }
    }

    /// The defaults index into their own lists.
    #[test]
    fn the_defaults_are_in_range() {
        let state = PanoramaState::default();
        assert!(FACE_SIZES.get(state.face_size).is_some());
        assert!(OUTPUT_WIDTHS.get(state.output_width).is_some());
        assert!(FORMATS.get(state.format).is_some());
    }

    /// Every offered format is one an XMP packet can be put into — the reason
    /// the list is shorter than the snapshot floater's.
    #[test]
    fn every_format_can_carry_metadata() {
        for preset in FORMATS {
            assert!(
                matches!(preset.extension, "jpg" | "png"),
                "{} has no metadata container",
                preset.extension
            );
        }
    }

    /// A stored index out of range falls back into it rather than panicking or
    /// selecting nothing.
    #[test]
    fn a_stored_index_is_clamped() {
        assert_eq!(clamp_index(-3, 4), 0);
        assert_eq!(clamp_index(9, 4), 3);
        assert_eq!(clamp_index(2, 4), 2);
    }

    /// The cube lens is 90° on a square target and keeps the viewer's own clip
    /// planes, so the panorama clips the world where the live view does.
    #[test]
    fn the_cube_lens_is_ninety_degrees_and_keeps_the_clip_planes() {
        let previous = Projection::Perspective(PerspectiveProjection {
            fov: core::f32::consts::FRAC_PI_3,
            aspect_ratio: 1.7,
            near: 0.02,
            far: 4096.0,
            ..PerspectiveProjection::default()
        });
        let cube = cube_face_projection(&previous);
        assert!((cube.fov - core::f32::consts::FRAC_PI_2).abs() < LENS_EPSILON);
        assert!((cube.aspect_ratio - 1.0).abs() < LENS_EPSILON);
        assert!((cube.near - 0.02).abs() < LENS_EPSILON);
        assert!((cube.far - 4096.0).abs() < LENS_EPSILON);
    }

    /// The capture frame is level whatever the camera was doing: a pitched or
    /// rolled camera still produces a panorama whose horizon is horizontal.
    #[test]
    fn the_capture_frame_is_levelled() {
        let pitched = Quat::from_rotation_x(0.6).mul_quat(Quat::from_rotation_z(0.2));
        let basis = capture_basis(pitched);
        let up = basis.mul_vec3(Vec3::Y);
        assert!(
            up.distance(Vec3::Y) < 1.0e-4,
            "the capture frame's up is {up:?}, not the world's"
        );
        let forward = basis.mul_vec3(Vec3::NEG_Z);
        assert!(
            forward.y.abs() < 1.0e-4,
            "the capture frame looks {forward:?}, which is not level"
        );
    }

    /// A camera facing north reads as heading 0, east as 90 — the compass the
    /// GPano pose field is written in. Second Life's north is the Bevy world's
    /// `-Z` and its east the `+X`.
    #[test]
    fn headings_are_read_off_the_second_life_compass() {
        let north = Quat::IDENTITY;
        assert!(heading_degrees(north).abs() < HEADING_EPSILON);
        let east = Quat::from_rotation_y(-core::f32::consts::FRAC_PI_2);
        assert!((heading_degrees(east) - 90.0).abs() < HEADING_EPSILON);
        let south = Quat::from_rotation_y(core::f32::consts::PI);
        assert!((heading_degrees(south) - 180.0).abs() < HEADING_EPSILON);
        let west = Quat::from_rotation_y(core::f32::consts::FRAC_PI_2);
        assert!((heading_degrees(west) - 270.0).abs() < HEADING_EPSILON);
    }

    /// A camera pointing straight down still yields a usable capture frame
    /// rather than a degenerate one — the panorama is taken about a level
    /// horizon derived from the camera's roll.
    #[test]
    fn a_camera_looking_straight_down_still_has_a_frame() {
        let down = Quat::from_rotation_x(-core::f32::consts::FRAC_PI_2);
        let basis = capture_basis(down);
        let forward = basis.mul_vec3(Vec3::NEG_Z);
        assert!(forward.is_finite() && forward.y.abs() < 1.0e-4);
    }

    /// The combo labels say what they offer, in the shape a user reads.
    #[test]
    fn the_pickers_are_labelled_with_their_pixels() {
        assert_eq!(
            pixel_labels(&[512]),
            vec!["512 × 512".to_owned()],
            "a cube face is square"
        );
        assert!(
            panorama_labels()
                .first()
                .is_some_and(|label| label.contains('×')),
            "a panorama label names both dimensions"
        );
    }

    /// Map-URL coordinates are clamped into the range the URL form carries.
    #[test]
    fn map_coordinates_are_clamped() {
        assert_eq!(clamp_to_u8(-5.0), 0);
        assert_eq!(clamp_to_u8(300.0), 255);
        assert_eq!(clamp_to_u8(128.4), 128);
        assert_eq!(clamp_to_u16(-1.0), 0);
        assert_eq!(clamp_to_u16(9000.0), 4095);
    }

    /// A cube has six faces, and the capture shoots each of them once.
    #[test]
    fn the_capture_shoots_every_face() {
        assert_eq!(CUBE_FACES.len(), 6);
    }
}
