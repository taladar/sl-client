//! Render the 3D world at `1/n` of the window's resolution and stretch it back
//! over the view — the reference viewer's `RenderResolutionDivisor`.
//!
//! **Who wants it, for opposite reasons.** It is the bluntest quality lever
//! there is, and the one a user reaches for when a region is too heavy for their
//! machine; and it is the oldest and cheapest blur an RLV collar can impose
//! (`@setdebug_renderresolutiondivisor:<n>=force`), which is why the setting's
//! name lives a layer down in [`sl_viewer_world_api::SETTING_RENDER_RESOLUTION_DIVISOR`]
//! where the RLV surface can reach it without depending on this crate.
//!
//! **The world only.** The reference scales its *world view*
//! (`getWorldViewWidthRaw`), never its interface, and neither does this: the
//! point is a coarser world, not a coarser set of menus. So the world camera —
//! and only the world camera — is pointed at an image `1/n` the size of the
//! window, and one of the overlay cameras that draw the composited frame
//! ([`OverlayCamera`], the gizmo layer and the HUD / UI layer) stretches that
//! image over the window before it draws its own content at full size.
//!
//! # The scale factor is what keeps every other system honest
//!
//! A reduced render target is a trap for everything that maps between the screen
//! and the world: the cursor pick, the edit gizmos' drag rays, the beacon
//! arrows, the name-tag projection. All of them go through
//! [`Camera::viewport_to_world`] / [`Camera::world_to_viewport`], which work in
//! **logical** pixels — and Bevy derives a target's logical size as
//! `physical_size / scale_factor` for an image target exactly as it does for a
//! window.
//!
//! So the image is given a scale factor of the window's *divided by the
//! divisor*. Its physical size shrinks, its logical size does not, and every one
//! of those call sites keeps taking and returning window-logical coordinates
//! with no knowledge that anything changed. Without that, each would have needed
//! its own `/ n`, and the one that was missed would have been a pick that lands
//! somewhere the cursor is not.
//!
//! # The upscale is a pass, not a sprite
//!
//! The overlay cameras carry [`ClearColorConfig::None`] because they are drawn
//! *onto the world camera's frame* in the window's shared main texture. Divert
//! the world camera and that texture holds whatever was last in it, so something
//! has to put the reduced frame there first. That is
//! `upscale_world_system`: a fullscreen pass, run on the lowest-ordered
//! overlay camera before its own main pass, writing through
//! [`ViewTarget::get_color_attachment`] — the multisampled attachment the main
//! pass then loads, as [`crate::underwater_fog`] does, rather than a resolved
//! copy the next resolve would throw away. A sprite or a UI node could not do
//! it: one would blend by the frame's alpha (which here is the glow mask, not
//! coverage) and the other would draw *over* the HUD attachments instead of
//! under them.
//!
//! Reference (Firestorm, read-only): `pipeline.cpp`
//! (`LLPipeline::resizeScreenTexture` / `refreshCachedSettings`, and FIRE-7066's
//! clamp), `llviewercontrol.cpp` (`handleRenderResolutionDivisorChanged`),
//! `rlvextensions.cpp` (the `@setdebug` row).

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::Core3d;
use bevy::ecs::query::QueryItem;
use bevy::ecs::system::lifetimeless::Read;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
    CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FilterMode, FragmentState,
    MultisampleState, PipelineCache, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, SpecializedRenderPipeline,
    SpecializedRenderPipelines, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{GpuResourceAppExt as _, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::window::{PrimaryWindow, WindowRef};

use sl_settings::SettingValue;
use sl_viewer_settings::ViewerSettings;
use sl_viewer_world_api::{OverlayCamera, SETTING_RENDER_RESOLUTION_DIVISOR, ViewerCamera};

/// The internal handle the upscale shader (`resolution_divisor.wgsl`) is loaded
/// under.
const UPSCALE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("2d3b5a17-a7f8-4f1d-9fc5-9c71f2aea3e9");

/// The persisted-file section the divisor is grouped under (`[render]`), the
/// same one the rest of the reference's `Render*` family lives in.
const RENDER_SECTION: &[&str] = &["render"];

/// The reference default: render the world at the window's own resolution, which
/// costs nothing and is the only value at which none of this module runs.
pub const DEFAULT_RESOLUTION_DIVISOR: u32 = 1;

/// The largest divisor this viewer will honour, and the last rung of the
/// graphics tab's ladder.
///
/// The reference has no maximum — it merely refuses a divisor that would leave
/// fewer than one pixel (FIRE-7066's `res_mod >= resX` clamp, reproduced in
/// [`effective_divisor`]) — but a 1/32 render of a 4K window is 120×67 and
/// serves nobody, so this stops somewhere a user can still recognise the world.
/// A script may ask for more and gets this.
pub const MAX_RESOLUTION_DIVISOR: u32 = 16;

/// Register the divisor with the rest of the render family, so the name exists
/// and persists (a user's Firestorm `RenderResolutionDivisor` ports straight
/// over), the graphics tab has something to bind to, and the RLV allowlist's one
/// writable row has something to write. Called from [`ViewerSettings`]'s
/// registrar list.
pub fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        RENDER_SECTION,
        SETTING_RENDER_RESOLUTION_DIVISOR,
        SettingValue::U32(DEFAULT_RESOLUTION_DIVISOR),
        "Divisor for rendering the 3D scene at reduced resolution (1 = full)",
    );
}

