//! A debug screenshot-capture harness (used to diagnose R11, the base-body skin
//! distortion under animation).
//!
//! When `SL_VIEWER_SCREENSHOT_DIR` is set, the viewer saves a numbered sequence
//! of PNG frames at a fixed interval — after a startup delay long enough for
//! login, asset decode, baking, and the debug animation to settle — then quits.
//! This lets an animated avatar be inspected offline, frame by frame, without an
//! operator sitting at the live window, and (since it leaves the cursor
//! un-grabbed) without hijacking the desktop it runs on. It is also one half of
//! the Firestorm cross-check, which is why the frame's size and content are
//! pinned rather than inherited from the window — see below.
//!
//! The per-frame PNG encode + disk write is done **off the main thread** on Bevy's
//! [`IoTaskPool`] (like the user-facing Snapshot floater), rather than with Bevy's
//! synchronous `save_to_disk` observer. A full-resolution PNG deflate on the frame
//! thread stalls the frame and spikes the next frame's `Time::delta`, which made
//! time-based animations (the water surface, driven by `time.elapsed_secs()`) jump
//! on the catch-up frame — the "water briefly accelerates then normal" artifact
//! seen during capture runs. Off-thread, the capture costs the frame nothing past
//! the (already off-thread) GPU read-back, so the harness better reflects live
//! behaviour.
//!
//! # The pinned capture size, and why it is not the window's
//!
//! A frame captured from the window is whatever size the window happened to be,
//! which is fine while the only consumer is this viewer's own eyes, and fatal the
//! moment a frame is put beside Firestorm's: two images of different dimensions
//! cannot be diffed, tiled into a contact sheet, or compared at a named pixel.
//!
//! So a run never captures the window. Every camera of the capture renders into
//! an off-screen image of exactly [`CaptureSize`] — 1080p unless
//! `--capture-size WxH` (env `SL_VIEWER_CAPTURE_SIZE`, the same variable the
//! Firestorm harness reads, so one env block sizes both viewers) says otherwise
//! — and the frame is read from that. Asking the *window* to be `WxH` would not
//! do: a window size is a *request*, and a tiling compositor answers it with its
//! own size, mid-run and more than once (Firestorm's harness watched a sequence
//! change resolution between `frame_000` and `frame_001` when the window lost
//! focus). A harness whose resolution is chosen by the window manager produces
//! frames that cannot be compared with the other viewer's, or even with each
//! other.
//!
//! # The layers in the frame
//!
//! The composited frame is four passes with four cameras — the world, the edit
//! gizmos, the HUD attachments, and the UI (which shares the HUD's camera, being
//! drawn by `bevy_ui` on the default UI camera). [`CaptureContent`] routes each
//! **independently**: `--capture-ui`, `--capture-hud` and `--capture-gizmos`
//! (envs `SL_VIEWER_CAPTURE_{UI,HUD,GIZMOS}`), each off by default, so the frame
//! holds the world alone unless asked — which is what a renderer comparison
//! wants, and what the Firestorm side captures by default too.
//!
//! They are separate switches, not one "chrome" switch, because the questions
//! are separate: *does the other viewer draw this HUD the same way* is asked
//! with the HUD in and the UI out, and neither answer should require the other
//! layer to be in the picture.
//!
//! A layer that is not in the frame keeps the window where it can be watched —
//! with one exception. The HUD layer and the UI are one camera, so a run that
//! asks for exactly one of them **hides** the other rather than leaving it
//! somewhere: `--capture-hud` alone means no UI anywhere for that run.
//!
//! # The window preview
//!
//! With every camera of the capture rendering off-screen, the window would show
//! nothing at all, so the harness puts the captured image on a quad in front of a
//! camera of its own: what is on screen is what lands in the frame. It is a
//! textured quad rather than a UI image node for two reasons, both learned the
//! hard way, and it matches the window's other cameras' sample count and
//! HDR-ness for a third — see `spawn_capture_preview`.

use core::str::FromStr;
use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use bevy::window::WindowRef;
use sl_client_bevy::SlCommand;

use crate::harness_status::HarnessStatus;
use crate::quiescence::SceneQuiescence;
use crate::session::{ViewerSession, request_logout};
use crate::world_api::{OverlayCamera, ViewerCamera};

/// The offline-inspection screenshot harness (R11): capture a numbered PNG
/// sequence into `dir` after a startup delay, then quit.
///
/// Added only in screenshot mode, which is why the schedule resource is carried
/// on the plugin rather than initialised from the world.
#[derive(Debug)]
pub struct ScreenshotPlugin {
    /// Directory the PNG sequence is written to.
    pub dir: PathBuf,
    /// The pixel grid the frames are rendered at, and which layers of the
    /// composited frame they hold. See [the module docs](self).
    pub content: CaptureContent,
    /// Whether this run logs into a grid, and so whether "the region never came
    /// up" means the run failed.
    ///
    /// False in `--replay`, which rebuilds an avatar from a captured bundle with
    /// no grid at all: there, waiting for a region would be waiting for
    /// something that is never coming, and the frames of the replayed avatar are
    /// exactly what the run is for.
    pub grid_expected: bool,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ScreenshotSchedule::new(
            self.dir.clone(),
            self.grid_expected,
        ))
        .insert_resource(self.content)
        // After `Startup`, which is where the viewer spawns its one camera.
        .add_systems(PostStartup, pin_capture_target)
        .add_systems(
            Update,
            (
                route_overlay_cameras,
                capture_screenshots,
                poll_screenshot_saves,
            )
                .chain(),
        );
    }
}

