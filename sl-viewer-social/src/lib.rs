//! Who the agent knows, blocks and belongs to, and whether it is here.
//!
//! The mute list, the buddy list, the group memberships, the away / Do Not
//! Disturb state and what the map surfaces are pointing at. `sl-viewer-people`
//! and the session runtime fill these; the name tags, the radar, the chat
//! surfaces, the minimap and every menu that offers to block, friend or IM
//! someone read them.
//!
//! They live in their own crate because *everything* reads them and nothing
//! about them is world state: a name tag asking whether a resident is muted
//! should not have to reach through the world layer to find out, and the world
//! layer should not have to carry a buddy list to let it.
//!
//! Nothing here reaches back into a feature, or into the world: the crate
//! depends on `bevy` and `sl-client-bevy` and on nothing else in the viewer.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one model and is named for it, so the types read \
              as `mute::MuteModel` and `friends::FriendsModel`. Those are the \
              names the whole viewer already calls them by; renaming them to \
              satisfy a style rule this codebase does not follow would churn \
              every call site"
)]

pub mod friends;
pub mod groups;
pub mod map_tracking;
pub mod mute;
pub mod presence;

pub use friends::*;
pub use groups::*;
pub use map_tracking::*;
pub use mute::*;
pub use presence::*;

use sl_client_bevy::Uuid;

/// A short, readable stand-in for an unresolved agent id — its first eight hex
/// digits (mirrors `conversations`'s placeholder).
#[must_use]
pub fn short_id(id: Uuid) -> String {
    id.simple().to_string().chars().take(8).collect()
}