/// The divisor actually applied to a target of `size` physical pixels.
///
/// Three rules, two of them the reference's:
///
/// - anything at or below `1` is off (`res_mod > 1`);
/// - a divisor at least as large as the smaller side is held below it, so the
///   reduced target keeps at least one pixel per axis — Firestorm's FIRE-7066
///   fix, which is there because the unclamped version divided a dimension to
///   zero and broke the render outright;
/// - and this viewer's own ceiling, [`MAX_RESOLUTION_DIVISOR`].
#[must_use]
pub fn effective_divisor(stored: u32, size: UVec2) -> u32 {
    let smallest = size.x.min(size.y);
    // A target with no room to divide at all: one pixel each way is already the
    // floor, so there is nothing to take away.
    if smallest < 2 {
        return DEFAULT_RESOLUTION_DIVISOR;
    }
    let capped = stored
        .min(MAX_RESOLUTION_DIVISOR)
        .min(smallest.saturating_sub(1));
    if capped <= DEFAULT_RESOLUTION_DIVISOR {
        DEFAULT_RESOLUTION_DIVISOR
    } else {
        capped
    }
}

/// The physical size of the world's render target at `divisor`, never smaller
/// than one pixel a side (a zero-sized texture is a wgpu validation error, and
/// [`effective_divisor`] is what normally keeps us away from it).
#[must_use]
pub fn world_target_size(size: UVec2, divisor: u32) -> UVec2 {
    // `checked_div` rather than `/` to satisfy the workspace's
    // `arithmetic_side_effects` lint; the `max(1)` above already rules out the
    // divide by zero, and a `None` would fall back to the undivided side.
    let divisor = divisor.max(1);
    let side = |side: u32| side.checked_div(divisor).unwrap_or(side).max(1);
    UVec2::new(side(size.x), side(size.y))
}

/// The scale factor the reduced target is given: the window's, divided by the
/// divisor, so the target's **logical** size stays the window's own and every
/// `viewport_to_world` / `world_to_viewport` call site keeps working in window
/// pixels (see the module docs).
#[must_use]
pub fn world_target_scale_factor(window_scale_factor: f32, divisor: u32) -> f32 {
    window_scale_factor / u32_to_f32(divisor.max(1))
}

/// Widen a divisor to the float a scale factor is expressed in.
#[expect(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "a divisor is at most MAX_RESOLUTION_DIVISOR, exactly representable"
)]
const fn u32_to_f32(value: u32) -> f32 {
    value as f32
}

/// The reduced-resolution image the world camera renders into while the divisor
/// is above one.
///
/// Created on first use and then resized in place, because the handle is what
/// the world camera's [`RenderTarget`] and the overlay camera's
/// [`WorldUpscale`] both name: allocating a new image per window resize would
/// leave both pointing at the previous one for a frame.
#[derive(Resource, Debug, Default)]
pub struct WorldRenderTarget {
    /// The image, once there has been a reason to make one. `None` while the
    /// divisor has never been above one — the overwhelmingly common case, in
    /// which this module allocates nothing at all.
    image: Option<Handle<Image>>,
    /// The window the world camera was pointed at before it was diverted, so
    /// turning the divisor back off restores *that* window rather than assuming
    /// the primary one. `None` until the first divert.
    window: Option<WindowRef>,
}

impl WorldRenderTarget {
    /// The reduced-resolution world image, if one has been made.
    #[must_use]
    pub const fn image(&self) -> Option<&Handle<Image>> {
        self.image.as_ref()
    }
}

/// Marks the overlay camera that must stretch the reduced world frame over its
/// view before drawing its own content, and names the image to stretch.
///
/// Present only while the divisor is above one, on exactly one camera: the
/// lowest-ordered [`OverlayCamera`] still pointed at the window, which is the
/// first thing to draw after the world and therefore the last moment the window
/// texture can be filled before anything is composited onto it.
#[derive(Component, Debug, Clone)]
pub struct WorldUpscale {
    /// The reduced-resolution world frame to stretch over this view.
    pub image: Handle<Image>,
}

impl SyncComponent for WorldUpscale {
    type Target = Self;
}

impl ExtractComponent for WorldUpscale {
    type QueryData = Read<Self>;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(item: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(item.clone())
    }
}