/// What a captured frame holds: the pixel grid it is rendered at, and which of
/// the composited frame's layers are in it.
///
/// Each layer is an independent choice, and independent of the size, so a run
/// can ask for exactly one comparison — the HUD without the UI over it, the UI
/// without the HUD, the world alone. The world is always in the frame; there
/// would be nothing to compare without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct CaptureContent {
    /// The pixel grid every frame is rendered at.
    pub size: CaptureSize,
    /// Whether the viewer's `bevy_ui` interface is in the frame.
    pub ui: bool,
    /// Whether the HUD-attachment layer is in the frame.
    pub hud: bool,
    /// Whether the edit-tool gizmo overlay is in the frame.
    pub gizmos: bool,
}

impl CaptureContent {
    /// The world alone at [`CaptureSize::DEFAULT`] — what a cross-viewer render
    /// comparison wants, and what the Firestorm side captures by default.
    pub const WORLD_ONLY: Self = Self {
        size: CaptureSize::DEFAULT,
        ui: false,
        hud: false,
        gizmos: false,
    };

    /// Whether an overlay camera draws anything this capture asked for — and so
    /// whether it is routed into the captured frame or left on the window.
    ///
    /// The HUD camera answers for two layers, being `bevy_ui`'s default camera
    /// as well as the HUD's: it is routed when *either* was asked for, and
    /// [`route_overlay_cameras`] then hides whichever was not.
    #[must_use]
    const fn draws_wanted_layer(self, camera: OverlayCamera) -> bool {
        match camera {
            OverlayCamera::Gizmos => self.gizmos,
            OverlayCamera::HudAndUi => self.ui || self.hud,
        }
    }

    /// Whether one of the two layers sharing the HUD camera was asked for and the
    /// other was not — the only case in which the harness hides content, since
    /// the two cannot be routed apart.
    #[must_use]
    const fn splits_hud_camera(self) -> bool {
        self.ui != self.hud
    }

    /// The layers in the frame, for the run's opening log line: a run that
    /// captured the wrong thing should say so in its first line rather than in
    /// its frames.
    #[must_use]
    fn describe(self) -> String {
        let mut layers = vec!["world"];
        if self.gizmos {
            layers.push("gizmos");
        }
        if self.hud {
            layers.push("HUD");
        }
        if self.ui {
            layers.push("UI");
        }
        layers.join(" + ")
    }
}

/// The pixel grid a harness run's frames are rendered at (`--capture-size`, env
/// `SL_VIEWER_CAPTURE_SIZE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureSize {
    /// Frame width, in pixels.
    pub width: u32,
    /// Frame height, in pixels.
    pub height: u32,
}

/// The largest capture dimension accepted. `wgpu`'s downlevel-guaranteed maximum
/// 2D texture size, so a pinned target is one every adapter can allocate — a
/// typo that asks for a 100000-pixel frame is refused at the command line rather
/// than at the first capture, half an hour into a run.
const MAX_CAPTURE_DIMENSION: u32 = 8192;

impl CaptureSize {
    /// 1080p: the grid both viewers capture at unless told otherwise.
    ///
    /// A real default rather than "whatever the window is", for the reason in
    /// [the module docs](self) — and 1080p rather than something smaller because
    /// the fine detail a comparison is looking for (texture banding, a mesh LOD
    /// swap, an alpha-sorting seam) has to survive into the frame.
    pub const DEFAULT: Self = Self {
        width: 1920,
        height: 1080,
    };

    /// The frame's aspect ratio, for the window preview's letterbox.
    ///
    /// Converted through `u16` rather than an `as` cast (the workspace bans
    /// those); every accepted dimension is well inside `u16` by
    /// [`MAX_CAPTURE_DIMENSION`].
    #[must_use]
    fn aspect(self) -> f32 {
        let width = f32::from(u16::try_from(self.width).unwrap_or(u16::MAX));
        let height = f32::from(u16::try_from(self.height).unwrap_or(u16::MAX));
        width / height
    }
}

impl core::fmt::Display for CaptureSize {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl FromStr for CaptureSize {
    type Err = String;

    /// Parse `WIDTHxHEIGHT`.
    ///
    /// Every malformed value is an error rather than a fallback to some default:
    /// a silent fallback produces a whole run of unusable frames whose only
    /// symptom is that the diff step later refuses them.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let (width, height) = trimmed.split_once(['x', 'X']).ok_or_else(|| {
            format!("expected a capture size as `WIDTHxHEIGHT` (e.g. `1920x1080`), got `{text}`")
        })?;
        Ok(Self {
            width: parse_dimension("width", width)?,
            height: parse_dimension("height", height)?,
        })
    }
}

/// One side of a [`CaptureSize`]: a positive pixel count no larger than
/// [`MAX_CAPTURE_DIMENSION`].
fn parse_dimension(field: &str, text: &str) -> Result<u32, String> {
    let value: u32 = text
        .trim()
        .parse()
        .map_err(|error| format!("capture {field} `{text}` is not a pixel count: {error}"))?;
    if value == 0 {
        return Err(format!("capture {field} must not be zero"));
    }
    if value > MAX_CAPTURE_DIMENSION {
        return Err(format!(
            "capture {field} {value} is larger than the {MAX_CAPTURE_DIMENSION} pixels every \
             adapter can allocate"
        ));
    }
    Ok(value)
}

