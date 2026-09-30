//! Whether this machine can render at all.
//!
//! Every stage viewer renders — headless is an off-screen window, not no
//! window — so a machine with no GPU adapter cannot run one, whichever
//! backend. The stage asks once, before it starts a grid, and a test with no
//! adapter skips loudly instead of failing on a symptom (a viewer process that
//! dies at start-up, an App with no render device).

/// Whether wgpu finds an adapter, with the backends and flags the
/// environment selects — the ones Bevy's renderer will ask for.
#[must_use]
pub(crate) fn adapter_available() -> bool {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).is_ok()
}