/// Whether two render targets name the same place.
///
/// [`RenderTarget`] has no `PartialEq` of its own, and the comparison is here so
/// the drive system can leave a target it did not set alone and can skip a write
/// that would change nothing (writing through `&mut RenderTarget` every frame
/// would re-extract the camera's whole target for no reason).
fn same_target(left: &RenderTarget, right: &RenderTarget) -> bool {
    match (left, right) {
        // `WindowRef` has no `PartialEq` either, and its two variants are
        // "the primary window" and a named entity.
        (RenderTarget::Window(WindowRef::Primary), RenderTarget::Window(WindowRef::Primary)) => {
            true
        }
        (
            RenderTarget::Window(WindowRef::Entity(left)),
            RenderTarget::Window(WindowRef::Entity(right)),
        ) => left == right,
        (RenderTarget::Image(left), RenderTarget::Image(right)) => left == right,
        (RenderTarget::TextureView(left), RenderTarget::TextureView(right)) => left == right,
        (RenderTarget::None { size: left }, RenderTarget::None { size: right }) => left == right,
        _ => false,
    }
}

/// Whether the world camera's target is one this module is entitled to move.
///
/// The window is (that is where the world camera lives in the viewer), and so is
/// the reduced image this module made. Anything else is a harness that pointed
/// the world camera at a target of its own — the readback tier and the
/// full-stack tier both spawn the viewer's camera bundle straight onto an image
/// — and moving it would quietly break the capture.
fn is_ours(target: &RenderTarget, ours: Option<&Handle<Image>>) -> bool {
    match target {
        RenderTarget::Window(_) => true,
        RenderTarget::Image(image) => ours.is_some_and(|ours| &image.handle == ours),
        RenderTarget::TextureView(_) | RenderTarget::None { .. } => false,
    }
}

/// The world camera's render target and the projection a target swap has to
/// mark changed — the two halves [`drive_resolution_divisor`] writes.
///
/// A named type because the query is a mouthful inline, and because the
/// `Without<OverlayCamera>` is what keeps it disjoint from
/// [`OverlayCameraTargets`] rather than decoration.
type WorldCameraTarget<'world, 'state> = Query<
    'world,
    'state,
    (&'static mut RenderTarget, Option<&'static mut Projection>),
    (With<ViewerCamera>, Without<OverlayCamera>),
>;

/// Every overlay camera, its order, where it draws and whether it already
/// carries the upscale — what picks the camera that stretches the reduced frame
/// back over the window.
type OverlayCameraTargets<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static Camera,
        &'static RenderTarget,
        Has<WorldUpscale>,
    ),
    (With<OverlayCamera>, Without<ViewerCamera>),
>;

/// The extracted views the upscale pass may run on: a camera carrying a
/// [`WorldUpscale`], with the format and sample count its pipeline is
/// specialized for.
type UpscaleViews<'world, 'state> = Query<
    'world,
    'state,
    (Entity, &'static ExtractedView, &'static Msaa),
    (With<ExtractedCamera>, With<WorldUpscale>),
>;

/// Tell Bevy that this camera's target has changed shape, by marking its
/// [`Projection`] changed.
///
/// **Not optional, and not a tidiness measure.** `camera_system` refreshes a
/// camera's `target_info` — the physical size and scale factor every later stage
/// reads — only when the *window or image it names* changed, when the camera is
/// new, when its viewport moved, or when its projection changed. Pointing a
/// camera at a **different** target is in none of those: swap the
/// [`RenderTarget`] and the camera keeps reporting the size of the target it
/// left.
///
/// What that costs is not subtle. `prepare_view_targets` groups cameras by
/// target, format and sample count and sizes the shared main texture from the
/// *first* camera it visits in the group — so a world camera that has moved back
/// to the window while still claiming the reduced image's size can create the
/// **window's** texture at `1/n` size, and then the overlays, the HUD and the
/// whole interface are drawn into it and stretched: a blocky UI, and sub-pixel
/// drift in the UI layout where it is not blocky enough to notice. Which of the
/// group is visited first decides whether it happens at all, which is what makes
/// it intermittent.
///
/// Marking the projection is the recompute trigger in that list which a consumer
/// can reach, and the recompute is wanted anyway: the aspect ratio of the new
/// target is exactly what the projection has to be rebuilt from.
fn announce_target_change(projection: Option<&mut Mut<'_, Projection>>) {
    if let Some(projection) = projection {
        projection.set_changed();
    }
}