/// The `--capture-size` command-line value parser, so a malformed value is
/// refused by `clap` before the viewer starts rather than a run in.
///
/// # Errors
///
/// Returns the parse failure's message when `text` is not `WIDTHxHEIGHT` with two
/// positive dimensions no larger than the 8192 pixels every adapter can
/// allocate.
pub fn parse_capture_size(text: &str) -> Result<CaptureSize, String> {
    CaptureSize::from_str(text)
}

/// The screenshot capture schedule, inserted only in screenshot mode.
#[derive(Debug, Resource)]
pub(crate) struct ScreenshotSchedule {
    /// Directory the PNG sequence is written to.
    dir: PathBuf,
    /// The first capture's **timeout**, in seconds from startup: the capture
    /// itself fires when the scene goes quiet (see [`SceneQuiescence`]), and
    /// this is how long a permanently-busy scene is given before a frame is
    /// taken anyway — captured either way, so a run always produces something.
    start_delay: f32,
    /// Seconds between successive captures.
    interval: f32,
    /// How many frames to capture before quitting.
    max_frames: usize,
    /// The next capture time (elapsed seconds); `None` until the scene has
    /// settled (or the timeout fired) and the first capture is armed.
    next_at: Option<f32>,
    /// The index of the next frame to write.
    index: usize,
    /// How many frames actually reached the disk, which is what the status file
    /// reports: a frame captured is not a frame written, and the difference is
    /// the whole reason a harness reads the counts rather than the run's word.
    written: usize,
    /// When the region came up (elapsed seconds), once it has.
    region_seen_at: Option<f32>,
    /// Consecutive frames the scene has been quiet.
    quiet_frames: u32,
    /// How long the run waits to get in world before giving up, in seconds from
    /// startup (`SL_VIEWER_LOGIN_TIMEOUT`, default 180) — the same variable and
    /// default the Firestorm harness uses.
    login_timeout: f32,
    /// Set when the run has gone wrong in a way it cannot capture through: no
    /// camera to capture, a frame that failed to reach the disk, a login that
    /// never landed. Carries the `reason` the status file reports.
    failure: Option<String>,
    /// Whether the first capture fired on a settled scene or on the timeout,
    /// which the status file's `reason` distinguishes: a frame taken mid-load is
    /// worth having as long as nobody reads it as a settled one.
    settled: bool,
    /// Whether [`HarnessStatus`] has already been written, so the "everything is
    /// written" branch — reached on every frame of the logout — writes it once.
    status_written: bool,
    /// Whether a region is expected at all — false for a grid-less `--replay`
    /// run, where nothing should wait for a login that was never attempted.
    grid_expected: bool,
}

/// How many consecutive quiet frames the first capture waits for: long enough
/// that a lull between a decode finishing and the next fetch being issued does
/// not read as settled.
const QUIET_HOLD_FRAMES: u32 = 30;

/// The least seconds after the region comes up before the first capture, so
/// the burst of fetches a handshake sets off has begun (an instant of quiet
/// right after arrival is not a loaded scene).
const MIN_SETTLE_SECS: f32 = 5.0;

/// How long a run waits to reach the world before it gives up, unless
/// `SL_VIEWER_LOGIN_TIMEOUT` says otherwise. Firestorm's harness defaults to
/// the same 180 s.
const DEFAULT_LOGIN_TIMEOUT_SECS: f32 = 180.0;

