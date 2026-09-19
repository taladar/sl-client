//! The parameter bundle the four context menus share.
//!
//! Every menu in this crate opens the same way: it decides which entries its
//! **conditions** enable, from who we are, where we are, what we are doing and
//! who we know. That cluster is the same in all of them, so it is named once
//! here rather than spelled out per system.

use bevy::prelude::*;

use crate::social::FriendsModel;
use crate::world_api::SelfGroundSit;
use sl_client_bevy::{SlAgentParcel, SlIdentity};

/// What a context menu's conditions are drawn from, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): our own agent, the parcel we
/// are standing in, whether we are ground-sitting, and the friend roster.
///
/// Every menu in this crate opens against exactly these — the entries it greys
/// are a function of who we are, where we are, what we are doing and who we
/// know.
#[derive(Debug, bevy::ecs::system::SystemParam)]
pub(crate) struct MenuConditionFacts<'w> {
    /// Our own agent, which decides what "me" entries apply.
    pub(crate) identity: Res<'w, SlIdentity>,
    /// The parcel we are standing in, which gates its own entries.
    pub(crate) parcel: Res<'w, SlAgentParcel>,
    /// Whether we are ground-sitting, which flips Sit / Stand.
    pub(crate) ground_sit: Res<'w, SelfGroundSit>,
    /// The friend roster, which splits Add / Remove Friend.
    pub(crate) friends: Res<'w, FriendsModel>,
}