/// Point the world camera at a reduced-resolution image (or back at the window),
/// keep that image sized to the window, and mark the overlay camera that has to
/// stretch it back out.
///
/// Everything here is idempotent and guarded: at the default divisor it does
/// nothing but confirm the camera is on the window, which is the state a viewer
/// that never touches the setting stays in for its whole run.
pub(crate) fn drive_resolution_divisor(
    mut commands: Commands,
    settings: Res<ViewerSettings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut target: ResMut<WorldRenderTarget>,
    mut world: WorldCameraTarget,
    overlays: OverlayCameraTargets,
) {
    let (Ok(window), Ok((mut world_target, mut projection))) =
        (windows.single(), world.single_mut())
    else {
        return;
    };
    if !is_ours(&world_target, target.image.as_ref()) {
        return;
    }

    // The overlay that would draw first over the world frame, and so the one
    // that has to lay the world frame down. A harness may have routed an overlay
    // off the window; one that is not on the window cannot fill it.
    let destination = overlays
        .iter()
        .filter(|(_, _, overlay_target, _)| matches!(overlay_target, RenderTarget::Window(_)))
        .min_by_key(|(_, camera, _, _)| camera.order)
        .map(|(entity, _, _, _)| entity);

    let window_size = UVec2::new(
        window.physical_width().max(1),
        window.physical_height().max(1),
    );
    let stored = settings
        .store()
        .get_u32(SETTING_RENDER_RESOLUTION_DIVISOR)
        .unwrap_or(DEFAULT_RESOLUTION_DIVISOR);
    let divisor = if destination.is_some() {
        effective_divisor(stored, window_size)
    } else {
        // Nothing is drawing over the window, so there is nobody to stretch a
        // reduced frame back over it — render at full size rather than into an
        // image no one shows.
        DEFAULT_RESOLUTION_DIVISOR
    };

    // Remember which window the camera is on while it is still on one, so
    // turning the divisor off puts it back where it was rather than assuming the
    // primary window.
    if let RenderTarget::Window(current) = &*world_target {
        target.window = Some(*current);
    }

    // Whether the shape of what the camera renders into moved this frame — a
    // different target, or the same image at a different size. See
    // `announce_target_change` below, which is not optional.
    let mut reshaped = false;

    let wanted = if divisor > DEFAULT_RESOLUTION_DIVISOR {
        let size = world_target_size(window_size, divisor);
        let handle = target
            .image
            .get_or_insert_with(|| {
                // `Rgba16Float`, because the world camera is an `Hdr` one: its
                // main texture is `Rgba16Float` whatever the target is, so any
                // other format would make the camera's own output blit quantise
                // the finished frame on its way into this image and the upscale
                // would stretch the quantised copy.
                images.add(Image::new_target_texture(
                    size.x,
                    size.y,
                    TextureFormat::Rgba16Float,
                    None,
                ))
            })
            .clone();
        if let Some(mut image) = images.get_mut(&handle)
            && (image.width() != size.x || image.height() != size.y)
        {
            image.resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
            reshaped = true;
        }
        RenderTarget::Image(ImageRenderTarget {
            handle,
            scale_factor: world_target_scale_factor(window.resolution.scale_factor(), divisor),
        })
    } else {
        RenderTarget::Window(target.window.unwrap_or(WindowRef::Primary))
    };
    if !same_target(&world_target, &wanted) {
        *world_target = wanted;
        reshaped = true;
    }
    if reshaped {
        announce_target_change(projection.as_mut());
    }

    // Exactly one overlay carries the upscale, and only while the world is
    // somewhere else. The image handle is made once and then resized in place,
    // so "already carries one" is enough to skip the write — which matters,
    // because an unconditional insert would re-extract the component into the
    // render world every frame for the life of the session.
    let upscaling = (divisor > DEFAULT_RESOLUTION_DIVISOR)
        .then(|| target.image.clone())
        .flatten()
        .zip(destination);
    for (entity, _, _, has_upscale) in &overlays {
        match &upscaling {
            Some((image, chosen)) if *chosen == entity => {
                if !has_upscale {
                    commands.entity(entity).insert(WorldUpscale {
                        image: image.clone(),
                    });
                }
            }
            _ => {
                if has_upscale {
                    commands.entity(entity).remove::<WorldUpscale>();
                }
            }
        }
    }
}

/// The plugin: the setting-driven target switch in the main world, and the
/// upscale pass in the render world.
#[derive(Debug, Default)]
pub struct ResolutionDivisorPlugin;

impl Plugin for ResolutionDivisorPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            UPSCALE_SHADER_HANDLE,
            "resolution_divisor.wgsl",
            Shader::from_wgsl
        );
        app.init_resource::<WorldRenderTarget>()
            .add_plugins(ExtractComponentPlugin::<WorldUpscale>::default())
            // In `Update` so the target is settled before Bevy's own
            // `camera_system` recomputes each camera's target info and
            // projection in `PostUpdate` — a divisor change then reaches the
            // very next frame, as the reference's `gResizeScreenTexture` does.
            .add_systems(Update, drive_resolution_divisor);

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<WorldUpscalePipeline>>()
            .add_systems(RenderStartup, init_upscale_pipeline)
            .add_systems(
                Render,
                prepare_upscale_pipelines.in_set(RenderSystems::Prepare),
            )
            .add_systems(
                Core3d,
                // Before anything this camera draws: the overlay's own content
                // belongs *on* the world frame, so the world frame has to be
                // there first.
                upscale_world_system.in_set(Core3dSystems::Prepass),
            );
    }
}

/// The upscale pipeline's global data (bind-group layout descriptor, the
/// magnifying sampler, and the fullscreen vertex shader, which pipeline
/// specialization needs per view format).
#[derive(Resource)]
struct WorldUpscalePipeline {
    /// The bind-group layout descriptor (world texture, sampler), resolved to a
    /// real layout per frame via the pipeline cache.
    layout: BindGroupLayoutDescriptor,
    /// The sampler the reduced frame is magnified through: linear and clamped to
    /// the edge, which is what the reference's own blit of its screen target
    /// uses.
    sampler: Sampler,
    /// The shared fullscreen-triangle vertex shader, needed by pipeline
    /// specialization (which has no world access to fetch it).
    fullscreen_shader: FullscreenShader,
}