impl ScreenshotSchedule {
    /// A schedule writing `SL_VIEWER_SCREENSHOT_FRAMES` frames (default 30) at
    /// `SL_VIEWER_SCREENSHOT_INTERVAL` s (default 0.5), the first once the
    /// scene has gone **quiet** — with `SL_VIEWER_SCREENSHOT_DELAY` s (default
    /// 25) as the timeout after which a frame is captured anyway. Quiet makes
    /// two runs comparable by construction; the timeout keeps a
    /// permanently-busy scene from hanging the run.
    #[must_use]
    pub(crate) fn new(dir: PathBuf, grid_expected: bool) -> Self {
        let env_f32 = |key: &str, default: f32| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        let env_usize = |key: &str, default: usize| {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        Self {
            dir,
            start_delay: env_f32("SL_VIEWER_SCREENSHOT_DELAY", 25.0),
            interval: env_f32("SL_VIEWER_SCREENSHOT_INTERVAL", 0.5),
            max_frames: env_usize("SL_VIEWER_SCREENSHOT_FRAMES", 30),
            next_at: None,
            index: 0,
            written: 0,
            region_seen_at: None,
            quiet_frames: 0,
            login_timeout: env_f32("SL_VIEWER_LOGIN_TIMEOUT", DEFAULT_LOGIN_TIMEOUT_SECS),
            failure: None,
            settled: false,
            status_written: false,
            grid_expected,
        }
    }

    /// Write `harness-status.json` for this run, once.
    ///
    /// Called before the logout rather than after it: a status written on the
    /// way out is a status a killed viewer never writes, and *no file* is how a
    /// driving harness tells "the run did not happen" from "the viewers drew
    /// different things" — see [`crate::harness_status`].
    fn write_status(&mut self) {
        if self.status_written {
            return;
        }
        self.status_written = true;
        let status = match &self.failure {
            Some(reason) => HarnessStatus::new(false, reason, self.written, self.max_frames),
            None if self.settled => {
                HarnessStatus::new(true, "complete", self.written, self.max_frames)
            }
            None => HarnessStatus::new(
                true,
                "complete (captured before the scene settled)",
                self.written,
                self.max_frames,
            ),
        };
        if let Err(error) = status.write(&self.dir) {
            // The frames are already on disk and the run is over; a harness that
            // finds no status treats this as a run that did not happen, which is
            // the right conclusion from an unwritable directory.
            error!("screenshot: {error}");
        }
    }
}

/// The pinned off-screen capture, installed once by [`pin_capture_target`].
#[derive(Debug, Resource)]
pub(crate) struct PinnedCapture {
    /// The image every camera of the capture renders into, and every frame is
    /// read from.
    target: Handle<Image>,
    /// The world camera, so the window can be handed back to it when the run's
    /// last frame is written.
    camera: Entity,
    /// The window-side preview camera and its quad, despawned with the retarget.
    /// The quad is **not** a child of the camera: a child at the identity
    /// transform sits exactly *at* the camera, which renders as an empty black
    /// window (and did).
    preview: Option<(Entity, Entity)>,
}

/// The render layer the capture preview lives on: its camera renders this layer
/// and nothing else, and no other camera renders it, so the preview quad is
/// invisible to the world, to the probes and to the capture itself.
///
/// Layer `2` is the one gap in the workspace's assignment — `0` world, `1` HUD,
/// `3` edit gizmos, `4`–`6` reflection probes, `7` water exclusion.
const CAPTURE_PREVIEW_RENDER_LAYER: usize = 2;

/// Point the world camera at an off-screen image of the pinned size, and put a
/// preview of that image in the window.
///
/// Runs once, in `PostStartup`: the viewer spawns its one [`ViewerCamera`] in
/// `Startup`, and the target image needs `Assets<Image>`, so neither can be done
/// while the app is being built. The overlay cameras are routed separately and
/// per frame by [`route_overlay_cameras`], because the gizmo overlay's camera is
/// spawned lazily, long after this.
fn pin_capture_target(
    mut commands: Commands,
    content: Res<CaptureContent>,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    camera: Query<Entity, With<ViewerCamera>>,
) {
    let Ok(camera) = camera.single() else {
        error!("screenshot: no single viewer camera to capture; this run will produce nothing");
        // Fail the run rather than letting it photograph nothing on schedule:
        // `capture_screenshots` writes the status and logs out.
        schedule.failure = Some("no single viewer camera to capture".to_owned());
        return;
    };
    // The window's own surface format: an 8-bit sRGB target, so a captured frame
    // is the transfer the window would have shown. The camera carries `Hdr`, so
    // the scene is still composed in float and tone-mapped into this by
    // `SlTonemap`.
    let target = images.add(Image::new_target_texture(
        content.size.width,
        content.size.height,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands
        .entity(camera)
        .insert(RenderTarget::Image(target.clone().into()));
    let preview = spawn_capture_preview(
        &mut commands,
        &target,
        content.size,
        &mut meshes,
        &mut materials,
    );
    commands.insert_resource(PinnedCapture {
        target,
        camera,
        preview: Some(preview),
    });

    info!(
        "screenshot: capturing {} at {}; the window shows a preview of the captured frame",
        content.describe(),
        content.size
    );
    if !content.size.width.is_multiple_of(4) {
        // The reference viewer's snapshot path pads its width up to a multiple of
        // four (`image_width += (image_width * 3) % 4`, a BMP row-alignment hack
        // that runs whatever the format), so Firestorm's frame would come out
        // wider than ours and the pair could not be diffed. Ours is exact; say so
        // rather than letting the diff step discover it.
        warn!(
            "screenshot: a capture width of {} is not a multiple of 4; Firestorm's snapshot path \
             rounds its own width up, so its frames will not match this run's",
            content.size.width
        );
    }
}

/// The window-side preview of the capture: a camera of its own showing the
/// captured image on an unlit quad, letterboxed against black.
///
/// Two things make this a textured quad rather than the obvious `bevy_ui`
/// [`ImageNode`]. The frame's **alpha is not opacity** — it carries the glow
/// mask (and the HDR brightness the PNG write drops), so a UI image node, which
/// alpha-blends, showed the world as black with only the glowing prims visible.
/// An unlit [`AlphaMode::Opaque`] material ignores the texture's alpha entirely,
/// which is the same thing the PNG write does. And a UI preview would be drawn
/// by the default UI camera — the very camera a UI capture routes into the
/// captured image, which would put the preview inside the frame it previews.
///
/// The camera sees [`CAPTURE_PREVIEW_RENDER_LAYER`] alone, so it renders no
/// world geometry and (being on no layer the sun is on) builds no shadow
/// cascades; the quad is invisible to every other camera.
fn spawn_capture_preview(
    commands: &mut Commands,
    target: &Handle<Image>,
    size: CaptureSize,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> (Entity, Entity) {
    let aspect = size.aspect();
    let layers = RenderLayers::layer(CAPTURE_PREVIEW_RENDER_LAYER);
    let camera = commands
        .spawn((
            Name::new("capture-preview-camera"),
            Camera3d::default(),
            Camera {
                // Before every camera of the capture (which draw into the image,
                // not the window); it is the only thing that clears the window.
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            // The captured frame, fitted: a view at least `aspect` wide and one
            // unit high shows the whole quad and letterboxes whatever the
            // window's own aspect leaves over.
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::AutoMin {
                    min_width: aspect,
                    min_height: 1.0,
                },
                ..OrthographicProjection::default_3d()
            }),
            // Back from the quad, looking at it. Far enough that the quad is
            // never inside the near plane, and pointedly **not** the quad's
            // parent: parenting it and leaving its transform at the identity
            // puts the quad exactly at the camera, which renders as a plain
            // black window and reads as "the viewer draws nothing".
            Transform::from_xyz(0.0, 0.0, PREVIEW_CAMERA_DISTANCE).looking_at(Vec3::ZERO, Vec3::Y),
            // The frame is already tone-mapped sRGB; tone-mapping it again would
            // make the preview disagree with the file on disk.
            Tonemapping::None,
            // **Load-bearing**, and the same rule the HUD camera states: a camera
            // sharing the window must match the others' sample count and
            // HDR-ness. Bevy keys a view's main texture on those, so a camera
            // that differs gets a *separate* one — and the later camera's blit to
            // the swapchain then overwrites the earlier camera's. With the HUD /
            // UI camera (`Msaa::Sample4`, `Hdr`, no clear) drawing after this
            // one, an `Msaa::Off` non-HDR preview showed as a black window with
            // the UI over it, which is exactly what a broken renderer looks like.
            Msaa::Sample4,
            Hdr,
            layers.clone(),
        ))
        .id();
    let quad = commands
        .spawn((
            Name::new("capture-preview-quad"),
            Mesh3d(meshes.add(Rectangle::new(aspect, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(target.clone()),
                unlit: true,
                // Load-bearing: the frame's alpha is the glow mask, not opacity.
                alpha_mode: AlphaMode::Opaque,
                ..default()
            })),
            Transform::default(),
            layers,
        ))
        .id();
    (camera, quad)
}

/// How far the preview camera stands back from the quad. Any distance inside the
/// orthographic projection's depth range will do; this one is comfortably clear
/// of the near plane.
const PREVIEW_CAMERA_DISTANCE: f32 = 5.0;

/// Every root `bevy_ui` node: the trees a capture that excludes the UI hides.
///
/// Roots rather than the whole tree because `Visibility` is inherited, and by
/// query rather than by name because the viewer's UI is several roots (the
/// scaffold's, the chat overlay's, a demo panel's) and a floater can add one at
/// any time.
type UiRootQuery<'world, 'state> = Query<
    'world,
    'state,
    (Entity, &'static mut Visibility),
    (With<Node>, Without<ChildOf>, Without<crate::hud::HudScreen>),
>;

/// Route each overlay camera into the captured frame or leave it on the window,
/// and hide whatever an asymmetric request excludes.
///
/// Per frame rather than once, because the gizmo overlay's camera is spawned
/// lazily (when the edit tools first need it), and a UI root can be spawned at
/// any time by a floater opening.
///
/// The rule for a camera is *route it iff anything it draws was asked for*. The
/// HUD camera draws two things — the HUD layer and, as `bevy_ui`'s default
/// camera, the UI — so when only one of them is wanted the other's content is
/// hidden. With neither wanted the camera keeps the window, which is why an
/// ordinary world-only run still shows its UI live over the preview.
fn route_overlay_cameras(
    mut commands: Commands,
    pinned: Option<Res<PinnedCapture>>,
    content: Res<CaptureContent>,
    overlays: Query<(Entity, &OverlayCamera, Option<&RenderTarget>)>,
    mut hud_screens: Query<&mut Visibility, With<crate::hud::HudScreen>>,
    mut ui_roots: UiRootQuery,
) {
    let Some(pinned) = pinned else {
        return;
    };
    for (entity, layer, current) in &overlays {
        // Only ever *add* the routing: handing a camera back to the window is the
        // end-of-run unpin, which drops this resource first.
        if content.draws_wanted_layer(*layer) && !targets_image(current, &pinned.target) {
            commands
                .entity(entity)
                .insert(RenderTarget::Image(pinned.target.clone().into()));
        }
    }
    // The HUD layer and the UI share a camera, so an asymmetric request hides the
    // half it did not ask for. Nothing is hidden when neither was asked for: that
    // camera still has the window, and a run is easier to watch with its UI on.
    if content.splits_hud_camera() {
        if !content.hud {
            for mut visibility in &mut hud_screens {
                set_hidden(&mut visibility);
            }
        }
        if !content.ui {
            for (_entity, mut visibility) in &mut ui_roots {
                set_hidden(&mut visibility);
            }
        }
    }
}

/// Hide a subtree without touching an already-hidden one, so the harness does not
/// mark a `Visibility` changed on every frame of a run.
fn set_hidden(visibility: &mut Mut<'_, Visibility>) {
    if **visibility != Visibility::Hidden {
        **visibility = Visibility::Hidden;
    }
}

/// Whether a camera's current render target is already `target`.
fn targets_image(current: Option<&RenderTarget>, target: &Handle<Image>) -> bool {
    matches!(current, Some(RenderTarget::Image(image)) if image.handle == *target)
}

/// Hand the window back to the world camera and drop the preview, once the run's
/// last frame has been captured **and** written.
///
/// So the logout the harness ends with — and its grace period, which is seconds
/// of live window — shows the world rather than a preview of a target nothing is
/// capturing any more. The overlay cameras are handed back with it; the content
/// an asymmetric request hid stays hidden, since the run is over.
///
/// Idempotent: the schedule's "everything is written" branch is reached on every
/// frame of the logout, and re-inserting the render target each time would churn
/// the camera's view resources for no reason.
fn unpin_capture_target(
    commands: &mut Commands,
    pinned: &mut PinnedCapture,
    overlays: &Query<(Entity, &OverlayCamera, Option<&RenderTarget>)>,
) {
    let Some((preview_camera, preview_quad)) = pinned.preview.take() else {
        return;
    };
    commands
        .entity(pinned.camera)
        .insert(RenderTarget::Window(WindowRef::Primary));
    for (entity, _layer, current) in overlays {
        if targets_image(current, &pinned.target) {
            commands
                .entity(entity)
                .insert(RenderTarget::Window(WindowRef::Primary));
        }
    }
    commands.entity(preview_camera).despawn();
    commands.entity(preview_quad).despawn();
}

/// A pending off-thread screenshot write, spawned by [`capture_screenshots`] and
/// drained by [`poll_screenshot_saves`]. The task yields the written path on
/// success, or a formatted error string, so a failed write surfaces in the log
/// rather than being swallowed.
#[derive(Debug, Component)]
pub(crate) struct ScreenshotSaveTask(Task<Result<PathBuf, String>>);

/// Capture a frame to `frame_NNN.png` on the schedule, then request a clean grid
/// logout once the last frame is taken **and** its write has finished.
///
/// The frame is read from the pinned off-screen target every camera of the
/// capture renders into ([`PinnedCapture`]), never from the window, so its size
/// is the one that was asked for and its content is the layers that were asked
/// for — see [the module docs](self).
///
/// The PNG encode + disk write is offloaded to [`IoTaskPool`]; the logout is held
/// until every pending [`ScreenshotSaveTask`] has drained so a race between the
/// last frame's write and process exit can't truncate the final PNG(s).
///
/// The logout (rather than an immediate `AppExit`) is what lets the run leave the
/// avatar cleanly logged out: an abrupt process exit strands the grid session, and
/// the next login is then rejected until the grid times the stale presence out. The
/// actual exit is driven by the session systems (on `LoggedOut`, or the quit-deadline
/// fallback), the same as a Menu ▸ Quit / `Ctrl+Q` request.
#[expect(
    clippy::too_many_arguments,
    reason = "one system owns the whole schedule: when to arm (time, quiescence, schedule), what \
              to capture (the pinned target and the cameras drawing into it), and what to do when \
              the last frame has drained (session, commands)"
)]
pub(crate) fn capture_screenshots(
    time: Res<Time>,
    quiescence: SceneQuiescence,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut commands: Commands,
    mut session: ResMut<ViewerSession>,
    mut sl_commands: MessageWriter<SlCommand>,
    pending_saves: Query<(), With<ScreenshotSaveTask>>,
    mut pinned: Option<ResMut<PinnedCapture>>,
    mut scene_dump: Option<ResMut<crate::scene_dump::SceneDumpRequest>>,
    overlays: Query<(Entity, &OverlayCamera, Option<&RenderTarget>)>,
) {
    let now = time.elapsed_secs();
    // A run that cannot capture is over: say so in the status file and log out,
    // rather than filling the directory with frames of whatever is on screen.
    if schedule.failure.is_some() {
        // Hold the logout until every write in flight has drained, as the normal
        // end of a run does: an abrupt exit truncates the last PNG, and a
        // truncated frame is a rendering bug that never happened.
        if pending_saves.is_empty() {
            schedule.write_status();
            request_logout(&mut session, &mut sl_commands, now);
        }
        return;
    }
    if schedule.next_at.is_none() {
        // The first capture waits for the scene to settle: region up for a
        // while, and quiet for a run of frames. The configured delay is the
        // timeout that lets a permanently-busy scene still produce a frame.
        if quiescence.region_is_up() && schedule.region_seen_at.is_none() {
            schedule.region_seen_at = Some(now);
        }
        // Nothing was ever in front of the camera, so there is nothing to
        // photograph and no point starting the schedule: a run that captures
        // here writes a full set of empty frames and reports them as a
        // successful comparison. Wait for the login timeout and fail the run.
        //
        // Only for a run that logs in. A `--replay` run has no grid and never
        // will, and its frames — a bundle's avatar, rebuilt offline — are the
        // whole point of it.
        if schedule.grid_expected && schedule.region_seen_at.is_none() {
            if now < schedule.login_timeout {
                return;
            }
            let reason = format!(
                "login not completed within {:.0} s; nothing was captured",
                schedule.login_timeout
            );
            error!("screenshot: {reason}");
            schedule.failure = Some(reason);
            schedule.write_status();
            request_logout(&mut session, &mut sl_commands, now);
            return;
        }
        schedule.quiet_frames = if quiescence.is_quiet() {
            schedule.quiet_frames.saturating_add(1)
        } else {
            0
        };
        let settled = schedule
            .region_seen_at
            .is_some_and(|at| now - at >= MIN_SETTLE_SECS)
            && schedule.quiet_frames >= QUIET_HOLD_FRAMES;
        if !settled {
            if now < schedule.start_delay {
                return;
            }
            info!(
                "screenshot: the scene did not go quiet within {:.0} s ({} fetch(es) \
                 outstanding); capturing anyway",
                schedule.start_delay,
                quiescence.outstanding()
            );
        }
        schedule.settled = settled;
        schedule.next_at = Some(now);
    }
    let next_at = schedule.next_at.unwrap_or(now);
    if now < next_at {
        return;
    }
    if schedule.index >= schedule.max_frames {
        // Don't log out (and so quit) while a frame's PNG is still being written
        // off-thread — dropping the task at exit would truncate the file.
        if !pending_saves.is_empty() {
            return;
        }
        info!(
            "screenshot: captured {} frames; logging out",
            schedule.index
        );
        // Before the logout, not after it: a harness reads this file to tell a
        // run that happened from one that did not, and a viewer that dies during
        // its logout must leave the run looking like what it was.
        schedule.write_status();
        // And the scene the last frame was taken from, if this run writes one.
        // The dump itself is written at the end of this frame (`Last`), where
        // the transforms it reads back are the ones the frame was rendered with.
        if let Some(dump) = scene_dump.as_mut() {
            dump.request();
        }
        // Every frame is written: give the window back to the world camera, so the
        // logout's grace period is watchable.
        if let Some(pinned) = pinned.as_mut() {
            unpin_capture_target(&mut commands, pinned, &overlays);
        }
        request_logout(&mut session, &mut sl_commands, now);
        return;
    }
    let path = schedule
        .dir
        .join(format!("frame_{:03}.png", schedule.index));
    info!("screenshot: capturing {}", path.display());
    // The cameras of the capture already point at the pinned target every frame,
    // so capturing that image is capturing this frame at the size and with the
    // layers the run asked for.
    let Some(pinned) = pinned else {
        // `pin_capture_target` said why; do not silently capture something else.
        return;
    };
    commands
        .spawn(Screenshot::image(pinned.target.clone()))
        .observe(save_off_thread(path));
    schedule.index = schedule.index.saturating_add(1);
    schedule.next_at = Some(now + schedule.interval);
}

/// Build the [`ScreenshotCaptured`] observer that writes one captured frame to
/// `path` off the main thread.
///
/// The frame is decoded to an opaque RGB image on the frame thread (dropping the
/// HDR alpha, which carries brightness — the same as Bevy's `save_to_disk`), then
/// the heavy PNG deflate + disk write is handed to [`IoTaskPool`] via a
/// [`ScreenshotSaveTask`] that [`poll_screenshot_saves`] drains.
fn save_off_thread(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>, Commands) {
    move |captured, mut commands| {
        let capture_entity = captured.entity;
        let dynamic = match captured.image.clone().try_into_dynamic() {
            // Discard the alpha channel (HDR brightness) so the PNG looks right.
            Ok(dynamic) => image::DynamicImage::ImageRgb8(dynamic.to_rgb8()),
            Err(error) => {
                error!("screenshot: cannot decode capture: {error}");
                commands.entity(capture_entity).despawn();
                return;
            }
        };
        let path = path.clone();
        let task = IoTaskPool::get().spawn(async move {
            let format = image::ImageFormat::from_path(&path).map_err(|error| error.to_string())?;
            dynamic
                .save_with_format(&path, format)
                .map_err(|error| error.to_string())?;
            Ok(path)
        });
        commands.spawn(ScreenshotSaveTask(task));
        // One-shot; drop the capture entity so a save does not leak one.
        commands.entity(capture_entity).despawn();
    }
}

/// Poll the off-thread screenshot writes; when one finishes, log the saved path
/// (or the write error), then drop the task entity. Runs every frame; a write in
/// flight costs one cheap non-blocking poll.
pub(crate) fn poll_screenshot_saves(
    mut commands: Commands,
    mut schedule: ResMut<ScreenshotSchedule>,
    mut tasks: Query<(Entity, &mut ScreenshotSaveTask)>,
) {
    for (entity, mut task) in &mut tasks {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            continue;
        };
        match result {
            Ok(path) => {
                info!("screenshot: saved {}", path.display());
                schedule.written = schedule.written.saturating_add(1);
            }
            Err(error) => {
                error!("screenshot: save failed: {error}");
                // A frame the harness thinks it captured but that never reached
                // the disk is exactly the gap the status file exists to close:
                // the run is short by one frame, and it must say so.
                schedule
                    .failure
                    .get_or_insert_with(|| format!("a frame failed to reach the disk: {error}"));
            }
        }
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::world_api::OverlayCamera;

    use super::{CaptureContent, CaptureSize, MAX_CAPTURE_DIMENSION, parse_capture_size};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// The ordinary form: the 1080p grid both viewers' harnesses are pointed at.
    #[test]
    fn a_pixel_grid_parses() -> Result<(), TestError> {
        assert_eq!(
            parse_capture_size("1920x1080")?,
            CaptureSize {
                width: 1920,
                height: 1080,
            }
        );
        Ok(())
    }

    /// A capital `X` and whitespace around the value are accepted, because a
    /// value that arrives through an environment block gets typed by hand.
    #[test]
    fn the_separator_and_surrounding_space_are_forgiving() -> Result<(), TestError> {
        let expected = CaptureSize {
            width: 1280,
            height: 720,
        };
        assert_eq!(parse_capture_size("1280X720")?, expected);
        assert_eq!(parse_capture_size("  1280 x 720  ")?, expected);
        Ok(())
    }

    /// A size prints back the way it is written, so a log line and a command line
    /// can be compared without translating between them.
    #[test]
    fn a_size_prints_the_way_it_parses() -> Result<(), TestError> {
        assert_eq!(parse_capture_size("800x600")?.to_string(), "800x600");
        Ok(())
    }

    /// The largest dimension every adapter can allocate is accepted; one pixel
    /// more is not — refused at the command line rather than at the first
    /// capture, a run in.
    #[test]
    fn the_dimension_ceiling_is_the_last_accepted_value() -> Result<(), TestError> {
        let at_ceiling = format!("{MAX_CAPTURE_DIMENSION}x{MAX_CAPTURE_DIMENSION}");
        assert_eq!(
            parse_capture_size(&at_ceiling)?,
            CaptureSize {
                width: MAX_CAPTURE_DIMENSION,
                height: MAX_CAPTURE_DIMENSION,
            }
        );
        let over = format!("{}x1080", MAX_CAPTURE_DIMENSION.saturating_add(1));
        assert!(
            parse_capture_size(&over).is_err(),
            "`{over}` is past the ceiling and should have been refused"
        );
        Ok(())
    }

    /// Every malformed value is an error rather than a fallback to a default.
    ///
    /// A silent fallback is the failure this whole flag exists to prevent: it
    /// produces a full run of frames that are the wrong size, whose only symptom
    /// is that the diff step later refuses them.
    #[test]
    fn a_malformed_size_is_refused_rather_than_defaulted() {
        for text in [
            "",
            "1920",
            "1920x",
            "x1080",
            "0x1080",
            "1920x0",
            "axb",
            "-1x1080",
            "1920.5x1080",
            "1920x1080x2",
        ] {
            assert!(
                parse_capture_size(text).is_err(),
                "`{text}` should have been refused"
            );
        }
    }

    /// A capture with the given layers, at the default size.
    const fn content(ui: bool, hud: bool, gizmos: bool) -> CaptureContent {
        CaptureContent {
            size: CaptureSize::DEFAULT,
            ui,
            hud,
            gizmos,
        }
    }

    /// The default is the world alone at 1080p — the comparison the cross-check
    /// asks for, and the same default the Firestorm side carries.
    #[test]
    fn the_default_capture_is_the_world_alone_at_1080p() {
        assert_eq!(
            CaptureContent::WORLD_ONLY,
            content(false, false, false),
            "the world-only constant must not drift from its fields"
        );
        assert_eq!(CaptureSize::DEFAULT.to_string(), "1920x1080");
        assert_eq!(CaptureContent::WORLD_ONLY.describe(), "world");
    }

    /// Each layer is an independent switch: asking for one does not route
    /// another's camera into the frame.
    #[test]
    fn each_layer_routes_its_own_camera_only() {
        let gizmos_only = content(false, false, true);
        assert!(gizmos_only.draws_wanted_layer(OverlayCamera::Gizmos));
        assert!(!gizmos_only.draws_wanted_layer(OverlayCamera::HudAndUi));

        let hud_only = content(false, true, false);
        assert!(hud_only.draws_wanted_layer(OverlayCamera::HudAndUi));
        assert!(!hud_only.draws_wanted_layer(OverlayCamera::Gizmos));

        let ui_only = content(true, false, false);
        assert!(ui_only.draws_wanted_layer(OverlayCamera::HudAndUi));
        assert!(!ui_only.draws_wanted_layer(OverlayCamera::Gizmos));
    }

    /// The HUD layer and the UI share one camera, so exactly one of them is the
    /// case that has to hide the other's content — and neither, or both, is not.
    #[test]
    fn only_an_asymmetric_hud_and_ui_request_hides_anything() {
        assert!(content(true, false, false).splits_hud_camera());
        assert!(content(false, true, false).splits_hud_camera());
        assert!(!content(false, false, false).splits_hud_camera());
        assert!(!content(true, true, false).splits_hud_camera());
        // The gizmo switch never enters it: its camera routes on its own.
        assert!(!content(true, true, true).splits_hud_camera());
    }

    /// A run says in its first line which layers it is capturing, so a run that
    /// captured the wrong thing is caught before its frames are read.
    #[test]
    fn the_opening_line_names_every_captured_layer() {
        assert_eq!(content(false, false, true).describe(), "world + gizmos");
        assert_eq!(content(false, true, false).describe(), "world + HUD");
        assert_eq!(content(true, false, false).describe(), "world + UI");
        assert_eq!(
            content(true, true, true).describe(),
            "world + gizmos + HUD + UI"
        );
    }

    /// The preview quad is fitted with the *frame's* aspect, so a 16:9 capture
    /// previewed in a 16:10 window is letterboxed rather than stretched.
    #[test]
    fn the_aspect_is_the_pinned_frames_own() -> Result<(), TestError> {
        let aspect = parse_capture_size("1920x1080")?.aspect();
        assert!(
            (aspect - 16.0 / 9.0).abs() < 1e-6,
            "1920x1080 is 16:9, got {aspect}"
        );
        Ok(())
    }
}
