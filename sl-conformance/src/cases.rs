//! The concrete conformance test implementations.
//!
//! Each submodule defines one [`GridTest`](crate::registry::GridTest). The
//! bodies exercise the same features as the `sl-client-tokio` examples but
//! report through the [`Metrics`](crate::metrics::Metrics) collector instead of
//! stdout, so the runner can stamp and store the result.

pub mod asset_decode;
pub mod chat_hear_other;
pub mod chat_invite_accept_decline;
pub mod chat_self_echo;
pub mod chat_whisper_shout_range;
pub mod draw_distance;
pub mod friendship_offer_accept;
pub mod group_session_message;
pub mod im_1to1;
pub mod im_typing;
pub mod inventory_fetch;
pub mod keepalive_ping;
pub mod login_handshake;
pub mod logout_clean;
pub mod offline_msg_fetch;
pub mod region_info;
pub mod session_mark_read;
pub mod throttle_set;
pub mod typing_indicator;