/// Build the upscale pipeline's shared data once, in the render world.
fn init_upscale_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "world_upscale_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The finished, reduced-resolution world frame.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // Its magnifying sampler.
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("world_upscale_sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(WorldUpscalePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
    });
}

/// The pipeline key: the view's target format and its MSAA sample count.
///
/// The sample count is here for the same reason it is on the fog pass — this
/// draws **into the main pass's attachment**, so a pipeline whose sample count
/// disagrees with the attachment is a validation error rather than an artifact.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct UpscalePipelineKey {
    /// The colour attachment's format.
    format: TextureFormat,
    /// The colour attachment's MSAA sample count.
    samples: u32,
}

impl SpecializedRenderPipeline for WorldUpscalePipeline {
    type Key = UpscalePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("world_upscale_pipeline".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: UPSCALE_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    // No blending: this pass *establishes* the frame the
                    // overlays draw onto, alpha (the glow mask) included, rather
                    // than mixing into whatever the window texture last held.
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            multisample: MultisampleState {
                count: key.samples,
                ..default()
            },
            ..default()
        }
    }
}

/// The specialized pipeline id for a view.
#[derive(Component)]
struct WorldUpscalePipelineId(CachedRenderPipelineId);

/// Specialize the upscale pipeline for each view's target format and sample
/// count.
fn prepare_upscale_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<WorldUpscalePipeline>>,
    pipeline: Res<WorldUpscalePipeline>,
    views: UpscaleViews,
) {
    for (entity, view, msaa) in &views {
        let key = UpscalePipelineKey {
            format: view.target_format,
            samples: msaa.samples(),
        };
        commands
            .entity(entity)
            .insert(WorldUpscalePipelineId(pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                key,
            )));
    }
}

