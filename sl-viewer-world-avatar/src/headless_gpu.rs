//! The headless render stack this crate's GPU tests run on.
//!
//! One definition, because the setting that matters here is one no copy would
//! think to add: **synchronous pipeline compilation**. Bevy compiles pipelines
//! on the global async-compute pool by default, and that pool outlives the
//! [`App`](bevy::app::App) — dropping the app joins the render thread but never
//! the pool. A test that stops as soon as its readback answers can return while
//! a compile is still running on a pool thread, and the process then exits
//! underneath it: in a debug build the Vulkan validation layer is inside that
//! compile, reading C++ statics that `exit` is destroying, and the test binary
//! segfaults *after* reporting `ok` (seen roughly one run in five on
//! `the_gpu_holds_a_stopped_motions_pose`). Compiling on the render thread
//! instead finishes every pipeline inside the `update` that asked for it, so
//! nothing is left running when the test returns.

use bevy::app::PluginGroupBuilder;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::winit::WinitPlugin;

/// [`DefaultPlugins`] for a windowless GPU test: no window and no exit for the
/// lack of one, no event loop (the test drives `update` itself), no log plugin
/// (the test harness owns the subscriber), and pipelines compiled synchronously
/// so no compile outlives the test — see the module documentation.
pub(crate) fn headless_gpu_plugins() -> PluginGroupBuilder {
    DefaultPlugins
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .disable::<WinitPlugin>()
        .disable::<LogPlugin>()
}
