//! State the world layer reads and a feature owns.
//!
//! The world — rendering, avatars, objects, terrain — has to consult things the
//! features above it maintain: a material preview depends on what the build
//! tool selected, a HUD element on which render layer it sits on, a heightfield
//! read on which region's terrain has streamed in. Left in the feature that
//! owns them, those types make the world depend on what sits on top of it.
//!
//! So the *types* live here and the *systems* stay with their feature:
//! `sl-viewer-edit` still drives [`SelectionSet`], the object ingest path still
//! fills [`ObjectState`], and the world reads either without knowing who wrote
//! it.
//!
//! One module per subject, and the crate root re-exports all of them flat, so
//! `sl_viewer_world_api::ObjectState` keeps working while the file a definition
//! lives in says what it is about.
//!
//! What is **not** here: anything with no world side at all. The mute list, the
//! buddy list, the group memberships and the away / busy modes live in
//! `sl-viewer-social`; the block / friend / profile / picker requests and the
//! drag, drop and open-editor vocabulary live in `sl-viewer-intents`. A
//! feature that only wants to ask another feature for something therefore never
//! reaches through the world layer to do it.
//!
//! Nothing here reaches back into a feature, and that is a property worth
//! keeping: a single upward reference added here would put the whole feature
//! tier back underneath the world.

pub mod edit_selection;
pub mod object_components;
pub mod object_flags;
pub mod object_graph;
pub mod phases;
pub mod rlv;
pub mod schedule_order;
pub mod settings;
pub mod targeted_ray_cast;
pub mod terrain;
pub mod ui_texture;
pub mod world_scoped;
pub mod world_state;
pub mod world_vocabulary;

pub use edit_selection::*;
pub use object_components::*;
pub use object_flags::*;
pub use object_graph::*;
pub use phases::*;
pub use settings::*;
pub use terrain::*;
pub use world_state::*;
pub use world_vocabulary::*;