/// The upscale pass: stretch the reduced world frame over this view, before the
/// view draws anything of its own.
///
/// Runs only on the view carrying a [`WorldUpscale`], which exists only while
/// the divisor is above one — so at the default this system finds no view and
/// the frame is byte-for-byte the one the viewer drew before this module
/// existed.
fn upscale_world_system(
    view: ViewQuery<(&ViewTarget, &WorldUpscale, &WorldUpscalePipelineId)>,
    images: Res<RenderAssets<GpuImage>>,
    pipeline_cache: Res<PipelineCache>,
    pipeline_res: Res<WorldUpscalePipeline>,
    mut ctx: RenderContext,
) {
    /// The debug label the pass, its bind group and its pipeline share.
    const LABEL: &str = "world_upscale";

    let (view_target, upscale, pipeline_id) = view.into_inner();
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    // The frame the world camera rendered. Absent for the frame or two between
    // the image being created and its GPU copy existing; leaving the window
    // alone shows the previous frame, which is what a stale frame looks like
    // anyway.
    let Some(world) = images.get(&upscale.image) else {
        return;
    };

    let bind_group = ctx.render_device().create_bind_group(
        Some(LABEL),
        &pipeline_cache.get_bind_group_layout(&pipeline_res.layout),
        &BindGroupEntries::sequential((&world.texture_view, &pipeline_res.sampler)),
    );

    let pass_descriptor = RenderPassDescriptor {
        label: Some(LABEL),
        // This view's own colour attachment — under MSAA the multisampled
        // texture the main pass then loads, not a resolved copy that the next
        // resolve would discard (see [`crate::underwater_fog`]).
        color_attachments: &[Some(view_target.get_color_attachment())],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };
    let mut render_pass = ctx.begin_tracked_render_pass(pass_descriptor);
    render_pass.set_render_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use bevy::app::{App, TaskPoolPlugin};
    use bevy::asset::{AssetApp as _, AssetPlugin, Assets, Handle};
    use bevy::camera::{Camera, RenderTarget};
    use bevy::ecs::change_detection::DetectChanges as _;
    use bevy::image::Image;
    use bevy::math::UVec2;
    use bevy::prelude::default;
    use bevy::window::{PrimaryWindow, Window, WindowRef, WindowResolution};
    use pretty_assertions::assert_eq;
    use sl_settings::{Scope, SettingValue, SettingsStore};
    use sl_viewer_settings::ViewerSettings;
    use sl_viewer_world_api::{OverlayCamera, SETTING_RENDER_RESOLUTION_DIVISOR, ViewerCamera};

    use super::{
        DEFAULT_RESOLUTION_DIVISOR, MAX_RESOLUTION_DIVISOR, WorldRenderTarget, WorldUpscale,
        drive_resolution_divisor, effective_divisor, register_settings, world_target_scale_factor,
        world_target_size,
    };

    /// A boxed error so a test can use `?` rather than the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The main-world half of this module, standing alone: the settings store
    /// with the divisor registered as the viewer registers it, an image store,
    /// a primary window, the world camera, and the viewer's own two overlay
    /// cameras at their own orders.
    ///
    /// The render half needs a GPU and is not here; what these tests pin is
    /// every decision [`drive_resolution_divisor`] makes, which is where the
    /// feature can go wrong in ways a screenshot would not show — a harness
    /// camera moved out from under its capture, a mark left on the wrong
    /// overlay, an image nobody stretches.
    fn drive_app(window: UVec2, scale: f32) -> App {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()));
        app.init_asset::<Image>();
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        app.insert_resource(settings);
        app.init_resource::<WorldRenderTarget>();
        app.init_resource::<ProjectionAnnounced>();
        app.add_systems(bevy::app::Update, drive_resolution_divisor);
        app.add_systems(bevy::app::Last, record_projection_change);
        app.world_mut().spawn((
            Window {
                resolution: WindowResolution::new(window.x, window.y)
                    .with_scale_factor_override(scale),
                ..default()
            },
            PrimaryWindow,
        ));
        app.world_mut().spawn((
            ViewerCamera,
            Camera::default(),
            // The camera carries a projection because that is what a target
            // swap has to mark changed for Bevy to resize the camera — see
            // `announce_target_change`.
            bevy::camera::Projection::default(),
            window_target(),
        ));
        app.world_mut().spawn((
            OverlayCamera::Gizmos,
            Camera {
                order: 1,
                ..default()
            },
            window_target(),
        ));
        app.world_mut().spawn((
            OverlayCamera::HudAndUi,
            Camera {
                order: 2,
                ..default()
            },
            window_target(),
        ));
        app
    }

    /// The primary window as a render target, which is where every camera in
    /// [`drive_app`] starts.
    fn window_target() -> RenderTarget {
        RenderTarget::Window(WindowRef::Primary)
    }

    /// Whether the world camera's [`Projection`](bevy::camera::Projection) was
    /// marked changed during the frame just run — the announcement Bevy's
    /// `camera_system` needs in order to refresh the camera's target size after
    /// a target swap.
    #[derive(bevy::prelude::Resource, Default)]
    struct ProjectionAnnounced(bool);

    /// Record that announcement, in `Last`, so a test can ask about the frame
    /// that has just run rather than about a component tick it would have to
    /// interpret itself.
    fn record_projection_change(
        mut announced: bevy::prelude::ResMut<ProjectionAnnounced>,
        cameras: bevy::prelude::Query<
            bevy::prelude::Ref<bevy::camera::Projection>,
            bevy::prelude::With<ViewerCamera>,
        >,
    ) {
        announced.0 = cameras.iter().any(|projection| projection.is_changed());
    }

    /// What [`record_projection_change`] saw last frame.
    fn announced(app: &App) -> bool {
        app.world().resource::<ProjectionAnnounced>().0
    }

    /// Put a divisor in the store the way the preferences slider or an RLV
    /// script would, and run one frame.
    fn set_divisor(app: &mut App, divisor: u32) {
        app.world_mut().resource_mut::<ViewerSettings>().set(
            Scope::Global,
            SETTING_RENDER_RESOLUTION_DIVISOR,
            SettingValue::U32(divisor),
        );
        app.update();
    }

    /// The world camera's current target.
    fn world_target(app: &mut App) -> Result<RenderTarget, TestError> {
        Ok(app
            .world_mut()
            .query_filtered::<&RenderTarget, bevy::prelude::With<ViewerCamera>>()
            .single(app.world())?
            .clone())
    }

    /// Which overlay, if any, carries the upscale mark.
    fn marked_overlay(app: &mut App) -> Option<OverlayCamera> {
        app.world_mut()
            .query_filtered::<&OverlayCamera, bevy::prelude::With<WorldUpscale>>()
            .iter(app.world())
            .next()
            .copied()
    }

    /// A 1080p window in physical pixels, the size most of these read against.
    const WINDOW: UVec2 = UVec2::new(1920, 1080);

    /// The registered default is the reference's: a viewer that never touches
    /// the setting renders the world at the window's own resolution.
    #[test]
    fn the_registered_default_is_full_resolution() {
        let mut settings = ViewerSettings::from_store_for_test(SettingsStore::new());
        register_settings(&mut settings);
        assert_eq!(
            settings
                .store()
                .get_u32(SETTING_RENDER_RESOLUTION_DIVISOR)
                .ok(),
            Some(DEFAULT_RESOLUTION_DIVISOR)
        );
        assert_eq!(DEFAULT_RESOLUTION_DIVISOR, 1);
    }

    /// `0` and `1` alike mean "off" — the reference's `res_mod > 1` — so a
    /// script writing zero cannot divide the world away.
    #[test]
    fn nothing_at_or_below_one_divides_anything() {
        for stored in [0, 1] {
            assert_eq!(effective_divisor(stored, WINDOW), 1);
        }
    }

    /// An ordinary divisor passes through and halves both axes.
    #[test]
    fn an_ordinary_divisor_shrinks_both_axes() {
        assert_eq!(effective_divisor(2, WINDOW), 2);
        assert_eq!(world_target_size(WINDOW, 2), UVec2::new(960, 540));
    }

    /// The ceiling holds, so a script asking for 1/1000 gets the largest
    /// divisor this viewer will draw at rather than a single pixel.
    #[test]
    fn a_script_cannot_ask_past_the_ceiling() {
        assert_eq!(effective_divisor(1000, WINDOW), MAX_RESOLUTION_DIVISOR);
    }

    /// Firestorm's FIRE-7066 clamp: a divisor at least as large as the smaller
    /// side is held below it, so neither axis can reach zero — which is what
    /// broke the reference's render before that fix.
    #[test]
    fn a_divisor_larger_than_the_window_is_held_below_it() {
        let narrow = UVec2::new(1920, 5);
        assert_eq!(effective_divisor(MAX_RESOLUTION_DIVISOR, narrow), 4);
        let size = world_target_size(narrow, effective_divisor(MAX_RESOLUTION_DIVISOR, narrow));
        assert!(size.x >= 1 && size.y >= 1, "got {size}");
        // And a target with no room at all simply switches the feature off.
        assert_eq!(effective_divisor(4, UVec2::new(1, 1)), 1);
    }

    /// A reduced target keeps at least one pixel a side even if a caller hands
    /// [`world_target_size`] a divisor [`effective_divisor`] would have
    /// rejected — a zero-sized texture is a wgpu validation error, not an
    /// artifact.
    #[test]
    fn a_reduced_target_never_reaches_zero() {
        assert_eq!(world_target_size(UVec2::new(4, 4), 64), UVec2::new(1, 1));
        assert_eq!(world_target_size(UVec2::new(4, 4), 0), UVec2::new(4, 4));
    }

    /// The whole reason the target carries a scale factor of its own: the
    /// reduced image's **logical** size is the window's, so every
    /// `viewport_to_world` / `world_to_viewport` caller keeps working in window
    /// pixels and none of them needs to know about the divisor.
    #[test]
    fn the_reduced_target_keeps_the_windows_logical_size() {
        for (scale, divisor) in [(1.0_f32, 2_u32), (1.0, 3), (2.0, 2), (1.5, 4)] {
            let physical = world_target_size(WINDOW, divisor);
            let factor = world_target_scale_factor(scale, divisor);
            let logical = physical.as_vec2() / factor;
            let window_logical = WINDOW.as_vec2() / scale;
            assert!(
                (logical - window_logical).abs().max_element() <= 1.0,
                "at scale {scale} divisor {divisor}: {logical} against {window_logical}"
            );
        }
    }

    /// At the default the world camera stays on the window, nothing is marked,
    /// and — the part worth pinning — **no image is allocated**: a viewer that
    /// never touches the setting pays nothing for this module existing.
    #[test]
    fn the_default_leaves_the_frame_exactly_as_it_was() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        app.update();
        assert!(matches!(world_target(&mut app)?, RenderTarget::Window(_)));
        assert_eq!(marked_overlay(&mut app), None);
        assert!(
            app.world()
                .resource::<WorldRenderTarget>()
                .image()
                .is_none(),
            "the default divisor must allocate no render target"
        );
        Ok(())
    }

    /// Above one, the world camera moves to a reduced image carrying the
    /// divided scale factor, and the **lowest-ordered** overlay on the window —
    /// the gizmo layer, which draws first over the world — is the one told to
    /// stretch it back out.
    #[test]
    fn a_divisor_diverts_the_world_and_marks_the_first_overlay() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        set_divisor(&mut app, 2);

        let RenderTarget::Image(target) = world_target(&mut app)? else {
            return Err("the world camera was not diverted to an image".into());
        };
        assert!(
            (target.scale_factor - 0.5).abs() < 1.0e-6,
            "got {}",
            target.scale_factor
        );
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target.handle)
            .ok_or("the reduced target is not in the image store")?;
        assert_eq!(UVec2::new(image.width(), image.height()), WINDOW / 2);
        assert_eq!(marked_overlay(&mut app), Some(OverlayCamera::Gizmos));
        Ok(())
    }

    /// And back: switching the divisor off returns the world camera to the
    /// window it came from and drops the mark, so the overlay stops running a
    /// pass it no longer has a frame for.
    #[test]
    fn turning_it_off_puts_the_world_back() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        set_divisor(&mut app, 4);
        assert!(matches!(world_target(&mut app)?, RenderTarget::Image(_)));

        set_divisor(&mut app, 1);
        assert!(matches!(world_target(&mut app)?, RenderTarget::Window(_)));
        assert_eq!(marked_overlay(&mut app), None);
        Ok(())
    }

    /// The reduced target follows the window's size rather than being
    /// reallocated, because its handle is what both the camera's target and the
    /// overlay's mark name — a new image per resize would leave both pointing
    /// at the old one for a frame.
    #[test]
    fn the_reduced_target_follows_a_resize_in_place() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        set_divisor(&mut app, 2);
        let before: Handle<Image> = app
            .world()
            .resource::<WorldRenderTarget>()
            .image()
            .ok_or("no reduced target")?
            .clone();

        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, bevy::prelude::With<PrimaryWindow>>();
        let mut window = windows.single_mut(app.world_mut())?;
        window.resolution = WindowResolution::new(1280, 720).with_scale_factor_override(1.0);
        app.update();

        let after = app
            .world()
            .resource::<WorldRenderTarget>()
            .image()
            .ok_or("no reduced target")?;
        assert_eq!(&before, after, "the image handle must be stable");
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(after)
            .ok_or("the reduced target is not in the image store")?;
        assert_eq!(
            UVec2::new(image.width(), image.height()),
            UVec2::new(640, 360)
        );
        Ok(())
    }

    /// With no overlay left on the window — a capture harness has routed them
    /// both elsewhere — there is nobody to stretch a reduced frame back out, so
    /// the world is rendered at full size rather than into an image nothing
    /// shows.
    #[test]
    fn with_nothing_drawing_the_window_the_divisor_is_forced_off() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        let elsewhere =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    64,
                    64,
                    super::TextureFormat::Rgba16Float,
                    None,
                ));
        let overlays: Vec<_> = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<OverlayCamera>>()
            .iter(app.world())
            .collect();
        for overlay in overlays {
            app.world_mut()
                .entity_mut(overlay)
                .insert(RenderTarget::Image(elsewhere.clone().into()));
        }
        set_divisor(&mut app, 2);
        assert!(matches!(world_target(&mut app)?, RenderTarget::Window(_)));
        assert_eq!(marked_overlay(&mut app), None);
        Ok(())
    }

    /// Every change of target shape announces itself, and a quiet frame does
    /// not.
    ///
    /// This is the one assertion here with a scar behind it. Bevy refreshes a
    /// camera's target size when the window or image it *names* changes, never
    /// when the camera is pointed somewhere else — so without the announcement
    /// a camera coming back to the window kept claiming the reduced image's
    /// size, and `prepare_view_targets` (which sizes a target's shared main
    /// texture from the first camera it visits) then built the **window's**
    /// texture at `1/n` and stretched the entire frame, interface included.
    /// Observed live on 2026-09-20 as a blocky UI after sliding the divisor
    /// back to 1, and as sub-pixel drift in the UI layout where it was not
    /// blocky enough to see; intermittent because it depends on which camera of
    /// the group is visited first.
    #[test]
    fn a_target_change_announces_itself() {
        let mut app = drive_app(WINDOW, 1.0);
        // Two settling frames: on the first, every component reads as changed
        // because it has just been spawned.
        app.update();
        app.update();
        assert!(
            !announced(&app),
            "a frame that changed nothing announces nothing"
        );

        set_divisor(&mut app, 2);
        assert!(
            announced(&app),
            "diverting the world camera to the reduced image must announce it"
        );
        app.update();
        assert!(!announced(&app), "and then stay quiet while nothing moves");

        set_divisor(&mut app, 1);
        assert!(
            announced(&app),
            "coming back to the window must announce it too — without this the \
             camera keeps the reduced size and the whole frame is drawn at 1/n"
        );
        app.update();
        assert!(!announced(&app));
    }

    /// A resize while the divisor is on reshapes the reduced image, and that is
    /// an announcement too: the `AssetEvent::Modified` Bevy would otherwise
    /// notice arrives after `camera_system` has already run for the frame.
    #[test]
    fn a_resize_under_a_divisor_announces_itself() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        set_divisor(&mut app, 2);
        app.update();
        assert!(!announced(&app));

        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, bevy::prelude::With<PrimaryWindow>>();
        let mut window = windows.single_mut(app.world_mut())?;
        window.resolution = WindowResolution::new(1280, 720).with_scale_factor_override(1.0);
        app.update();
        assert!(
            announced(&app),
            "a reduced target that changed size must announce it"
        );
        Ok(())
    }

    /// A world camera a harness has pointed at a target of its own — the
    /// readback and full-stack tiers both spawn the viewer's camera bundle
    /// straight onto an image — is left exactly where it was, whatever the
    /// setting says.
    #[test]
    fn a_harness_target_is_never_moved() -> Result<(), TestError> {
        let mut app = drive_app(WINDOW, 1.0);
        let capture =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    320,
                    240,
                    super::TextureFormat::Rgba16Float,
                    None,
                ));
        let camera = app
            .world_mut()
            .query_filtered::<bevy::prelude::Entity, bevy::prelude::With<ViewerCamera>>()
            .single(app.world())?;
        app.world_mut()
            .entity_mut(camera)
            .insert(RenderTarget::Image(capture.clone().into()));

        set_divisor(&mut app, 2);
        let RenderTarget::Image(target) = world_target(&mut app)? else {
            return Err("the harness target was replaced outright".into());
        };
        assert_eq!(target.handle, capture);
        assert_eq!(marked_overlay(&mut app), None);
        Ok(())
    }
}
