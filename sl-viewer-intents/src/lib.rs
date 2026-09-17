//! What one surface asks another to do, without naming who answers.
//!
//! Messages a surface writes to ask for something it does not own — block this
//! resident, open that profile, pick a texture, drop this item on that object,
//! edit that notecard. Each is read by a feature that sits far from the ones
//! asking: `RequestBlock` alone is written from avatar menus, the radar, the
//! minimap, the profile, the inspector, the friends list, three kinds of toast
//! and a `secondlife:///` link.
//!
//! They live in their own crate so asking does not mean depending on the
//! answer — and so neither side has to reach through the world layer, which
//! has nothing to do with either of them. Every payload is an id or a string
//! `sl-client-bevy` already owns.

pub mod drag_drop;
pub mod requests;

pub use drag_drop::*;
pub use requests::*;
