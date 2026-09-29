//! [`PipelineStatus`]: how many render pipelines are still queued or
//! compiling — the render half of "has the viewer settled".
//!
//! A frame rendered while a pipeline is still compiling simply omits whatever
//! that pipeline draws, so a screenshot, a pixel assertion or a quiet wait must
//! wait for none to be left.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::{Render, RenderApp, RenderSystems};

/// How many pipelines the render world still has queued or compiling, mirrored
/// into the main world every frame.
///
/// Shared through an atomic rather than extracted, because extraction copies
/// main → render and this travels the other way.
#[derive(Resource, Debug, Clone, Default)]
pub struct PipelineStatus(Arc<AtomicU32>);

impl PipelineStatus {
    /// Pipelines queued or compiling as of the last render.
    #[must_use]
    pub fn waiting(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Publishes [`PipelineStatus`]: the same cell in both worlds, written from the
/// render world's cleanup set each frame. An app with no renderer gets the
/// resource and nothing writes it.
#[derive(Debug, Default)]
pub struct PipelineStatusPlugin;

impl Plugin for PipelineStatusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PipelineStatus>();
    }

    fn finish(&self, app: &mut App) {
        let status = app.world().resource::<PipelineStatus>().clone();
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(status).add_systems(
                Render,
                publish_pipeline_status.in_set(RenderSystems::Cleanup),
            );
        }
    }
}

/// Render-world system: count the pipelines not yet ready into
/// [`PipelineStatus`].
fn publish_pipeline_status(cache: Res<PipelineCache>, status: Res<PipelineStatus>) {
    let waiting = u32::try_from(cache.waiting_pipelines().count()).unwrap_or(u32::MAX);
    status.0.store(waiting, Ordering::Relaxed);
}
